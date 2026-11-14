#!/usr/bin/env python3
"""Native reference/initialization semantics and public check/build rejections."""
import argparse
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
env = {**os.environ, 'TMPDIR': str(work)}
source = Path(__file__).resolve().parent / 'regressions/semantics/values.aeth'
records = []

def run(label, command, success=True):
    command = list(map(str, command))
    result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=30)
    records.append(dict(label=label, command=command, status=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert (result.returncode == 0) == success, f'{label}: {result.returncode}\n{result.stdout}\n{result.stderr}'
    return result

run('legal/check', [compiler, 'check', source])
for backend in args.backends:
    for opt in [0, 2]:
        executable = work / f'values-{backend}-O{opt}'
        run(f'legal/{backend}/O{opt}/build', [compiler, 'build', source, '--backend', backend, f'-O{opt}', '-o', executable])
        result = run(f'legal/{backend}/O{opt}/run', [executable])
        assert result.stdout == '' and result.stderr == ''

negative = {
    'uninitialized': 'fn main()->i32{let value:i32;value}',
    'partial_branch': 'fn f(b:bool)->i32{let value:i32;if b{value=1;}value}fn main()->i32{f(false)}',
    'zero_iteration': 'fn main()->i32{let mut value:i32;while false{value=1;}value}',
    'partial_aggregate': 'struct Pair{x:i32,y:i32}fn main()->i32{let p:Pair;p.x=1;p.y}',
    'return_local': 'fn bad()->&i32{let value:i32=1;&value}fn main()->i32{*bad()}',
    'return_field': 'struct View{item:&i32}fn bad()->View{let x:i32=1;View{item:&x}}fn main()->i32{0}',
    'return_parameter_wrapper': 'fn id(p:&i32)->&i32{p}fn bad()->&i32{let x:i32=1;id(&x)}fn main()->i32{0}',
    'parameter_write_escape': 'fn set(out:&mut &i32,value:&i32){*out=value;}fn main()->i32{let a:i32=1;let mut r:&i32=&a;{let b:i32=2;set(&mut r,&b);}*r}',
    'borrow_write': 'fn main()->i32{let mut x:i32=1;let r:&i32=&x;x=2;*r}',
    'move_mutable_reference': 'fn main()->i32{let mut x:i32=1;let r:&mut i32=&mut x;let q:&mut i32=r;*q=2;*r}',
    'effect_wrapper': 'extern "C"{fn puts(p:*u8)->i32;}fn emit(){puts("x");}fn main()->i32 pure{emit();0}',
    'unknown_abi': 'extern "mystery"{fn external();}fn main()->i32{0}',
    'unknown_method': 'struct S{x:i32}fn main()->i32{S::missing();0}',
    'invalid_try': 'fn main()->i32{1?}',
    'type_as_value': 'struct S{x:i32}fn bad()->S{S}fn main()->i32{0}',
}
for name, text in negative.items():
    path = work / f'{name}.aeth'
    path.write_text(text)
    result = run(f'{name}/check', [compiler, 'check', '--error-format', 'json', path], success=False)
    diagnostic = json.loads(result.stderr)
    expected = ('E2301' if name in ['uninitialized', 'partial_branch', 'zero_iteration', 'partial_aggregate', 'move_mutable_reference']
                else {'effect_wrapper': 'E2200', 'unknown_abi': 'E2202', 'unknown_method': 'E2001',
                      'invalid_try': 'E2104', 'type_as_value': 'E2104'}.get(name, 'E2302'))
    assert diagnostic['code'] == expected, f'{name}: {diagnostic}'
    assert diagnostic.get('location'), f'{name}: missing source location' 
    for backend in args.backends:
        run(f'{name}/{backend}/build', [compiler, 'build', path, '--backend', backend, '-o', work / f'{name}-{backend}'], success=False)
print(f'{len(records)} checks passed; evidence: {work / "results.json"}')
