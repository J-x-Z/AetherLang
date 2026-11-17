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


# Independent bindings for a shared C symbol must preserve the Aether spelling
# at each use, including reexports and qualified static accesses.
def abi_views(bits):
    fixed, signed = f'u{bits}', f'i{bits}'
    (work/'sized.aeth').write_text('extern "C" { fn bridge(p:*isize, n:usize)->usize; static mut counter:usize; }')
    (work/'fixed.aeth').write_text(f'extern "C" {{ fn bridge(p:*{signed}, n:{fixed})->{fixed}; static mut counter:{fixed}; }}')
    (work/'alias.aeth').write_text('pub use sized::bridge as count;')
    module = work/'abi_views.aeth'
    module.write_text(f'''#![no_std] #![no_main]
use alias::count; use fixed::bridge as fixed_count; mod sized; mod fixed;
#[no_mangle] static mut counter:usize=12;
#[no_mangle] fn bridge(p:*isize,n:usize)->usize {{ unsafe {{ n + (*p as usize) }} }}
#[no_mangle] fn verify()->i32 {{
    let a:isize=2; let b:{signed}=3;
    let first:usize=count(&a as *isize, sized::counter);
    let second:{fixed}=fixed_count(&b as *{signed}, fixed::counter);
    if first==14 && second==15 {{0}} else {{1}}
}}''')
    return module

targets = {'i686-unknown-none-elf': 32, 'x86_64-unknown-linux-gnux32': 32,
           'x86_64-unknown-linux-gnu': 64, 'wasm32-unknown-unknown': 32}
for triple, bits in targets.items():
    abi = abi_views(bits)
    run([compiler, 'check', abi, '--target', triple])
    for invalid in (f'i{bits}', f'u{96-bits}', 'f32', 'char'):
        negative = work/'abi_conflict.aeth'
        negative.write_text(f'extern "C" {{fn clash(p:*usize);}} extern "C" {{fn clash(p:*{invalid});}} fn main(){{}}')
        result = run([compiler, 'check', negative, '--target', triple], 1)
        assert 'E2202' in result.stderr, result.stderr
    strict = work/'ordinary.aeth'
    strict.write_text(f'fn ordinary(a:usize)->u{bits}{{a}} fn main(){{}}')
    assert 'E2100' in run([compiler,'check',strict,'--target',triple],1).stderr
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
        views = abi_views(32)
        abi_obj = work / f'abi-views-{backend}.o'
        abi_wasm = abi_obj.with_suffix('.wasm')
        run([compiler, views, '--target', 'wasm32-unknown-unknown', '--backend', backend,
             '--cc','clang','--emit-obj','-O2','-o',abi_obj])
        run(['wasm-ld','--no-entry','--export=verify',abi_obj,'-o',abi_wasm])
        run(['node','-e',"const fs=require('fs'); const m=new WebAssembly.Module(fs.readFileSync(process.argv[1])); process.exit(new WebAssembly.Instance(m).exports.verify());",abi_wasm])
        const_source = Path(__file__).resolve().parent / 'regressions/layout/const_width.aeth'
        const_obj = work / f'const-layout-{backend}.o'
        const_wasm = const_obj.with_suffix('.wasm')
        run([compiler, const_source, '--target', 'wasm32-unknown-unknown', '--backend', backend,
             '--cc', 'clang', '--emit-obj', '-O2', '-o', const_obj])
        run(['wasm-ld', '--no-entry', '--export=verify_const_layout', const_obj, '-o', const_wasm])
        run(['node', '-e', "const fs=require('fs'); process.exit(new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1]))).exports.verify_const_layout());", const_wasm])
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
