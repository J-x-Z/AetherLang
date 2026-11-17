#!/usr/bin/env python3
"""Single-evaluation fixed arrays, const counts, borrow safety, and bounded IR."""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
a=p.parse_args();compiler=a.compiler.resolve();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
root=Path(__file__).resolve().parents[1];records=[]
def run(command,expected=0,stdout=None,timeout=30):
 command=list(map(str,command));r=subprocess.run(command,text=True,capture_output=True,timeout=timeout)
 records.append(dict(command=command,status=r.returncode,expected=expected,stdout=r.stdout,stderr=r.stderr))
 (work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),checks=records),indent=2))
 assert r.returncode==expected,(command,r.stdout,r.stderr)
 if stdout is not None:assert r.stdout==stdout and not r.stderr,(command,r.stdout,r.stderr)
 return r
for source,output in [('array_repeat.aeth',''),('script/array_repeat.ath','once\nonce\n')]:
 for backend in a.backends:
  for level in [0,2,3]:
   binary=work/f'{Path(source).suffix[1:]}-{backend}-{level}'
   run([compiler,root/'tests/regressions'/source,'--backend',backend,'-O',level,'-o',binary])
   run([binary],stdout=output,timeout=10)
negative={
 'exclusive':'fn main(){let mut x:i64=1;let copies:[&mut i64;2]=[&mut x;2];}',
 'nested_exclusive':'struct R{value:&mut i64}fn main(){let mut x:i64=1;let copies:[R;2]=[R{value:&mut x};2];}',
 'enum_exclusive':'enum R{Value(&mut i64),Empty}fn main(){let mut x:i64=1;let copies:[R;2]=[R::Value(&mut x);2];}',
 'escape':'fn leak()->[&i64;3]{let x:i64=1;[&x;3]}fn main(){}',
 'negative':'fn main(){let values:[i64;0]=[1;-1];}',
 'runtime':'fn f(n:usize){let values:[i64;2]=[1;n];}fn main(){}',
 'shape':'fn main(){let values:[i64;2]=[1;3];}',
 'owner':'struct H{fd:i32}fn main(){let values:[own H;2]=[unsafe{H{fd:3} as own H};2];}',
}
for name,source in negative.items():
 path=work/f'{name}.aeth';path.write_text(source)
 checked=run([compiler,'check','--error-format','json',path],1)
 built=run([compiler,'build','--error-format','json',path,'--backend','c','-o',work/'rejected'],1)
 assert json.loads(checked.stderr)==json.loads(built.stderr),(name,checked.stderr,built.stderr)
# These only check source semantics, never allocate the represented array.
large=work/'large.aeth';large.write_text('#![no_main]\nstatic zeroes:[u8;1000000000]=[0;1000000000];fn inspect(value:[u8;1000000000])->u8{value[0]}fn make()->[u8;1000000000]{[7;1000000000]}')
run([compiler,'check',large],timeout=15)
empty=work/'empty_reference.aeth';empty.write_text('fn empty()->[&i64;0]{let value:i64=7;[&value;0]}fn main(){let values:[&i64;0]=empty();}')
run([compiler,'check',empty])
compact=work/'compact.aeth';compact.write_text('#![no_main]\nfn make()->[u8;65536]{[7;65536]}')
for backend in a.backends:
 ir=work/f'compact-{backend}.ir'
 run([compiler,compact,'--backend',backend,'--emit-ir','-O0','-o',ir])
 assert ir.stat().st_size<20000,('repeat expanded into linear IR',ir.stat().st_size)
print(f'{len(records)} array repeat commands passed')
