#!/usr/bin/env python3
"""Self-hosted closure values, shared diagnostics and native C/LLVM execution."""
import argparse,subprocess,json,hashlib,os
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--compiler',type=Path,required=True);p.add_argument('--seed',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--fixtures',type=Path,default=Path(__file__).resolve().parent/'regressions/bootstrap/closures');a=p.parse_args();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True);records=[]
def run(cmd,status=0):
 cmd=list(map(str,cmd));result=subprocess.run(cmd,capture_output=True,text=True,timeout=60,env={**os.environ,'TMPDIR':str(work)});records.append(dict(command=cmd,status=result.returncode,expected=status,stdout=result.stdout,stderr=result.stderr));(work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(a.compiler.read_bytes()).hexdigest(),seed_sha256=hashlib.sha256(a.seed.read_bytes()).hexdigest(),checks=records),indent=2));assert result.returncode==status,records[-1];return result
(work/'local.aeth').write_text('struct Counter{value:i64}impl Counter{fn read(self:&Self)->i64{return self.value;}}pub fn result()->i64{let value:Counter=Counter{value:31};let read:_=||value.read();return read();}')
(work/'module.aeth').write_text('mod local;fn main()->i32{if local::result()!=31{return 1;}return 0;}')
sources=[a.fixtures/'basic.aeth',a.fixtures/'composition.aeth',a.fixtures/'scopes.aeth',a.fixtures/'script.ath',work/'module.aeth']
for src in sources:
 name=src.stem;run([a.seed,'check',src]);run([a.compiler,'check',src])
 for backend in ['c','llvm']:
  generated=work/f'{name}.{ "c" if backend=="c" else "ll"}';run([a.compiler,src,'--emit-'+backend,'-o',generated])
  for level in [0,2]:
   binary=work/f'{name}-{backend}-{level}';run(['clang',f'-O{level}',generated,'-lm','-o',binary]);result=run([binary]);assert result.stdout==('script closures ok\n' if src.suffix=='.ath' else '') and result.stderr==''
  binary=work/f'{name}-{backend}-sanitized';run(['clang','-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all',generated,'-lm','-o',binary]);result=run([binary]);assert result.stdout==('script closures ok\n' if src.suffix=='.ath' else '') and result.stderr==''
negative={
'environment_loan':'fn f(){let x:i64=3;let mut c:_=||->&i64{x+=1;&x};let r:&i64=c();c();let n:i64=*r;}',
'wrong_argument':'fn f(){let c:_=|x:i64|x;c(true);}',
'arity':'fn f(){let c:_=|x:i64|x;c();}',
'return_type':'fn f(){let c:_=|x:i64|->bool{x};}',
'undefined':'fn f(){let c:_=|x:i64|missing+x;}',
'immutable':'fn f(){let x:i64=7;let c:_=||{x+=1;x};c();}',
'mut_shared':'fn apply<F>(f:&F)->i64{return f();}fn f(){let x:i64=7;let c:_=||{x+=1;x};apply(&c);}',
'ordinary_inference':'fn f(){let x:_=7;}',
'concrete_reassignment':'fn f(){let mut x:_=|a:i64|a+1;x=|a:i64|a+2;}',
'owner_capture':'struct R{fd:i32}fn f(){unsafe{let r:own R=R{fd:3} as own R;let c:_=||r.fd;}}',
'return_local':'fn f()->&i64{let c:_=||->&i64{let x:i64=3;&x};return c();}',
'environment_escape':'fn f()->&i64{let x:i64=3;let c:_=||->&i64{&x};return c();}',
'parameter_escape':'fn f()->&i64{let c:_=|x:i64|->&i64{&x};return c(3);}',
'duplicate_parameter':'fn f(){let c:_=|x:i64,x:i64|x;}',
'false_branch_return':'fn f(){let c:_=|x:bool|->i64{if x{return 1;}};}',
}
for name,code in negative.items():
 path=work/f'{name}.aeth';path.write_text(code+'fn main()->i32{return 0;}');run([a.seed,'check',path],1);run([a.compiler,'check',path],1);run([a.compiler,path,'-o',work/'rejected.c'],1)
for name,code in {
'exclusive_reference_capture':'fn f(){let mut x:i64=3;let p:&mut i64=&mut x;let c:_=||*p;}',
}.items():
 path=work/f'{name}.aeth';path.write_text(code+'fn main()->i32{return 0;}');run([a.compiler,'check',path],1)
for name,code in {
 'unassigned':'def main():\n    if False:\n        x = 3\n    f = lambda (): x\n    return 0\n',
 'unknown':'def main():\n    f = lambda (): missing\n    return 0\n',
 'arity':'def main():\n    f = lambda (x: int): x\n    return f()\n',
 'argument':'def main():\n    f = lambda (x: int): x\n    return f(True)\n',
 'return_annotation':'def main():\n    f = lambda (x: int) -> bool: x\n    return 0\n',
 'reassignment':'def main():\n    f = lambda (x: int): x + 1\n    f = lambda (x: int): x + 2\n    return 0\n',
}.items():
 path=work/f'script_{name}.ath';path.write_text(code);run([a.seed,'check',path],1);run([a.compiler,'check',path],1);run([a.compiler,path,'-o',work/'rejected.c'],1)
print(f'{len(records)} closure checks passed')
