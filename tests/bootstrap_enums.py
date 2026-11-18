#!/usr/bin/env python3
"""Tagged generic enum values, guarded matching, and Result propagation in Core."""
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
compiler=a.compiler.resolve();cc=shutil.which(a.cc);records=[]
env={**os.environ,'TMPDIR':str(w)}
if not cc:raise SystemExit('native C compiler unavailable')

def record(name,passed,**facts):
    records.append(dict(name=name,passed=bool(passed),**facts))
    (w/'results.json').write_text(json.dumps(records,indent=2)+'\n')
    print(('PASS ' if passed else 'FAIL ')+name,flush=True)
    if not passed:raise SystemExit(name)

def run(name,cmd,code=0,stdout=None,diagnostic=None):
    start=time.monotonic();cmd=list(map(str,cmd))
    try:r=subprocess.run(cmd,cwd=w,env=env,capture_output=True,timeout=45)
    except subprocess.TimeoutExpired:record(name,False,command=cmd,error='timeout')
    text=(r.stdout+r.stderr).decode(errors='replace')
    record(name,r.returncode==code and (stdout is None or r.stdout==stdout) and (diagnostic is None or diagnostic in text),
           command=cmd,exit=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'),elapsed_seconds=time.monotonic()-start)

(w/'compiler.json').write_text(json.dumps(dict(path=str(compiler),sha256=hashlib.sha256(compiler.read_bytes()).hexdigest()),indent=2))
(w/'data.bin').write_bytes(b'A\x00\xff');(w/'empty.bin').write_bytes(b'')
local=w/'module';local.mkdir(exist_ok=True)
(local/'values.aeth').write_text('const N:i32=9; enum State{Ready(i32),Empty} pub fn state()->State{State::Ready(N)}')
(local/'main.aeth').write_text('use values::{N,State,state}; fn main()->i32{let n:i32=match state(){State::Ready(x)=>x,State::Empty=>0};if n!=N{return 1;}return 0;}')
cases=[('ranges',root/'tests/regressions/bootstrap/range_values.aeth',b'range-values=ok\n',[]),
       ('values',root/'tests/regressions/bootstrap/enum_values.aeth',b'enum-values=ok\n',[]),
       ('file',root/'tests/regressions/bootstrap/enum_file.aeth',b'enum-file=ok\n',[w/'data.bin',w/'empty.bin',w/'missing.bin']),
       ('implicit-public',local/'main.aeth',b'',[])]
for name,source,expected,arguments in cases:
    run(name+'-check',[compiler,source,'-I',root/'stdlib','--check'])
    for fmt in a.formats:
        emitted=w/(name+('.ll' if fmt=='llvm' else '.c'))
        run(name+'-'+fmt+'-emit',[compiler,source,'-I',root/'stdlib',*(['--emit-llvm'] if fmt=='llvm' else []),'-o',emitted])
        for opt in ['0','2']:
            binary=w/(name+'-'+fmt+opt)
            run(name+'-'+fmt+opt+'-build',[cc,'-O'+opt,*(['-fsanitize=address,undefined','-fno-sanitize-recover=all'] if a.sanitize else []),emitted,'-o',binary])
            run(name+'-'+fmt+opt+'-run',[binary,*arguments],stdout=expected)

prefix='enum E{A(i32),B} '
invalid={
 'range-float':('fn main(){for i in 0.0..1.0{}}','matching integer'),
 'range-mismatched':('fn main(){let x:u8=2;let y:i64=4;for i in x..y{}}','matching integer'),
 'range-immutable':('fn main(){for i in 0..3{i=4;}}','immutable'),
 'range-scope':('fn f()->i64{for i in 0..3{}return i;}','unknown value'),
 'missing-arm':(prefix+'fn f(v:E)->i32{match v{E::A(n)=>n}}','non-exhaustive'),
 'guard-is-not-coverage':(prefix+'fn f(v:E)->i32{match v{E::A(n) if n>0=>n,E::B=>0}}','non-exhaustive'),
 'wrong-pattern-enum':(prefix+'enum F{A(i32),B} fn f(v:E)->i32{match v{F::A(n)=>n,_=>0}}','different enum'),
 'pattern-count':(prefix+'fn f(v:E)->i32{match v{E::A=>0,E::B=>1}}','payload count'),
 'payload-count':(prefix+'fn f()->E{E::A(1,2)}','argument count'),
 'payload-type':(prefix+'fn f()->E{E::A(true)}','type mismatch'),
 'unknown-variant':(prefix+'fn f()->E{E::Missing}','unknown enum variant'),
 'duplicate-variant':('enum E{A,A}','duplicate enum variant'),
 'recursive-layout':('enum E{A(E),B}','recursive struct'),
 'unknown-payload':('enum E{A(Missing),B}','unknown struct type'),
 'duplicate-arm':(prefix+'fn f(v:E)->i32{match v{E::A(n)=>n,E::A(_)=>0,E::B=>0}}','duplicate enum pattern'),
 'wildcard-shadow':(prefix+'fn f(v:E)->i32{match v{_=>0,E::B=>1}}','after wildcard'),
 'guard-type':(prefix+'fn f(v:E)->i32{match v{E::A(n) if n=>n,_=>0}}','type mismatch'),
 'arm-type':(prefix+'fn f(v:E)->i32{match v{E::A(n)=>n,E::B=>false}}','type mismatch'),
 'binding-scope':(prefix+'fn f(v:E)->i32{match v{E::A(n)=>{},E::B=>{}};return n;}','unknown value'),
 'immutable-binding':(prefix+'fn f(v:E)->i32{match v{E::A(n)=>{n=2;n},E::B=>0}}','immutable'),
 'unknown-generic':('enum E<T>{A(T),B} fn f(){E::B;}','cannot infer'),
 'generic-body':('enum E<T>{A(T),B} fn f(v:E<u8>)->i64{match v{E::A(n)=>n,E::B=>0}}','type mismatch'),
 'try-target':('use result::Result;fn f(v:Result<i32,i32>)->i32{v?}','Result-returning'),
 'try-source':('use result::Result;fn f()->Result<i32,i32>{let x:i32=1?;Result::Ok(x)}','? requires'),
 'try-error':('use result::Result;fn f(v:Result<i32,u8>)->Result<i32,i32>{Result::Ok(v?)}','same error'),
 'foreign-enum':(prefix+'extern "C"{fn f(v:E);}','foreign enum value ABI'),
 'foreign-nested-enum':(prefix+'struct S{v:E} extern "C"{fn f(v:S);}','foreign enum value ABI'),
 'foreign-array-enum':(prefix+'struct S{v:[E;2]} extern "C"{fn f()->S;}','foreign enum value ABI'),
}
for name,(text,diagnostic) in invalid.items():
    source=w/(name+'.aeth');source.write_text(text)
    run(name+'-check',[compiler,source,'-I',root/'stdlib','--check'],code=1,diagnostic=diagnostic)
    for fmt in a.formats:
        target=w/(name+'.'+fmt);target.write_bytes(b'preserve existing output\n')
        run(name+'-'+fmt+'-reject',[compiler,source,'-I',root/'stdlib',*(['--emit-llvm'] if fmt=='llvm' else []),'-o',target],code=1,diagnostic=diagnostic)
        record(name+'-'+fmt+'-preserved',target.read_bytes()==b'preserve existing output\n')
print(f'{len(records)} enum checks passed')
