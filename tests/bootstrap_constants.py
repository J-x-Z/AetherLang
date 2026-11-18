#!/usr/bin/env python3
"""Self-hosted typed const generics, checked dimensions and numeric consumers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--formats',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--sanitize',action='store_true')
p.add_argument('--cc',default=os.environ.get('CC','clang'))
a=p.parse_args();root=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
compiler=a.compiler.resolve();cc=shutil.which(a.cc);records=[];env={**os.environ,'TMPDIR':str(w)}
if not cc:raise SystemExit('native C compiler unavailable')

def record(name,passed,**facts):
    records.append(dict(name=name,passed=bool(passed),**facts));(w/'results.json').write_text(json.dumps(records,indent=2)+'\n')
    print(('PASS ' if passed else 'FAIL ')+name,flush=True)
    if not passed:raise SystemExit(name)

def run(name,cmd,status=0,stdout=None,diagnostic=None):
    cmd=list(map(str,cmd));started=time.monotonic()
    try:r=subprocess.run(cmd,cwd=w,env=env,capture_output=True,timeout=45)
    except subprocess.TimeoutExpired:record(name,False,command=cmd,error='timeout')
    text=(r.stdout+r.stderr).decode(errors='replace')
    record(name,r.returncode==status and (stdout is None or r.stdout==stdout) and (diagnostic is None or diagnostic in text),
           command=cmd,exit=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'),elapsed_seconds=time.monotonic()-started)

(w/'compiler.json').write_text(json.dumps(dict(path=str(compiler),sha256=hashlib.sha256(compiler.read_bytes()).hexdigest()),indent=2))
module=w/'dimensions';module.mkdir(exist_ok=True)
(module/'numbers.aeth').write_text('type Width=usize;const BASE:Width=2;const WIDTH:Width=BASE+1;pub struct Block<T,const N:usize>{data:[T;N]}pub fn width<T,const N:usize>(b:Block<T,N>)->usize{N}')
(module/'main.aeth').write_text('use numbers::{WIDTH,Block,width};fn main()->i32{let b:Block<i64,WIDTH>=Block{data:[1;WIDTH]};let c:Block<i64,{1+2}>=b;if width(c)!=3{return 1;}return 0;}')
for name,source,expected in [
 ('shared-primary-fixture',root/'tests/regressions/const_generics.aeth',b''),
 ('numerical-vec',root/'tests/regressions/bootstrap/const_values.aeth',b'const-values=ok\n'),
 ('binary32-context',root/'tests/regressions/floating_context.aeth',b''),
 ('constant-aliases',root/'tests/regressions/constants_aliases.aeth',b'constants and aliases\n'),
 ('scientific-constants',root/'tests/regressions/bootstrap/scientific_constants.aeth',b''),
 ('imported-constant',module/'main.aeth',b'')]:
    run(name+'-check',[compiler,source,'-I',root/'stdlib','--check'])
    for fmt in a.formats:
        emitted=w/(name+('.ll' if fmt=='llvm' else '.c'))
        run(name+'-'+fmt+'-emit',[compiler,source,'-I',root/'stdlib',*(['--emit-llvm'] if fmt=='llvm' else []),'-o',emitted])
        for opt in ['0','2']:
            binary=w/(name+'-'+fmt+opt)
            run(name+'-'+fmt+opt+'-build',[cc,'-O'+opt,*(['-fsanitize=address,undefined','-fno-sanitize-recover=all'] if a.sanitize else []),emitted,'-o',binary])
            run(name+'-'+fmt+opt+'-run',[binary],stdout=expected)

invalid={
 'dead-operand':('const X:bool=false&&7;','bool'),
 'dead-name':('const X:bool=true||absent;','unknown'),
 'float-nonfinite':('const X:f64=0.0/0.0;','non-finite'),
 'float32-overflow':('const X:f32=2.0e38*2.0;','non-finite'),
 'logical-operand':('const X:bool=true&&1;','logical constant operands'),
 'add-overflow':('const X:u8=255+1;','overflows'),
 'subtract-overflow':('const X:u8=0-1;','overflows'),
 'multiply-overflow':('const X:i8=64*2;','overflows'),
 'signed-add-overflow':('const X:i64=9223372036854775807+1;','overflows'),
 'signed-subtract-overflow':('const X:i8=-128-1;','overflows'),
 'zero-divisor':('const X:i64=4/0;','division by zero'),
 'division-overflow':('const X:i64=(-9223372036854775808)/(-1);','overflows'),
 'remainder-overflow':('const X:i64=(-9223372036854775808)%(-1);','overflows'),
 'shift-width':('const X:i64=1<<64;','shift count'),
 'negative-shift':('const X:i64=1<<(-1);','shift count'),
 'shift-overflow':('const X:i8=1<<7;','overflows'),
 'negative-dimension':('struct A{data:[u8;-1]}','nonnegative'),
 'bool-dimension':('struct A{data:[u8;true]}','nonnegative'),
 'dimension-overflow':('struct A{data:[u64;9223372036854775807]}','address space'),
 'repeat-overflow':('fn f(){let x:[u8;2]=[0;9223372036854775807+1];}','overflows'),
 'unknown-dimension':('struct A{data:[u8;UNKNOWN]}','declared constant'),
 'constant-cycle':('const A:usize=B;const B:usize=A;','cyclic'),
 'constant-call':('fn f()->usize{2}const X:usize=f();','compile-time constant'),
 'constant-mismatch':('const A:u8=3;const B:i64=A;','constant type mismatch'),
 'const-param-type':('fn f<const N:f64>(){}','integer or bool'),
 'type-used-as-value':('struct B<const N:usize>{v:[u8;N]}fn f(){let b:B<u8>=B{v:[0;1]};}','const argument required'),
 'value-used-as-type':('struct B<T>{v:T}fn f(){let b:B<3>=B{v:0};}','type argument required'),
 'bool-param-integer':('struct B<const N:bool>{v:u8}fn f(){let b:B<1>=B{v:0};}','const argument type mismatch'),
 'cannot-infer-const':('fn f<const N:usize>()->usize{N}fn g()->usize{f()}','cannot infer'),
 'duplicate-const':('fn f<const N:usize,const N:usize>(){}','duplicate generic'),
 'conflicting-inference':('fn f<const N:usize>(a:[u8;N],b:[u8;N]){}fn g(){f([0;2],[0;3]);}','mismatch'),
 'dependent-dimension':('fn f<const N:usize>(a:[u8;N],b:[u8;N+1]){}fn g(){f([0;2],[0;2]);}','mismatch'),
 'const-shadow':('fn f<const N:usize>(a:[u8;N]){let N:i64=2;}fn g(){f([0;2]);}','shadows a const'),
 'const-assignment':('fn f<const N:usize>(a:[u8;N]){N=2;}fn g(){f([0;2]);}','assignable'),
 'alias-cycle':('type A<T>=A<T>;fn f(){let x:A<u8>=0;}','deep'),
 'value-const-type':('fn f<const N:usize>(x:N){}','not a value type'),
}
for name,(source_text,diagnostic) in invalid.items():
    source=w/(name+'.aeth');source.write_text(source_text)
    run(name+'-check',[compiler,source,'--check'],status=1,diagnostic=diagnostic)
    for fmt in a.formats:
        out=w/(name+'.'+fmt);out.write_bytes(b'existing user artifact\n')
        run(name+'-'+fmt+'-reject',[compiler,source,*(['--emit-llvm'] if fmt=='llvm' else []),'-o',out],status=1,diagnostic=diagnostic)
        record(name+'-'+fmt+'-preserved',out.read_bytes()==b'existing user artifact\n')
print(f'{len(records)} const generic checks passed')
