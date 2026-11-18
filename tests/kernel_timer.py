#!/usr/bin/env python3
"""Boot a direct-LLVM x86-64 guest and require distinct hardware timer samples of ordinary Aether work."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--seed', required=True, type=Path, help='Aether compiler with x86-64 LLVM support')
parser.add_argument('--work-dir', required=True, type=Path)
args = parser.parse_args()
compiler = args.seed.resolve(strict=True)
repo = Path(__file__).resolve().parents[1]
kernel = repo/'examples/kernel'
target = 'x86_64-unknown-none'
args.work_dir.mkdir(exist_ok=True, parents=True)
work = Path(tempfile.mkdtemp(prefix='timer-', dir=args.work_dir.resolve()))
report = {'target': target, 'compiler': str(compiler),
          'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
          'passed': False, 'runs': [], 'commands': []}

def tool(name):
    prefix = os.getenv('AETHER_LLVM_PREFIX') or os.getenv('LLVM_SYS_211_PREFIX')
    if prefix and (Path(prefix)/'bin'/name).is_file():
        return str(Path(prefix)/'bin'/name)
    path = shutil.which(name)
    if not path:
        raise RuntimeError(f'{name} is required')
    return path

def run(argv, expected=0):
    command = list(map(str, argv))
    item = {'argv': command}
    report['commands'].append(item)
    result = subprocess.run(command, cwd=work, env={**os.environ, 'TMPDIR': str(work)},
                            capture_output=True, text=True, timeout=20)
    item.update(exit=result.returncode, stderr=result.stderr)
    if result.returncode != expected:
        raise RuntimeError(f'{command[0]} returned {result.returncode}, expected {expected}\n{result.stderr}')
    return result.stdout

def test(level):
    name = f'timer-O{level}'
    obj, elf, image, console = [work/(name+s) for s in ('.o','.elf','.bin','.log')]
    run([compiler, kernel/'timer.aeth', '--backend', 'llvm', '--target', target,
         f'-O{level}', '--emit-obj', '-o', obj])
    run([tool('ld.lld'), '-T', repo/'examples/kernel.ld', work/'boot.o', obj, '-o', elf])
    symbols = run([tool('llvm-nm'), '-S', elf])
    worker = re.search(r'^([0-9a-fA-F]+) ([0-9a-fA-F]+) T timer_work$', symbols, re.M)
    if not worker:
        raise RuntimeError('timer_work function range missing from the linked ELF')
    address, size = (int(v, 16) for v in worker.groups())
    run([tool('llvm-objcopy'), '-O', 'binary', elf, image])
    run([tool('qemu-system-x86_64'), '-machine', 'pc,accel=tcg', '-smp', '1', '-m', '32M',
         '-icount', 'shift=3,sleep=off', '-nodefaults', '-display', 'none', '-no-reboot',
         '-debugcon', f'file:{console}', '-device', 'isa-debug-exit,iobase=0xf4,iosize=4',
         '-kernel', image], expected=33)
    line = console.read_text(encoding='ascii').strip()
    match = re.fullmatch(r'timer ticks=(\d+) samples=(\d+) steps=(\d+) sampled_steps=(\d+) ip=(\d+) checksum=(\d+)',line)
    if not match:
        raise RuntimeError(f'invalid guest results: {line!r}')
    ticks, samples, steps, sampled, ip, checksum = map(int, match.groups())
    cycles, tail = divmod(steps, 1024)
    if (samples != 8 or ticks < samples or not samples <= sampled <= steps
            or not address <= ip < address+size or checksum != cycles*523776+tail*(tail+1)//2):
        raise RuntimeError(f'failed timer/worker checks: {line}, worker={address:#x}+{size:#x}')
    report['runs'].append({'opt': level, 'ticks': ticks, 'progress_samples': samples,
                           'steps': steps, 'sampled_steps': sampled, 'interrupted_ip': ip,
                           'worker_address': address, 'worker_bytes': size, 'checksum': checksum,
                           'image_sha256': hashlib.sha256(image.read_bytes()).hexdigest()})
    print(f'O{level}: {line}', flush=True)

try:
    run([tool('clang'), '--target='+target, '-c',kernel/'timer_boot.S','-o',work/'boot.o'])
    for opt in (0, 2):
        test(opt)
    report['passed'] = True
except Exception as error:
    report['error'] = str(error)
    raise
finally:
    receipt = work/'results.json'
    receipt.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(receipt, flush=True)
