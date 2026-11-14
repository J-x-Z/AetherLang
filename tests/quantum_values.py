#!/usr/bin/env python3
"""Aether-owned quantum simulation and variational optimization on both backends."""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--backends',nargs='+',default=['c','llvm']);a=p.parse_args()
w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True);c=a.compiler.resolve();records=[]
def run(name,cmd):
 r=subprocess.run(list(map(str,cmd)),text=True,capture_output=True,timeout=60)
 records.append(dict(name=name,command=list(map(str,cmd)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr));(w/'results.json').write_text(json.dumps(records,indent=2))
 assert r.returncode==0,str(records[-1]);return r
source=Path(__file__).resolve().parent/'regressions/quantum/reference.aeth'
run('semantic check',[c,'check',source])
for backend in a.backends:
 for level in [0,2]:
  binary=w/f'reference-{backend}-{level}';run('build quantum',[c,source,'--backend',backend,f'-O{level}','-o',binary]);run('run quantum',[binary])
source_c=w/'reference.c';binary=w/'reference-sanitized';run('emit C',[c,source,'--emit-c','-o',source_c]);run('sanitizer build',[os.environ.get('CC','clang'),'-std=c11','-O1','-g','-fsanitize=address,undefined',source_c,'-lm','-o',binary]);run('sanitizer run',[binary])
print(f'{len(records)} quantum checks passed; {w / "results.json"}')
