#!/usr/bin/env python3
"""Check target-dependent integer types, and execute 32-bit code if wasm tools are available."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['llvm', 'c'])
args = p.parse_args()
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
source = Path(__file__).resolve().parent / 'regressions/layout/pointer_width.aeth'
records = []

def run(command, expected=0):
    cmd = list(map(str, command))
    result = subprocess.run(cmd, text=True, capture_output=True, timeout=30)
    records.append(dict(command=cmd, status=result.returncode, expected=expected,
                        stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert result.returncode == expected, f'{cmd}: {result.stderr} {result.stdout}'
    return result

targets = {'i686-unknown-none-elf': 32, 'x86_64-unknown-linux-gnux32': 32,
           'x86_64-unknown-linux-gnu': 64, 'wasm32-unknown-unknown': 32}
for triple, bits in targets.items():
    for ty, literal in [('usize', '4294967296'), ('isize', '-2147483649')]:
        program = work / f'{ty}.aeth'
        program.write_text(f'#![no_main]\nfn value()->{ty} {{ {literal} }}')
        result = run([compiler, 'check', program, '--target', triple], 0 if bits == 64 else 1)
        if bits == 32:
            assert 'representable' in result.stderr, result.stderr
    for backend in args.backends:
        emitted = work / f'{triple}-{backend}.txt'
        mode = '--emit-llvm' if backend == 'llvm' else '--emit-c'
        run([compiler, source, '--backend', backend, '--target', triple, mode, '-O2', '-o', emitted])
        output = emitted.read_text()
        if backend == 'llvm':
            for name in ['signed_width', 'unsigned_width']:
                assert re.search(rf'define .*?i{bits} @{name}\(i{bits} ', output), (name, output)
            assert f'ret i32 {bits // 8}' in output, output
        else:
            assert f'int{bits}_t signed_width(int{bits}_t ' in output, output
            assert f'uint{bits}_t unsigned_width(uint{bits}_t ' in output, output

# Both are external runtime tools. The type and emission checks above also run
# on hosts without them. No 32-bit native process is needed on macOS arm64.
if shutil.which('node') and shutil.which('wasm-ld') and shutil.which('clang'):
    for backend in args.backends:
        obj = work / f'layout-{backend}.o'
        wasm = obj.with_suffix('.wasm')
        run([compiler, source, '--target', 'wasm32-unknown-unknown', '--backend', backend,
             '--cc', 'clang', '--emit-obj', '-O2', '-o', obj])
        run(['wasm-ld', '--no-entry', '--export=layout_result', obj, '-o', wasm])
        run(['node', '-e', """const fs = require('fs');
const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])));
if (instance.exports.layout_result() !== 4) process.exit(1);
""", wasm])
else:
    print('32-bit execution skipped: requires node, wasm-ld and clang')
print(f'{len(records)} target layout commands matched their expected status')
