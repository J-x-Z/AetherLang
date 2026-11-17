#!/usr/bin/env python3
"""Private static isolation and explicit/public storage across native objects."""
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
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
cc = os.environ.get('CC', 'clang')
records = []

def run(label, command, failure=False):
    result = subprocess.run(list(map(str, command)), env=env, text=True,
                            capture_output=True, timeout=30)
    records.append(dict(label=label, command=list(map(str, command)), exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert (result.returncode == 0) != failure, f'{label}: {result.returncode}\n{result.stdout}\n{result.stderr}'
    return result

def source(name, text):
    path = work / name
    path.write_text(text)
    return path

# Neither Aether compilation sees the host's same-spelling symbol. Each object
# must keep its own private storage, even when its address escapes to C.
first = source('first.aeth', '''#![no_main]
mod declaration { extern "C" { static mut EXPORTED:i64; } }
static mut VALUE:i64=11;
static POINTER:*i64=&mut VALUE as *i64;
pub static LEGACY:i64=31;
#[no_mangle] static mut EXPORTED:i64=41;
#[no_mangle] fn first_address()->*i64 { unsafe { return POINTER; } }
#[no_mangle] fn exported_value()->i64 { unsafe { return EXPORTED; } }
''')
second = source('second.aeth', '''#![no_main]
static mut VALUE:i64=22;
static POINTER:*i64=&mut VALUE as *i64;
#[no_mangle] fn second_address()->*i64 { unsafe { return POINTER; } }
''')
host = source('host.c', '''#include <stdint.h>
int64_t __aeth_m0_VALUE=90;
extern int64_t *first_address(void), *second_address(void);
extern int64_t exported_value(void), EXPORTED;
extern const int64_t __aeth_m0_LEGACY;
int main(void) {
    int64_t *a=first_address(), *b=second_address();
    if(a==b || a==&__aeth_m0_VALUE || b==&__aeth_m0_VALUE) return 1;
    if(*a!=11 || *b!=22 || __aeth_m0_VALUE!=90) return 2;
    *a=101; *b=202;
    if(*first_address()!=101 || *second_address()!=202 || __aeth_m0_VALUE!=90) return 3;
    if(__aeth_m0_LEGACY!=31 || EXPORTED!=41 || exported_value()!=41) return 4;
    EXPORTED=42;
    return exported_value()==42 ? 0 : 5;
}
''')
for backend in args.backends:
    objects = []
    for path in [first, second]:
        obj = work / f'{path.stem}-{backend}.o'
        run(f'{backend}/{path.stem}/object', [compiler, path, '--backend', backend, '-O2', '--emit-obj', '-o', obj])
        objects.append(obj)
    binary = work / f'linkage-{backend}'
    run(f'{backend}/host link', [cc, '-std=c11', '-O2', host, *objects, '-o', binary])
    result = run(f'{backend}/run', [binary])
    assert not result.stdout and not result.stderr

# Reject conflicting ABI definitions before either backend is selected. Include
# separate source modules: ordinary module mangling must not hide the collision.
for name, text, diagnostic in [
    ('type', 'extern "C" {static mut ABI:i32;} #[no_mangle] static mut ABI:i64=1;', 'conflicting foreign'),
    ('mutability', 'extern "C" {static ABI:i64;} #[no_mangle] static mut ABI:i64=1;', 'conflicting foreign'),
    ('kind', 'extern "C" {fn ABI();} #[no_mangle] static mut ABI:i64=1;', 'conflicting foreign'),
    ('two_definitions', 'mod a {#[no_mangle] static ABI:i64=1;} mod b {#[no_mangle] static ABI:i64=2;}', 'Duplicate definition'),
    ('function_definition', 'mod a {#[no_mangle] static ABI:i64=1;} mod b {#[no_mangle] fn ABI(){}}', 'Duplicate definition'),
]:
    path = source(f'reject-{name}.aeth', text + ' fn main(){}')
    result = run(f'{name}/reject', [compiler, 'check', path], failure=True)
    assert diagnostic.lower() in result.stderr.lower(), result.stderr

print(f'{len(records)} global linkage checks passed; evidence: {work / "results.json"}')
