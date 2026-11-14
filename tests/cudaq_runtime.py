#!/usr/bin/env python3
"""Optional real CUDA-Q SDK validation; never replace it with a mock result.
Build the native adapter first with scripts/build_cudaq.py. This runner compares
Aether's own simulator with the SDK, and runs an Aether-owned optimizer loop.
"""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--bridge',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--backends',nargs='+',default=['c','llvm']);p.add_argument('--gpu',action='store_true');a=p.parse_args()
w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True);c=a.compiler.resolve();bridge=a.bridge.resolve();source=Path(__file__).resolve().parent/'regressions/quantum/cudaq.aeth';records=[]
def run(name,cmd):
 r=subprocess.run(list(map(str,cmd)),text=True,capture_output=True,timeout=120)
 records.append(dict(name=name,command=list(map(str,cmd)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr));(w/'results.json').write_text(json.dumps(records,indent=2))
 assert r.returncode==0,str(records[-1]);return r
for fixture in ['cudaq', 'cudaq_budget']:
 source=source.with_name(fixture+'.aeth')
 for backend in a.backends:
  for level in [0,2]:
   binary=w/f'{fixture}-{backend}-{level}';run('build SDK '+fixture,[c,source,'--backend',backend,f'-O{level}','--link-object',bridge,'-o',binary])
   r=run('run SDK '+fixture,[binary]+(['gpu'] if a.gpu else []));assert f'on_gpu={int(a.gpu)}' in r.stdout
print(f'{len(records)} native SDK checks passed')
