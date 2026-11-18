#!/usr/bin/env python3
"""Build the x86-64 page-pool guest and compare optional seed backends in QEMU."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--selfhost', required=True, type=Path, help='self-hosted Aether compiler')
parser.add_argument('--seed', type=Path, help='optional Rust seed for cross-target comparison')
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['llvm', 'c'],
                    help='backends built by each compiler; use c alone for historical self-hosts')
parser.add_argument('--work-dir', required=True, type=Path)
parser.add_argument('--qemu', action='store_true', help='also boot each image with QEMU TCG')
args = parser.parse_args()
repo = Path(__file__).resolve().parents[1]
example = repo/'examples/kernel'
args.work_dir.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='pages-', dir=args.work_dir.resolve()))
target = 'x86_64-unknown-none'
report = {'target': target, 'qemu': args.qemu, 'passed': False, 'commands': [], 'images': []}
receipt = work/'results.json'
env = {**os.environ, 'TMPDIR': str(work)}
print(f'Kernel build results: {receipt}', flush=True)


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
    start = time.monotonic()
    result = subprocess.run(command, env=env, cwd=work, timeout=30, capture_output=True, text=True)
    report['commands'].append({'argv': command, 'exit': result.returncode,
                               'seconds': time.monotonic()-start, 'stderr': result.stderr})
    if result.returncode != expected:
        raise RuntimeError(f'{command[0]} returned {result.returncode}, expected {expected}\n{result.stderr}')
    return result.stdout


def inspect_size(path):
    table = run([tool('llvm-size'), '-A', path])
    sections = re.findall(r'^\.text(?:\.\S+)?\s+(\d+)', table, re.MULTILINE)
    if not sections:
        raise ValueError(f'{path} has no text section')
    return sum(map(int, sections))


def build(compiler, lane, backend, level, clang, resource, boot, memory, scalar):
    stem = f'{lane}-{backend}-O{level}'
    obj, elf, image = [work/(stem+suffix) for suffix in ('.o','.elf','.bin')]
    flags = [compiler, example/'pages.aeth', '--backend', backend, f'-O{level}']
    if not scalar:
        flags += ['--target', target]
    if lane == 'selfhost' and backend == 'llvm':
        flags += ['--cc', clang]
    if backend == 'c':
        source = obj.with_suffix('.c')
        run([*flags, '--emit-c', '-o', source])
        cpu = ['-mgeneral-regs-only'] if scalar else []
        run([clang, '--target='+target, *cpu, '-std=c11', '-ffreestanding', '-nostdinc',
             '-isystem', resource/'include', '-mno-red-zone',
             '-fno-stack-protector', f'-O{level}', '-c', source, '-o', obj])
    else:
        run([*flags, '--emit-obj', '-o', obj])
    run([tool('ld.lld'), '-T', repo/'examples/kernel.ld', boot, obj, memory, '-o', elf])
    assert not run([tool('llvm-nm'), '-u', elf]).strip(), 'unresolved kernel symbol'
    symbols = run([tool('llvm-nm'), '-n', elf])
    end = re.search(r'^([\da-fA-F]+) \S __kernel_end$', symbols, re.MULTILINE)
    assert end and int(end[1], 16) <= 0x200000, 'guest exceeds its boot mapping'
    run([tool('llvm-objcopy'), '-O', 'binary', elf, image])
    item = {'build': stem, 'compiler': str(compiler), 'kernel_text_bytes': inspect_size(obj),
            'cpu_policy': 'general registers only' if scalar else 'x86-64 with SSE2 enabled',
            'linked_text_bytes': inspect_size(elf), 'image_bytes': image.stat().st_size,
            'image': str(image), 'sha256': hashlib.sha256(image.read_bytes()).hexdigest()}
    report['images'].append(item)
    if args.qemu:
        run([tool('qemu-system-x86_64'), '-machine', 'pc,accel=tcg', '-m', '32M',
             '-nodefaults', '-display', 'none', '-no-reboot', '-device',
             'isa-debug-exit,iobase=0xf4,iosize=4', '-kernel', image], expected=33)
        item['guest_exit'] = 33
    print(stem, item['kernel_text_bytes'], 'bytes of Aether text', flush=True)


try:
    clang = tool('clang')
    report['clang'] = run([clang, '--version']).splitlines()[0]
    resource = Path(run([clang, '-print-resource-dir']).strip())
    memory = work/'memory.o'
    run([clang, '--target='+target, '-std=c11', '-ffreestanding', '-fno-builtin',
         '-mgeneral-regs-only', '-mno-red-zone', '-fno-stack-protector', '-O2',
         '-c', example/'memory.c', '-o', memory])
    compilers = [(args.selfhost.resolve(strict=True), 'selfhost')]
    if args.seed:
        compilers.append((args.seed.resolve(strict=True), 'seed'))
    boots = {}
    for compiler, lane in compilers:
        for backend in args.backends:
            scalar = lane == 'selfhost' and backend == 'c'
            if scalar not in boots:
                boots[scalar] = work/('scalar-boot.o' if scalar else 'sse2-boot.o')
                flags = [] if scalar else ['-DAETHER_BOOT_SSE2']
                run([clang, '--target='+target, *flags, '-c', example/'pages_boot.S', '-o', boots[scalar]])
            for level in (0, 2):
                build(compiler, lane, backend, level, clang, resource, boots[scalar], memory, scalar)
    report['passed'] = True
finally:
    receipt.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
