#!/usr/bin/env python3
"""Build the native CUDA-Q C ABI adapter, reusing an existing SDK or wheel runtime.
Normal C++ SDK: --sdk <prefix>. A wheel also needs the matching SDK source
headers (--source) and any external header roots (--include, e.g. nlohmann/fmt).
No package installation, shell-profile changes or implicit downloads occur.
"""
import argparse,json,os,platform,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--sdk',type=Path,required=True);p.add_argument('--source',type=Path)
p.add_argument('--include',action='append',type=Path,default=[])
p.add_argument('--runtime-path',action='append',type=Path,default=[])
p.add_argument('--backend',choices=['qpp','custatevec-fp64'],default='qpp')
p.add_argument('--output',type=Path,required=True);p.add_argument('--cxx',default=os.environ.get('CXX','clang++'))
a=p.parse_args();sdk=a.sdk.resolve();out=a.output.resolve();out.parent.mkdir(parents=True,exist_ok=True)
root=Path(__file__).resolve().parent.parent
cmd=[a.cxx,'-std=c++20','-O2','-fPIC','-dynamiclib' if platform.system()=='Darwin' else '-shared',str(root/'runtime/cudaq/bridge.cpp')]
include=[sdk/'include',*a.include]
if a.source:include += [a.source/'runtime',a.source/'runtime/include',a.source/'cudaq/include']
for path in include:cmd += ['-I',str(path.resolve())]
cmd += ['-L',str(sdk/'lib')]
for path in [sdk/'lib',*a.runtime_path]:cmd += ['-Wl,-rpath,'+str(path.resolve())]
cmd += ['-lcudaq-builder','-lcudaq','-lcudaq-common','-lcudaq-platform-default','-lcudaq-em-default','-lnvqir','-lnvqir-'+a.backend]
if platform.system()=='Darwin':cmd += ['-Wl,-install_name,'+str(out)]
cmd += ['-o',str(out)]
r=subprocess.run(cmd,text=True,capture_output=True)
record=dict(command=cmd,exit=r.returncode,stdout=r.stdout,stderr=r.stderr,sdk=str(sdk),backend=a.backend)
out.with_suffix(out.suffix+'.build.json').write_text(json.dumps(record,indent=2));print(r.stdout,end='');print(r.stderr,end='')
if r.returncode:raise SystemExit(r.returncode)
print(out)
