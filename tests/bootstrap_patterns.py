#!/usr/bin/env python3
"""Self-hosted nested and scalar patterns with correlated exhaustiveness."""
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
p.add_argument('--reference-compiler',type=Path)
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
    return r

(w/'compiler.json').write_text(json.dumps(dict(path=str(compiler),sha256=hashlib.sha256(compiler.read_bytes()).hexdigest()),indent=2))
source=root/'tests/regressions/bootstrap/pattern_values.aeth'
run('values-check',[compiler,source,'-I',root/'stdlib','--check'])
if a.reference_compiler:run('values-reference-check',[a.reference_compiler.resolve(),'check',source,'-I',root/'stdlib'])
for fmt in a.formats:
    emitted=w/('values'+('.ll' if fmt=='llvm' else '.c'))
    run(fmt+'-emit',[compiler,source,'-I',root/'stdlib',*(['--emit-llvm'] if fmt=='llvm' else []),'-o',emitted])
    for opt in ['0','2']:
        binary=w/('values-'+fmt+opt)
        run(fmt+opt+'-build',[cc,'-O'+opt,*(['-fsanitize=address,undefined','-fno-sanitize-recover=all'] if a.sanitize else []),emitted,'-o',binary])
        run(fmt+opt+'-run',[binary],stdout=b'patterns=ok\n')

prefix='enum E{Bits(bool,bool),Empty}'
invalid={
 'pattern-depth':('enum E{A(*E),B}fn f(e:E)->i32{match e{'+('E::A('*280)+'_'+(')'*280)+'=>1,_=>0}}','pattern nesting'),
 'correlated-hole':(prefix+'fn f(e:E)->i32{match e{E::Bits(true,_)=>1,E::Bits(_,true)=>2,E::Empty=>0}}','non-exhaustive'),
 'guard-coverage':(prefix+'fn f(e:E)->i32{match e{E::Bits(_,_) if true=>1,E::Empty=>0}}','non-exhaustive'),
 'nested-hole':('enum Inner{A,B}enum Outer{Nested(Inner),Empty}fn f(e:Outer)->i32{match e{Outer::Nested(Inner::A)=>1,Outer::Empty=>0}}','non-exhaustive'),
 'integer-open':('fn f(n:u8)->i32{match n{0=>0,1=>1}}','non-exhaustive'),
 'bool-missing':('fn f(n:bool)->i32{match n{true=>1}}','non-exhaustive'),
 'guarded-default':('fn f(n:i32)->i32{match n{_ if true=>1}}','non-exhaustive'),
 'literal-type':('fn f(n:bool)->i32{match n{0=>1,_=>0}}','type mismatch'),
 'literal-overflow':('fn f(n:u8)->i32{match n{256=>1,_=>0}}','outside its type range'),
 'negative-unsigned':('fn f(n:u8)->i32{match n{-1=>1,_=>0}}','signed integer'),
 'nested-type':('enum Inner{A}enum Other{A}enum Outer{Nested(Inner)}fn f(e:Outer)->i32{match e{Outer::Nested(Other::A)=>1}}','different enum'),
 'duplicate-binding':('enum E{Pair(i32,i32)}fn f(e:E)->i32{match e{E::Pair(x,x)=>x}}','duplicate binding'),
 'nested-payload-count':('enum Inner{A(i32)}enum Outer{Nested(Inner)}fn f(e:Outer)->i32{match e{Outer::Nested(Inner::A)=>1}}','payload count'),
 'unknown-nested-variant':('enum Inner{A}enum Outer{Nested(Inner)}fn f(e:Outer)->i32{match e{Outer::Nested(Inner::Missing)=>1}}','unknown enum variant'),
 'guard-type':('fn f(n:i32)->i32{match n{v if v=>1,_=>0}}','type mismatch'),
 'binding-scope':('fn f(n:i32)->i32{match n{v=>v};v}','unknown value'),
 'binding-immutable':('fn f(n:i32)->i32{match n{v=>{v=1;v}}}','immutable'),
}

for name,(text,diagnostic) in invalid.items():
    source=w/(name+'.aeth');source.write_text(text)
    run(name+'-check',[compiler,source,'--check'],status=1,diagnostic=diagnostic)
    if a.reference_compiler:run(name+'-reference-check',[a.reference_compiler.resolve(),'check',source],status=1)
    for fmt in a.formats:
        out=w/(name+'.'+fmt);out.write_bytes(b'existing user artifact\n')
        run(name+'-'+fmt+'-reject',[compiler,source,*(['--emit-llvm'] if fmt=='llvm' else []),'-o',out],status=1,diagnostic=diagnostic)
        record(name+'-'+fmt+'-preserved',out.read_bytes()==b'existing user artifact\n')
print(f'{len(records)} pattern checks passed')
