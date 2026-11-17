#!/usr/bin/env python3
"""Native static storage, union representation and target layout regressions."""
import argparse
import json
import os
import platform
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', default=['c', 'llvm'])
args = parser.parse_args()
root = Path(__file__).resolve().parent / 'regressions/system'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
env = {**os.environ, 'TMPDIR': str(work)}
records = []
cc = os.environ.get('CC', 'clang')

def run(label, command, expected=0, environment=env):
    result = subprocess.run(list(map(str, command)), env=environment, text=True, capture_output=True, timeout=30)
    records.append(dict(label=label, command=list(map(str, command)), exit=result.returncode, stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    valid = result.returncode == 0 if expected == 0 else result.returncode != 0
    assert valid, f'{label}: {result.returncode}\n{result.stdout}\n{result.stderr}'
    return result

source = root / 'globals_unions.aeth'
obj = work / 'native.o'
run('native C globals', [cc, '-std=c11', '-O2', '-c', root / 'globals_unions.c', '-o', obj])
run('check globals', [compiler, 'check', source])
for backend in args.backends:
    actual_source = source
    if backend == 'llvm' and not ((platform.system() == 'Darwin' and platform.machine() in ('arm64', 'aarch64')) or (platform.system() in ('Darwin', 'Linux') and platform.machine() in ('x86_64', 'AMD64'))):
        unsupported = run('LLVM foreign aggregate ABI unsupported', [compiler, source, '--backend', 'llvm', '--emit-obj', '-o', work / 'unsupported.o'], expected=1)
        assert 'aggregate C ABI lowering is currently implemented for Apple arm64' in unsupported.stderr
        # Keep all static/union/layout checks on this target, with the value
        # transformation performed by an internal language function.
        actual_source = work / 'globals-internal.aeth'
        actual_source.write_text(source.read_text().replace('    fn word_flip(value: Word) -> Word;\n', '') + '\nfn word_flip(value:Word)->Word { unsafe { return Word { bits: value.bits ^ (2147483648 as u32) }; } }\n')
    for level in [0, 2]:
        binary = work / f'globals-{backend}-O{level}'
        run(f'{backend}/O{level}/build', [compiler, actual_source, '--backend', backend, f'-O{level}', '--link-object', obj, '-o', binary])
        result = run(f'{backend}/O{level}/run', [binary])
        assert result.stdout == '' and result.stderr == ''

csource, sanitized = work / 'globals-sanitized.c', work / 'globals-sanitized'
run('emit C', [compiler, source, '--emit-c', '-o', csource])
run('ASan UBSan build', [cc, '-std=c11', '-O0', '-g', '-fno-inline', '-fsanitize=address,undefined', csource, root / 'globals_unions.c', '-o', sanitized])
run('ASan UBSan run', [sanitized], environment={**env, 'ASAN_OPTIONS':'detect_stack_use_after_return=1', 'UBSAN_OPTIONS':'halt_on_error=1'})

for name, text in {
    'union_all_fields': 'union U { a:i32,b:i64 } fn main(){let u:U=U{a:1,b:2};}',
    'union_missing_field': 'union U { a:i32,b:i64 } fn main(){let u:U=U{};}',
    'static_runtime_initializer': 'fn value()->i64{return 42;} static X:i64=value(); fn main(){}',
    'static_reference_zero': 'static X:&i64; fn main(){}',
    'static_immutable_write': 'static X:i64=1; fn main(){unsafe {X=2;}}',
    'static_address_bounds': 'static mut X:[i32;2]=[1,2]; static P:*i32=&mut X[2] as *i32; fn main(){}',
    'volatile_implicit_drop': 'type Reg=*volatile u32; fn plain(p:*u32){} fn f(p:Reg){plain(p);} fn main(){}',
    'volatile_return_drop': 'fn f(p:*volatile u32)->*u32{return p;} fn main(){}',
    'unknown_layout_field': 'struct S{a:i32} fn main(){let x:usize=offset_of(S,b);}',
}.items():
    path = work / f'{name}.aeth'
    path.write_text(text)
    run(f'{name}/reject', [compiler, 'check', path], expected=1)

# Unit is a zero-sized value, including generic payloads and stored arrays.
unit = root / 'unit_values.aeth'
run('check unit', [compiler, 'check', unit])
for backend in args.backends:
    for level in [0, 2]:
        binary = work / f'unit-{backend}-O{level}'
        run(f'unit/{backend}/O{level}/build', [compiler, unit, '--backend', backend, f'-O{level}', '-o', binary])
        run(f'unit/{backend}/O{level}/run', [binary])

volatile = root / 'typed_volatile.aeth'
run('typed volatile/check', [compiler, 'check', volatile])
for backend in args.backends:
    for level in [0, 2]:
        binary = work / f'volatile-{backend}-O{level}'
        run(f'volatile/{backend}/O{level}/build', [compiler, volatile, '--backend', backend, f'-O{level}', '-o', binary])
        run(f'volatile/{backend}/O{level}/run', [binary])
volatile_c = work / 'volatile-sanitized.c'
volatile_binary = work / 'volatile-sanitized'
run('volatile emit C', [compiler, volatile, '--emit-c', '-o', volatile_c])
run('volatile sanitizer build', [cc, '-std=c11', '-O0', '-g', '-fno-inline', '-fsanitize=address,undefined', volatile_c, '-o', volatile_binary])
run('volatile sanitizer run', [volatile_binary], environment={**env, 'UBSAN_OPTIONS':'halt_on_error=1'})
if 'llvm' in args.backends:
    import re
    ir = work / 'volatile-O2.ll'
    run('volatile LLVM O2', [compiler, volatile, '--emit-llvm', '-O2', '-o', ir])
    text = ir.read_text()
    for name in ['typed_read_twice', 'annotated_read_twice']:
        body = re.search(r'define[^\n]*@' + name + r'\([^\n]*\)[^{]*\{(.*?)^}', text, re.S | re.M)
        assert body, f'missing exported function {name}'
        assert len(re.findall(r'load volatile i32,', body[1])) == 2, f'{name}: two volatile reads must remain at O2'

# Array lengths and union padding remain 64-bit even without an allocated value.
# The executable contains only layout assertions, never a 4 GiB object.
large_layout = root / 'large_layout.aeth'
run('large layout/check', [compiler, 'check', large_layout])
for backend in args.backends:
    for level in [0, 2]:
        binary = work / f'large-layout-{backend}-O{level}'
        run(f'large layout/{backend}/O{level}/build', [compiler, large_layout, '--backend', backend, f'-O{level}', '-o', binary])
        run(f'large layout/{backend}/O{level}/run', [binary])

if 'llvm' in args.backends:
    # These globals reference an external array; emitting IR requires no 4 GiB
    # allocation/link. The two offsets must be positive, pointer-width values.
    address_ir = work / 'large-static-address.ll'
    run('large static address/LLVM', [compiler, root / 'large_static_address.aeth', '--emit-llvm', '-O0', '-o', address_ir])
    address_text = address_ir.read_text()
    assert '[4294967297 x i8]' in address_text
    assert 'i64 2147483648' in address_text and 'i64 4294967296' in address_text
    assert 'i32 -2147483648' not in address_text

payload = root / 'union_payload_bytes.aeth'
run('union payload/check', [compiler, 'check', payload])
for backend in args.backends:
    for level in [0, 2]:
        binary = work / f'union-payload-{backend}-O{level}'
        run(f'union payload/{backend}/O{level}/build', [compiler, payload, '--backend', backend, f'-O{level}', '-o', binary])
        run(f'union payload/{backend}/O{level}/run', [binary])

for backend in args.backends:
    for level in [0,2]:
        binary=work/f'bitnot-{backend}-{level}'
        run('bitwise not build',[compiler,root/'bitwise_not.aeth','--backend',backend,f'-O{level}','-o',binary])
        run('bitwise not run',[binary])

print(f'{len(records)} system checks passed; evidence: {work / "results.json"}')
