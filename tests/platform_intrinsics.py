#!/usr/bin/env python3
"""Target-libc errno access through canonical typed compiler operations."""
import argparse, json, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True); p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',default=['c','llvm']); a=p.parse_args()
w=a.work_dir.resolve(); w.mkdir(parents=True,exist_ok=True); c=a.compiler.resolve(); records=[]
def run(name,cmd,fail=False):
 r=subprocess.run(list(map(str,cmd)),text=True,capture_output=True,timeout=30)
 records.append(dict(name=name,command=list(map(str,cmd)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr))
 (w/'results.json').write_text(json.dumps(records,indent=2))
 assert (r.returncode!=0 if fail else r.returncode==0),str(records[-1])
 return r
source=Path(__file__).resolve().parent/'regressions/platform/errno.aeth'
for backend in a.backends:
 for level in [0,2]:
  binary=w/f'errno-{backend}-{level}'
  run('build errno',[c,source,'--backend',backend,f'-O{level}','-o',binary]);run('run errno',[binary])
for name,source_text in {
 'arity':'fn main(){__errno_get(1);}',
 'pure':'fn value()->i32 pure {__errno_get()} fn main(){}',
 'redefine':'fn __errno_get()->i32{1} fn main(){}',
 'foreign':'extern "C"{fn __errno_get()->i32;} fn main(){}',
 'shadow':'fn main(){let __errno_get:i32=2;}',
 'wrong_type':'fn main(){let error:bool=__errno_get();}',
}.items():
 path=w/(name+'.aeth'); path.write_text(source_text);run('reject '+name,[c,'check',path],True)
if 'llvm' in a.backends:
 for target in ['x86_64-unknown-linux-gnu','aarch64-linux-android','x86_64-pc-windows-msvc']:
  run('cross object '+target,[c,source,'--backend','llvm','--emit-obj','--target',target,'-o',w/(target+'.o')])
print(f'{len(records)} platform checks passed')
