#!/usr/bin/env python3
"""Verify self-hosted static storage, constant data and C ABI linkage."""
import argparse
import json
import os
import subprocess
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--cc', default=os.environ.get('CC', 'clang'))
args = p.parse_args()
tool, work = args.compiler.resolve(), args.work_dir.resolve()
work.mkdir(exist_ok=True, parents=True)
fixtures = Path(__file__).resolve().parent/'regressions/bootstrap/storage'
records = []
def run(label, cmd, code=0):
    result = subprocess.run(list(map(str, cmd)), capture_output=True, text=True,
                            cwd=work, timeout=60, env={**os.environ, 'CC':args.cc})
    records.append(dict(label=label, command=result.args, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work/'results.json').write_text(json.dumps(records, indent=2))
    assert result.returncode == code, (label, result)
    return result

def write(name, content):
    path = work/name
    path.write_text(content)
    return path

host_obj = work/'host.o'
run('fixture host object', [args.cc, '-c', fixtures/'host.c', '-o', host_obj])
# Two objects with coincident private names and exported functions with small signed results.
source_a = write('a.aeth', '''#![no_main]
static mut VALUE:i64=11;
pub static LEGACY:i64=31;
extern "C"{static mut ABI:i64; fn signed_value(x:i8)->i8;}
#[no_mangle] static mut ABI:i64=41;
#[no_mangle] fn addr_a()->*i64{return &mut VALUE as *i64;}
#[no_mangle] fn signed_value(x:i8)->i8{return x-2;}
#[no_mangle] fn read_global()->i64{return ABI;}
''')
source_b = write('b.aeth', '''#![no_main]
static mut VALUE:i64=22;
#[no_mangle] fn addr_b()->*i64{return &mut VALUE as *i64;}
''')
oracle = write('oracle.c', '''#include <stdint.h>
extern int64_t *addr_a(void), *addr_b(void), read_global(void), ABI;
extern const int64_t __aeth_m0_LEGACY;
extern int8_t signed_value(int8_t);
int main(void) {
    int64_t *a=addr_a(), *b=addr_b();
    if(a==b || *a!=11 || *b!=22 || ABI!=41 || __aeth_m0_LEGACY!=31) return 1;
    *b=4; *a=5; ABI=55;
    return *addr_a()==5 && *addr_b()==4 && read_global()==55 && signed_value(-7)==-9 ? 0 : 2;
}''')
for backend in ('c','llvm'):
    for level in (0,2,3):
        binary = work/f'storage-{backend}-{level}'
        run('build '+binary.name, [tool,'build',fixtures/'app.aeth','--backend',backend,
                                   f'-O{level}','--link-object',host_obj,'-o',binary])
        run('run '+binary.name,[binary])
    objects = []
    for path in (source_a, source_b):
        obj = work/f'{path.stem}-{backend}.o'
        run('emit '+obj.name,[tool,path,'--emit-obj','--backend',backend,'-O2','-o',obj])
        objects.append(obj)
    binary = work/f'link-{backend}'
    run('oracle link '+backend,[args.cc,oracle,*objects,'-o',binary])
    run('oracle execute '+backend,[binary])
    out = work/('data.c' if backend=='c' else 'data.ll')
    run('compact BSS '+backend,[tool,fixtures/'app.aeth',f'--emit-{"c" if backend=="c" else "llvm"}','-o',out])
    assert out.stat().st_size < 60000
    for label, text in [
        ('immutable','static VALUE:i32=4;fn main(){VALUE=5;}'),
        ('runtime init','fn get()->i32{return 2;}static NUMBER:i32=get();fn main(){}'),
        ('out of bounds','static mut A:[i64;2];static P:*i64=&mut A[2] as *i64;fn main(){}'),
        ('declaration mutability','extern "C"{static mut ABI:i32;}#[no_mangle] static ABI:i32=1;fn main(){}'),
        ('declaration type','extern "C"{static ABI:i32;}#[no_mangle] static ABI:i64=1;fn main(){}'),
        ('declaration kind','extern "C"{fn ABI();}#[no_mangle] static ABI:i32=1;fn main(){}'),
        ('two definitions','#[no_mangle] static ABI:i32=1;#[no_mangle] static ABI:i32=2;fn main(){}'),
        ('generic exported symbol','#[no_mangle] fn f<T>(x:T)->T{return x;}fn main(){}'),
        ('undeclared attribute','#[maybe_export] fn f(){}fn main(){}'),
        ('nonconstant address','static BASE:*i32;static P:*i32=&mut *BASE as *i32;fn main(){}'),
    ]:
        bad = write('bad.aeth',text)
        run(backend+'/check/'+label,[tool,'check',bad,'--backend',backend],1)
print(f'{len(records)} Core static storage checks passed')
