#!/usr/bin/env python3
"""Pairwise contraction and Jacobi low-rank factorization with independent oracles."""
import argparse, hashlib, json, os, re, subprocess, sys, time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm']);p.add_argument('--metal',action='store_true',default=os.environ.get('AETHER_TEST_METAL')=='1')
p.add_argument('--provider',type=Path);p.add_argument('--device',choices=['metal','vulkan'],default='vulkan');p.add_argument('--no-sanitize',action='store_true');a=p.parse_args()
if a.metal and (a.provider or sys.platform!='darwin'):p.error('--metal requires macOS and cannot be combined with --provider')
r=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True);c=a.compiler.resolve();source=r/'tests/regressions/compute/planning.aeth'
cc=os.environ.get('CC','clang');env={**os.environ,'TMPDIR':str(w)};results=[]
def run(label,cmd,environment=env):
 cmd=list(map(str,cmd));start=time.perf_counter();value=subprocess.run(cmd,capture_output=True,text=True,timeout=60,env=environment)
 results.append(dict(label=label,command=cmd,exit=value.returncode,stdout=value.stdout,stderr=value.stderr,elapsed_seconds=time.perf_counter()-start));(w/'results.json').write_text(json.dumps(results,indent=2));assert value.returncode==0,(label,value.stdout,value.stderr);return value.stdout
# Enumerate every matrix-chain parenthesization independently of the library DP.
dims=[5,9,2,7,3]
def possibilities(i,j):
 if i==j:return [(0,None)]
 return [(a+b+dims[i]*dims[k+1]*dims[j+1],k) for k in range(i,j) for a,_ in possibilities(i,k) for b,_ in possibilities(k+1,j)]
cost,split=min(possibilities(0,3));matrices=[]
for operand,(rows,cols) in enumerate(zip(dims,dims[1:])):
 matrices.append([[(((i*cols+j)*5+operand*3)%17-8)*.03125+operand*.015625 for j in range(cols)] for i in range(rows)])
def mm(a,b):return [[sum(a[i][k]*b[k][j] for k in range(len(b))) for j in range(len(b[0]))] for i in range(len(a))]
reference=matrices[0]
for matrix in matrices[1:]:reference=mm(reference,matrix)
flat=[x for row in reference for x in row]
# A=U diag(4,2,.5) V^T, with known orthonormal Hadamard/rotation factors.
# Its rank-2 approximation drops the independent third column exactly.
rank2=[[.4,2.2,0],[2,1,0],[-2,-1,0],[-.4,-2.2,0]]
x=[[.5,.25,-.5,.125],[-.25,.5,.25,-.125]];low=[x for row in mm(x,rank2) for x in row]
oracle=dict(chain_cost=cost,root_split=split,left_associative_cost=sum(dims[0]*dims[k]*dims[k+1] for k in range(1,4)),chain_values=flat,singular_values=[4,2,.5],optimal_rank2_frobenius_residual=.5,low_rank_values=low)
(w/'oracle.json').write_text(json.dumps(oracle,indent=2))
def verify(text,device):
 lines=text.splitlines();assert len(lines)==3,lines
 m=re.fullmatch(r'chain cost=(\d+) split=(\d+) values (.+)',lines[0]);assert m and int(m[1])==cost and int(m[2])==split
 actual=list(map(float,m[3].split()));assert len(actual)==len(flat) and max(abs(x-y) for x,y in zip(actual,flat))<1e-6
 m=re.fullmatch(r'svd singular ([\d.]+) ([\d.]+) ([\d.]+) residual=([\d.]+) sweeps=(\d+) values (.+)',lines[1]);assert m
 assert max(abs(float(m[i+1])-expected) for i,expected in enumerate([4,2,.5,.5]))<2e-6 and 1<=int(m[5])<=64
 actual=list(map(float,m[6].split()));assert len(actual)==len(low) and max(abs(x-y) for x,y in zip(actual,low))<2e-6
 m=re.fullmatch(r'planning backend=(\d+) submitted=(\d+) peak=(\d+) remaining=(\d+)',lines[2]);assert m and int(m[1])=={'cpu':1,'metal':2,'vulkan':3}[device] and int(m[2])==5 and 0<int(m[3])<=1048576 and int(m[4])==0
loader=w/'loader.o';run('portable loader',[cc,'-std=c11','-O2','-c',r/'runtime/compute/loader.c','-o',loader]);provider=a.provider.resolve() if a.provider else None;device=a.device
if a.metal:
 device='metal';provider=w/'metal.dylib';run('real Metal provider',[cc,'-O2','-fobjc-arc','-dynamiclib',r/'runtime/metal/provider.m',r/'runtime/metal/bridge.m','-framework','Metal','-framework','Foundation','-o',provider])
link=[] if sys.platform=='darwin' else ['-l','dl','-l','m']
for backend in a.backends:
 for opt in (0,2):
  binary=w/f'planning-{backend}-O{opt}';run(f'{backend}/O{opt}/build',[c,source,'--sysroot',r,'--backend',backend,f'-O{opt}','--link-object',loader,*link,'-o',binary])
  verify(run(f'{backend}/O{opt}/cpu',[binary,'cpu']),'cpu')
  if provider:verify(run(f'{backend}/O{opt}/{device}',[binary,device,provider]),device)
if not a.no_sanitize:
 generated=w/'sanitized.c';binary=w/'sanitized';run('emit sanitizer C',[c,source,'--sysroot',r,'--emit-c','-o',generated]);run('sanitizer build',[cc,'-std=c11','-O0','-gline-tables-only','-fno-omit-frame-pointer','-fsanitize=address,undefined',generated,r/'runtime/compute/loader.c',*link,'-o',binary])
 sanitizer={**env,'ASAN_OPTIONS':'halt_on_error=1:exitcode=86','UBSAN_OPTIONS':'halt_on_error=1:exitcode=87'}
 verify(run('sanitized/cpu',[binary,'cpu'],sanitizer),'cpu')
 if provider:verify(run('sanitized/'+device,[binary,device,provider],sanitizer),device)
paths=[c,source,r/'stdlib/compute_network.aeth',r/'stdlib/compute_lowrank.aeth',r/'stdlib/compute_graph.aeth',r/'stdlib/compute.aeth']
(w/'hashes.json').write_text(json.dumps({str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in paths},indent=2));print(f'{len(results)} contraction/SVD checks passed; evidence: {w/"results.json"}')
