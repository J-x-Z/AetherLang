#!/usr/bin/env python3
"""Aether-written cooperative state machines, cancellation and shared diagnostics."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--seed',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);a=p.parse_args();a.compiler=a.compiler.resolve();a.seed=a.seed.resolve()
repo=Path(__file__).resolve().parents[1];fixtures=repo/'tests/regressions/async';work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True);records=[]
def run(command,status=0,stdout=None):
 command=list(map(str,command));r=subprocess.run(command,capture_output=True,text=True,timeout=60,env={**os.environ,'TMPDIR':str(work),'ASAN_OPTIONS':'detect_stack_use_after_return=1','UBSAN_OPTIONS':'halt_on_error=1'})
 records.append(dict(command=command,status=r.returncode,expected=status,stdout=r.stdout,stderr=r.stderr));(work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(a.compiler.read_bytes()).hexdigest(),seed_sha256=hashlib.sha256(a.seed.read_bytes()).hexdigest(),checks=records),indent=2));assert r.returncode==status,records[-1]
 if stdout is not None:assert r.stdout==stdout and not r.stderr,records[-1]
 return r
for name in ['basic.aeth','combined.aeth','resources.aeth','interleave.aeth','modules.aeth','basic.ath','modules.ath','compositions.ath','aliases.ath']:
 source=fixtures/name;run([a.seed,'check',source,'-I',repo/'stdlib']);run([a.compiler,'check',source,'-I',repo/'stdlib'])
 for backend in ['c','llvm']:
  emitted=work/(name+('.c' if backend=='c' else '.ll'));run([a.compiler,source,'-I',repo/'stdlib','--emit-'+backend,'-o',emitted])
  for opt in ['0','2','san']:
   binary=work/f'{name}-{backend}-{opt}';flags=['-O'+opt] if opt!='san' else ['-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all'];run(['clang',*flags,emitted,'-lm','-o',binary]);run([binary],stdout={'basic.ath':'16\n','modules.ath':'29\n'}.get(name,''))
negative={
 'outside.aeth':'fn main(){await yield_now();}',
 'native_export.aeth':'#[no_mangle] async fn exported()->i32{return 7;}fn main(){}',
 'generic.aeth':'async fn f<T>(x:T)->T{await yield_now();return x;}fn main(){}',
 'recursive.aeth':'async fn f()->i32{let x:i32=await f();return x;}fn main(){}',
 'nested.aeth':'async fn f(){if true{await yield_now();}}fn main(){}',
 'nested_argument.aeth':'async fn f(x:i32)->i32{return x;}async fn g(){let x:i32=await f(await f(1));}fn main(){}',
 'not_future.aeth':'fn f()->i32{1}async fn g()->i32{let x:i32=await f();return x;}fn main(){}',
 'uninitialized.aeth':'async fn f(){let x:i32;await yield_now();}fn main(){}',
 'reference.aeth':'async fn f(x:&i32)->i32{await yield_now();return *x;}fn main(){}',
 'reference_local.aeth':'async fn f()->i32{let x:i32=2;let r:&i32=&x;await yield_now();return *r;}fn main(){}',
 'unsafe_payload.aeth':'struct H{p:*u8}async fn f(h:own H)->*u8{let p:*u8=h.p;await yield_now();return p;}fn main(){}',
 'moved_task.aeth':'async fn f(){await yield_now();}fn main(){let mut x:_=f();x.cancel();x.poll();}',
 'immutable.aeth':'async fn f(){await yield_now();}fn main(){let x:_=f();x.poll();}',
 'hidden_helper.aeth':'async fn f(){await yield_now();}fn main(){__aeth_async_frame_0_drop(1);}',
 'dispatcher_break.aeth':'async fn f(){break;await yield_now();}fn main(){}',
 'missing_return.aeth':'async fn f()->i32{await yield_now();}fn main(){}',
 'wrong_await.aeth':'async fn f()->i32{return 1;}async fn g()->bool{let x:bool=await f();return x;}fn main(){}',
 'imported_unawaited.ath':'from script_child import delayed\ndef main():\n    x = delayed(1)\n    return 0\n',
 'outside.ath':'def main():\n    await yield_now()\n',
 'not_future.ath':'def f() -> int:\n    return 1\nasync def g() -> int:\n    return await f()\ndef main():\n    return 0\n',
 'unawaited.ath':'async def f() -> int:\n    return 1\ndef main():\n    x = f()\n    return 0\n',
 'closure_unawaited.ath':'async def f() -> int:\n    return 1\ndef main():\n    g = lambda (): f()\n    return 0\n',
 'discard_unawaited.ath':'async def f() -> int:\n    return 1\ndef main():\n    f()\n    return 0\n',
}
for name,text in negative.items():
 source=work/name;source.write_text(text)
 run([a.seed,'check',source,'-I',fixtures],1);run([a.compiler,'check',source,'-I',fixtures],1)
 for backend in ['c','llvm']:
  output=work/f'rejected.{backend}';output.write_text('existing user artifact\n');run([a.compiler,source,'-I',fixtures,'--emit-'+backend,'-o',output],1);assert output.read_text()=='existing user artifact\n'
print(f'{len(records)} self-hosted async checks passed')
