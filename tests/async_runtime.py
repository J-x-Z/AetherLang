#!/usr/bin/env python3
"""Concrete async frames: native suspension, cancellation, and Script/Core parity."""
import argparse
import json
import os
from pathlib import Path
import subprocess
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--no-sanitize',action='store_true')
a=p.parse_args();repo=Path(__file__).resolve().parent.parent;root=repo/'tests/regressions/async';work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
env={**os.environ,'TMPDIR':str(work),'ASAN_OPTIONS':'detect_stack_use_after_return=1','UBSAN_OPTIONS':'halt_on_error=1'};compiler=a.compiler.resolve();records=[]
def run(label,cmd,expected=0,diagnostic=None):
 r=subprocess.run(list(map(str,cmd)),env=env,text=True,capture_output=True,timeout=45)
 records.append(dict(label=label,command=list(map(str,cmd)),status=r.returncode,stdout=r.stdout,stderr=r.stderr));(work/'results.json').write_text(json.dumps(records,indent=2))
 assert r.returncode==expected,f'{label}: {r.returncode}\n{r.stdout}\n{r.stderr}'
 if diagnostic: assert diagnostic in r.stderr,f'{label}: {r.stderr}'
 return r
cases=['basic.aeth','combined.aeth','resources.aeth','interleave.aeth','modules.aeth','basic.ath','modules.ath','compositions.ath','aliases.ath']
for case in cases:
 run(f'{case}/check',[compiler,'check',root/case,'--sysroot',repo])
 for backend in a.backends:
  for level in [0,2,3]:
   output=work/f'{case}-{backend}-O{level}'
   run(f'{case}/{backend}/O{level}/build',[compiler,root/case,'--sysroot',repo,'--backend',backend,f'-O{level}','-o',output])
   r=run(f'{case}/{backend}/O{level}/run',[output]);assert r.stdout==({'basic.ath':'16\n','modules.ath':'29\n'}.get(case,'')) and not r.stderr
if not a.no_sanitize:
 for case in cases:
  source=work/f'{case}.c';output=work/f'{case}-sanitized'
  run(f'{case}/san/emit',[compiler,root/case,'--sysroot',repo,'--emit-c','-o',source])
  run(f'{case}/san/build',[os.environ.get('CC','clang'),'-std=c11','-O1','-g','-fsanitize=address,undefined','-fno-omit-frame-pointer',source,'-o',output])
  run(f'{case}/san/run',[output])
negatives={
 'outside.aeth':('fn main(){await yield_now();}','E2406'),
 'native_export.aeth':('#[no_mangle] async fn exported()->i32{return 7;}fn main(){}','E2406'),
 'generic.aeth':('async fn f<T>(x:T)->T{await yield_now();return x;}fn main(){}','E2406'),
 'recursive.aeth':('async fn f()->i32{let x:i32=await f();return x;}fn main(){}','E2406'),
 'nested.aeth':('async fn f(){if true{await yield_now();}}fn main(){}','E2406'),
 'nested_argument.aeth':('async fn f(x:i32)->i32{return x;}async fn g(){let x:i32=await f(await f(1));}fn main(){}','E2406'),
 'not_future.aeth':('fn f()->i32{1}async fn g()->i32{let x:i32=await f();return x;}fn main(){}','E2406'),
 'uninitialized.aeth':('async fn f(){let x:i32;await yield_now();}fn main(){}','E2406'),
 'reference.aeth':('async fn f(x:&i32)->i32{await yield_now();return *x;}fn main(){}','E2405'),
 'unsafe_payload.aeth':('struct H{p:*u8}async fn f(h:own H)->*u8{let p:*u8=h.p;await yield_now();return p;}fn main(){}','E2405'),
 'moved_task.aeth':('async fn f(){await yield_now();}fn main(){let mut x:_=f();x.cancel();x.poll();}','E2301'),
 'immutable.aeth':('async fn f(){await yield_now();}fn main(){let x:_=f();x.poll();}',None),
 'hidden_helper.aeth':('async fn f(){await yield_now();}fn main(){__aeth_async_frame_0_drop(1);}','E2001'),
 'dispatcher_break.aeth':('async fn f(){break;await yield_now();}fn main(){}','E2406'),
 'imported_unawaited.ath':('from script_child import delayed\ndef main():\n    x = delayed(1)\n    return 0\n','E1101'),
 'outside.ath':('def main():\n    await yield_now()\n','E1101'),
 'not_future.ath':('def f() -> int:\n    return 1\nasync def g() -> int:\n    return await f()\ndef main():\n    return 0\n','E1101'),
 'unawaited.ath':('async def f() -> int:\n    return 1\ndef main():\n    x = f()\n    return 0\n','E1101'),
}
for name,(source,diagnostic) in negatives.items():
 path=work/name;path.write_text(source)
 run(f'{name}/reject',[compiler,'check',path,'--sysroot',repo,'-I',root,'--error-format','json'],1,diagnostic)
print(f'{len(records)} async checks passed; evidence: {work/"results.json"}')
