#!/usr/bin/env python3
"""Compare resolved module identities, explicit effect bounds and native CLI behavior."""
import argparse
import json
import os
from pathlib import Path
import subprocess

opts=argparse.ArgumentParser(description=__doc__)
opts.add_argument('--compiler',type=Path,required=True)
opts.add_argument('--reference-compiler',type=Path)
opts.add_argument('--work-dir',type=Path,required=True)
args=opts.parse_args()
work=args.work_dir.resolve()
work.mkdir(parents=True,exist_ok=True)
tool=args.compiler.resolve()
records=[]
def run(command,code=0,diagnostic=''):
    proc=subprocess.run(list(map(str,command)),capture_output=True,text=True,cwd=work,timeout=60)
    records.append(dict(command=proc.args,exit=proc.returncode,out=proc.stdout,err=proc.stderr))
    (work/'results.json').write_text(json.dumps(records,indent=2))
    assert proc.returncode==code and diagnostic in (proc.stdout+proc.stderr), records[-1]

def file(name,src):
    p=work/name
    p.write_text(src+'\n')
    return p

# A diamond must yield the original type identity. Private imports do not leak.
file('base.aeth','pub struct Counter{n:i32} pub fn bump(x:Counter)->i32 pure{x.n+1} fn hide(){}')
file('left.aeth','pub use base::*;')
file('right.aeth','pub use base::{Counter,bump};')
file('facade.aeth','pub use left::*;pub use right::*;pub use base::Counter as Number;')
source=file('run.aeth','use facade::{Number,Counter,bump};fn copy(v:Number)->Counter pure{v}fn main()->i32{bump(copy(Number{n:41}))-42}')
for compiler in [tool]+([args.reference_compiler.resolve()] if args.reference_compiler else []):
    for backend in ('c','llvm'):
        exe=work/(compiler.name+'-'+backend)
        run([compiler,source,'--backend',backend,'-O2','-o',exe])
        run([exe])
    cases=[
        ('local','struct S{x:i32}fn f(input:S)->i32 pure{let mut s:S=input;s.x=7;s.x}',True),
        ('global','static mut S:i32=1;fn f()->i32 pure{return S;}',False),
        ('write','struct S{x:i32}fn f(s:*S) pure{s.x=7;}',False),
        ('readwrite','struct S{x:i32}fn f(s:*S) effect[write]{s.x+=1;}',False),
        ('write-only','struct S{x:i32}fn f(s:*S) effect[write]{s.x=1;}',True),
        ('address','struct S{x:i32}fn f(s:&mut S)->&mut i32 pure{return &mut s.x;}',True),
        ('transitive','fn a(p:*i32){b(p);}fn b(p:*i32){if *p==0{a(p);}}fn f(p:*i32) pure{a(p);}',False),
        ('foreign','extern "C"{@pure fn external(n:i32)->i32;}fn f()->i32 pure{external(1)}',True),
        ('unannotated','extern "C"{fn external(n:i32)->i32;}fn f()->i32 pure{external(1)}',False),
        ('duplicate','extern "C"{@pure fn external();}extern "C"{@writes fn external();}fn f() pure{external();}',False),
        ('known','extern "C"{@pure fn puts(s:*u8)->i32;}fn f() pure{puts("x");}',False),
        ('empty','static mut X:i32=1;fn f() effect[]{X=2;}',False),
        ('separator','fn f() effect[read write]{}',False),
        ('private','use facade;fn f(){facade::hide();}',False),
    ]
    for name,text,ok in cases:
        fixture=file(name+'.aeth',text+'fn main(){}')
        run([compiler,'check',fixture],0 if ok else 1)
# Conflicting re-exports must identify the original use site.
file('other.aeth','pub fn bump()->i32{7}')
file('clash.aeth','pub use left::*;pub use other::*;')
run([tool,'check',file('ambiguous.aeth','use clash::bump;fn main(){bump();}')],1,'ambiguous')
# Standard invocation selects an executable and derives its path including dots.
entry=file('default.name.aeth','fn main()->i32{0}')
run([tool, entry])
run([work/'default.name'])
for flag,extension in [('--emit-c','c'),('--emit-obj','o'),('--emit-llvm','ll')]:
    run([tool,entry,flag])
    assert entry.with_suffix('.'+extension).stat().st_size>0
run([tool, '--version'],0,'self-hosted')
run([tool, '--help'],0,'--emit-obj')
print(f'{len(records)} shared effect, module and CLI checks passed')
