#!/usr/bin/env python3
"""Captured closure value, invocation, lifetime and effect semantics."""
import argparse, hashlib, json, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
a=p.parse_args();compiler=a.compiler.resolve();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
root=Path(__file__).resolve().parents[1];records=[]
def run(command,expected=0,stdout=None):
 command=list(map(str,command));r=subprocess.run(command,text=True,capture_output=True,timeout=30)
 records.append(dict(command=command,status=r.returncode,expected=expected,stdout=r.stdout,stderr=r.stderr))
 (work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),checks=records),indent=2))
 assert r.returncode==expected,(command,r.stdout,r.stderr)
 if stdout is not None:assert r.stdout==stdout and not r.stderr,(command,r.stdout,r.stderr)
 return r
sources=[root/'tests/regressions/closures.aeth',root/'tests/regressions/script/closures.ath']
# The closure's generated body must retain the module's access to its private methods.
(work/'local.aeth').write_text('struct Value{v:i64}impl Value{fn get(self:&Self)->i64{self.v}fn through(self:&Self)->i64{let f:_=||->i64{self.get()};f()}}pub fn result()->i64{let x:Value=Value{v:31};let f:_=||->i64{x.through()};f()}')
module=work/'module.aeth';module.write_text('mod local;fn main()->i32{if local::result()!=31{return 1;}return 0;}');sources.append(module)
for index,source in enumerate(sources):
 for backend in a.backends:
  for level in [0,2,3]:
   binary=work/f'{index}-{backend}-{level}'
   run([compiler,source,'--backend',backend,'-O',level,'-o',binary])
   run([binary],stdout='script closures ok\n' if source.suffix=='.ath' else '')
negative={
 'escape':'fn bad()->&i64{let x:i64=7;let r:&i64=&x;let f:_=||->&i64{r};f()}fn main(){}',
 'temporary_escape':'fn main(){let r:&i64={let f:_=||->&i64{let x:i64=7;&x};f()};}',
 'immutable':'fn main(){let n:i64=7;let f:_=||->i64{n=n+1;n};f();}',
 'wrong_argument':'fn main(){let f:_=|x:i64|x;f(true);}',
 'arity':'fn main(){let f:_=|x:i64|x;f();}',
 'return_type':'fn main(){let f:_=|x:i64|->bool{x};}',
 'undefined':'fn main(){let f:_=|x:i64|->i64{missing+x};}',
 'owner':'struct H{fd:i32}fn main(){let h:own H=unsafe{H{fd:3} as own H};let f:_=||->i32{h.fd};}',
 'ordinary_inference':'fn main(){let x:_=7;}',
 'exclusive_copy':'fn main(){let mut n:i64=7;let p:&mut i64=&mut n;let mut f:_=||->i64{*p=*p+1;*p};let mut g:_=f;f();g();}',
 'effect':'extern "C"{@io fn puts(s:*u8)->i32;}fn main() pure{let f:_=||->i32{puts("effect")};f();}',
 'function_pointer':'fn main(){let f:fn(i64)->i64=|x:i64|x;}',
 'script_capture_unassigned':'def main():\n    f = lambda (): missing + 1\n    return 0\n',
 'script_ignored_signature':'def ignores(f: Callable[int,int]) -> int:\n    return 0\ndef main():\n    return ignores(lambda (value: bool) -> bool: value)\n',
 'script_nested_signature':'def ignores(f: Callable[int,Callable[int,int]]) -> int:\n    return 0\ndef main():\n    return ignores(lambda (value: int): lambda (other: bool): other)\n',
 'script_return_signature':'def invalid(f: Callable[int,int]) -> Callable[int,int]:\n    return lambda (value: bool): value\ndef main():\n    return 0\n',
 'script_arity':'def main():\n    f = lambda (x: int): x + 1\n    return f()\n',
 'script_wrong_callback':'def use(f: Callable[int,int]) -> int:\n    return f(3)\ndef main():\n    f = lambda (x: bool): not x\n    return use(f)\n',
 'script_concrete_reassignment':'def main():\n    f = lambda (x: int): x + 1\n    f = lambda (x: int): x + 2\n    return f(0)\n',
 'script_branch_binding':'def main():\n    if True:\n        f = lambda (): 1\n    return f()\n',

}
for name,source in negative.items():
 path=work/f'{name}.{"ath" if name.startswith("script_") else "aeth"}';path.write_text(source)
 checked=run([compiler,'check','--error-format','json',path],1)
 built=run([compiler,'build','--error-format','json',path,'--backend','c','-o',work/'rejected'],1)
 assert json.loads(checked.stderr)==json.loads(built.stderr),(name,checked.stderr,built.stderr)
print(f'{len(records)} closure commands passed')
