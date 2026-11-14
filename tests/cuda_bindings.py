#!/usr/bin/env python3
"""CUDA ownership/error tests with a fault shim; these are not GPU validation.
For actual GPU validation compile regressions/cuda/gpu_matrix.aeth and link
-lcublas -lcudart on a CUDA host. Exit 77 means cudaErrorNoDevice/count zero.
"""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['llvm','c'],default=['llvm','c']);a=p.parse_args()
root=Path(__file__).resolve().parents[1];work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
env={**os.environ,'TMPDIR':str(work)};source=root/'tests/regressions/cuda';records=[]
def run(name,command):
 r=subprocess.run(list(map(str,command)),env=env,capture_output=True,text=True,timeout=60)
 records.append(dict(name=name,command=list(map(str,command)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr,passed=r.returncode==0))
 (work/'results.json').write_text(json.dumps(records,indent=2)+'\n');print(f'{"PASS" if r.returncode==0 else "FAIL"} {name}',flush=True)
 if r.returncode:raise SystemExit(r.stderr or r.stdout or f'exit {r.returncode}')
run('fault-shim-build',[os.environ.get('CC','clang'),'-c',source/'faults.c','-o',work/'faults.o'])
for backend in a.backends:
 for level in ['0','2']:
  binary=work/f'ownership-{backend}-O{level}'
  run(f'build-{backend}-O{level}',[a.compiler.resolve(),'build',source/'ownership.aeth','--backend',backend,f'-O{level}','--link-object',work/'faults.o','-o',binary])
  run(f'ownership-{backend}-O{level}',[binary])
