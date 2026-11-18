#!/usr/bin/env python3
"""Checked-reference Core/Rust semantic agreement and native C/LLVM execution."""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True,help='Aether-written Core compiler')
p.add_argument('--seed',type=Path,required=True,help='Rust compiler used as a shared-semantics oracle')
p.add_argument('--work-dir',type=Path,required=True)
a=p.parse_args();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
core=a.compiler.resolve();seed=a.seed.resolve();root=Path(__file__).resolve().parents[1];records=[]
def run(command,status=0):
 r=subprocess.run(list(map(str,command)),capture_output=True,text=True,timeout=40,env={**os.environ,'TMPDIR':str(work)})
 records.append(dict(command=list(map(str,command)),status=r.returncode,expected=status,stdout=r.stdout,stderr=r.stderr))
 (work/'results.json').write_text(json.dumps(dict(core_sha256=hashlib.sha256(core.read_bytes()).hexdigest(),seed_sha256=hashlib.sha256(seed.read_bytes()).hexdigest(),checks=records),indent=2))
 assert r.returncode==status,(command,r.returncode,r.stdout,r.stderr)
 return r
positive=root/'tests/regressions/bootstrap/references.aeth'
run([seed,'check',positive]);run([core,positive,'--check'])
for backend in ['c','llvm']:
 generated=work/f'values.{"c" if backend=="c" else "ll"}'
 command=[core,positive,'-o',generated]
 if backend=='llvm':command+=['--emit-llvm']
 run(command)
 for level in [0,2]:
  binary=work/f'values-{backend}-{level}'
  run(['clang',f'-O{level}',generated,'-lm','-o',binary]);r=run([binary]);assert r.stdout=='' and r.stderr==''
negative={
 'return_local':'fn bad()->&i64{let x:i64=1;return &x;}',
 'return_parameter_storage':'fn bad(x:i64)->&i64{return &x;}',
 'return_through_call':'fn id(x:&i64)->&i64{return x;}fn bad()->&i64{let x:i64=1;return id(&x);}',
 'scope_escape':'fn f(x:&i64){let mut r:&i64=x;{let y:i64=2;r=&y;}let z:i64=*r;}',
 'write_during_loan':'fn f(){let mut x:i64=1;let r:&i64=&x;x=2;let z:i64=*r;}',
 'read_during_exclusive':'fn f(){let mut x:i64=1;let r:&mut i64=&mut x;let y:i64=x;*r=2;}',
 'two_exclusive':'fn f(){let mut x:i64=1;let a:&mut i64=&mut x;let b:&mut i64=&mut x;*a=2;*b=3;}',
 'shared_write':'fn f(x:&i64){*x=2;}',
 'shared_field_write':'struct S{x:i64}fn f(s:&S){s.x=2;}',
 'forge_pointer':'fn f(p:*i64)->&i64{return p as &i64;}',
 'forge_integer':'fn f()->&i64{return 0 as &i64;}',
 'implicit_raw':'fn f(){let x:i64=1;let p:*i64=&x;}',
 'wrong_raw_target':'fn f(x:&i64)->*i32{return x as *i32;}',
 'use_after_move':'fn f(){let mut x:i64=1;let a:&mut i64=&mut x;let b:&mut i64=a;*a=2;*b=3;}',
 'branch_move':'fn f(flag:bool){let mut x:i64=1;let r:&mut i64=&mut x;if flag{let moved:&mut i64=r;*moved=3;}*r=2;}',
 'loop_move':'fn f(flag:bool){let mut x:i64=1;let r:&mut i64=&mut x;while flag{let moved:&mut i64=r;*moved=3;}}',
 'argument_alias':'fn both(a:&mut i64,b:&i64){}fn f(){let mut x:i64=1;both(&mut x,&x);}',
 'reborrow_parent':'fn f(){let mut x:i64=1;let r:&mut i64=&mut x;let s:&mut i64=&mut *r;*r=3;*s=4;}',
}
for name,source in negative.items():
 path=work/f'{name}.aeth';path.write_text(source+'fn main()->i32{return 0;}')
 run([seed,'check',path],1);run([core,path,'--check'],1);run([core,path,'-o',work/'rejected.c'],1)
# Exclusive aggregate payloads and foreign references remain explicit boundaries.
boundaries={
 'exclusive_payload':'fn f(x:&mut i64){let a:[&mut i64;1]=[x];}',
 'foreign_reference':'extern "C"{fn bad(x:&i64);}',
 'unproven_raw_origin':'fn f(p:*i64)->&i64{return &*p;}',
}
for name,source in boundaries.items():
 path=work/f'{name}.aeth';path.write_text(source+'fn main()->i32{return 0;}');run([core,path,'--check'],1)
print(f'{len(records)} Core reference checks passed')
