#!/usr/bin/env python3
"""Check the self-hosted x86-64 bare-metal LLVM target and its C calling convention."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--cc', default=os.environ.get('CC', 'clang'))
p.add_argument('--qemu', action='store_true')
args = p.parse_args()
compiler = args.compiler.resolve(strict=True)
cc = shutil.which(args.cc)
if not cc:
    p.error('a target-capable Clang is required')
args.work_dir.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(dir=args.work_dir.resolve(), prefix='kernel-target-'))
target = 'x86_64-unknown-none'
repo = Path(__file__).resolve().parents[1]
abi = repo/'tests/regressions/bootstrap/native_abi'
example = repo/'examples/kernel'
records = []


def run(argv, expected=0):
    result = subprocess.run(list(map(str, argv)), env={**os.environ, 'TMPDIR': str(work)},
                            cwd=work, capture_output=True, text=True, timeout=30)
    records.append(dict(argv=result.args, status=result.returncode, expected=expected, stderr=result.stderr))
    (work/'results.json').write_text(json.dumps(records, indent=2)+'\n', encoding='utf-8')
    assert result.returncode == expected, (result.args, result.stderr, result.stdout)
    return result


def llvm_tool(name):
    full = Path(cc).parent/name
    return str(full) if full.is_file() else shutil.which(name) or name


# Reuse the bidirectional SysV ABI oracle, including register exhaustion,
# mixed int/SSE aggregates, sret, packed-integer argument pieces and varargs.
source = work/'cross.aeth'
source.write_text('#![no_std]\n'+(abi/'values.aeth').read_text(), encoding='utf-8')
flags = ['--target', target, '--backend', 'llvm', '--cc', cc]
run([compiler, 'check', source, *flags])
if args.qemu:
    cflags = [cc, '--target='+target, '-ffreestanding', '-fno-stack-protector', '-mno-red-zone', '-O2']
    entry = work/'entry.c'
    entry.write_text('extern int main(int, char **);\n'
                     'int kernel_main(void *base, unsigned pages){ return pages == 4 ? main(0,0) : 1; }\n')
    for src, name in ((entry, 'entry'), (abi/'oracle.c', 'oracle'), (example/'memory.c', 'memory')):
        run([*cflags, '-c', src, '-o', name+'.o'])
    run([*cflags, '-DAETHER_BOOT_SSE2', '-c', example/'pages_boot.S', '-o', 'boot.o'])

for opt in ('-O0','-O2'):
    ir, obj = (work/('cross'+opt+suffix) for suffix in ('.ll','.o'))
    run([compiler, source, '--target='+target, '--emit-llvm', opt, '--cc', cc, '-o', ir])
    text = ir.read_text()
    assert f'target triple = "{target}"' in text
    assert 'target datalayout = "e-m:e-' in text
    assert 'byval(' in next(line for line in text.splitlines() if line.startswith('declare') and '@spill(' in line)
    protected = set(re.findall(r'^attributes #(\d+) = \{[^}]*\bnoredzone\b', text, re.M))
    for definition in re.findall(r'^define [^\n]+', text, re.M):
        group = re.search(r'#(\d+)\s*\{', definition)
        assert ' noredzone' in definition or group and group[1] in protected, definition
    run([compiler, source, *flags, '--emit-obj', opt, '-o', obj])
    header = obj.read_bytes()[:64]
    assert header[:7] == b'\x7fELF\x02\x01\x01' and header[16:20] == b'\x01\x00\x3e\x00'
    if args.qemu:
        image = work/('abi'+opt+'.bin')
        elf = image.with_suffix('.elf')
        run([llvm_tool('ld.lld'), '-T', repo/'examples/kernel.ld', 'boot.o','entry.o',
             'memory.o','oracle.o', obj, '-o', elf])
        run([llvm_tool('llvm-objcopy'), '-O', 'binary', elf, image])
        run(['qemu-system-x86_64', '-machine', 'pc,accel=tcg', '-m', '32M', '-nodefaults',
             '-display', 'none', '-no-reboot', '-device', 'isa-debug-exit,iobase=0xf4,iosize=4',
             '-kernel', image], expected=33)

# Every rejection preserves the output that existed before the command.
old = work/'preserved'
old.write_bytes(b'previous')
small = work/'small.aeth'
for contents, extra, diagnostic in (
    ('fn main()->i32{ return 0; }', [*flags,'--emit-obj'], 'no_std'),
    ('#![no_std] fn main()->i32{ return __errno_get(); }', [*flags,'--emit-llvm'], 'errno'),
    ('#![no_std] fn main()->i32{ return 0; }', ['--target','aarch64-unknown-none','--emit-llvm'], 'supported cross target'),
    ('#![no_std] fn main()->i32{ return 0; }', [*flags], '--emit-obj'),
    ('#![no_std] fn main()->i32{ return 0; }', ['--target',target,'--emit-c'], 'LLVM'),
    ('#![no_std] fn main()->i32{ return 0; }', [*flags,'--target',target,'--emit-obj'], 'duplicate'),
):
    small.write_text(contents)
    failed = run([compiler, small, *extra, '-o', old], expected=1)
    assert diagnostic in failed.stderr and old.read_bytes() == b'previous'
# Wrap the actual assembler, deliberately returning an ELF64 object of a
# different architecture. A successful tool exit must not publish that file.
if os.name == 'posix':
    import shlex
    wrong = work/'wrong-target-cc'
    wrong.write_text('#!/bin/sh\nexec '+shlex.quote(cc)+' "$@" --target=aarch64-unknown-none\n')
    wrong.chmod(0o755)
    failed = run([compiler, small, '--target', target, '--emit-obj', '--cc', wrong,
                  '-o', old], expected=1)
    assert 'invalid native output' in failed.stderr and old.read_bytes() == b'previous'
    assert not list(work.glob('preserved.tmp.*'))
print(f'{len(records)} checks passed; {work}/results.json')
