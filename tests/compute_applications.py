#!/usr/bin/env python3
"""Numerical application acceptance: independent Python oracles, CPU and explicit GPU."""
import argparse, hashlib, json, math, os, re, struct, subprocess, sys, time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--metal',action='store_true',default=os.environ.get('AETHER_TEST_METAL')=='1')
p.add_argument('--verify-output',type=Path,nargs='+',help='Check captured program stdout from an explicit device using the same numerical oracle')
p.add_argument('--provider',type=Path)
p.add_argument('--device',choices=['metal','vulkan'],default='vulkan')
p.add_argument('--no-sanitize',action='store_true')
a=p.parse_args()
if not a.compiler and not a.verify_output:p.error('--compiler is required when building workloads')
if not a.verify_output and a.metal and (a.provider or sys.platform!='darwin'):p.error('--metal requires macOS and cannot be combined with --provider')
r=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
c=a.compiler.resolve() if a.compiler else None;source=r/'tests/regressions/compute/applications.aeth'
env={**os.environ,'TMPDIR':str(w)};cc=os.environ.get('CC','clang');results=[]
def run(label,command,environment=env):
    command=list(map(str,command));started=time.perf_counter()
    process=subprocess.run(command,capture_output=True,text=True,timeout=60,env=environment)
    row=dict(label=label,command=command,exit=process.returncode,stdout=process.stdout,stderr=process.stderr,elapsed_seconds=time.perf_counter()-started)
    results.append(row);(w/'results.json').write_text(json.dumps(results,indent=2))
    assert process.returncode==0,(label,process.returncode,process.stdout,process.stderr)
    return process.stdout
# No Aether implementation is used to calculate these expected numbers.
def f32(x):return struct.unpack('f',struct.pack('f',x))[0]
def mm(x,y):return [[sum(v*y[k][j] for k,v in enumerate(row)) for j in range(len(y[0]))] for row in x]
x=[[(i*6+k)%7/8-3/8 for k in range(6)] for i in range(4)]
u=[[(i*2+k)%5/4-2/4 for k in range(2)] for i in range(6)]
v=[[.5,-.25,.75],[.125,.25,-.5]];weight=mm(u,v);decoded=[]
for row in weight:
    scale=f32(max(map(abs,row))/127) or 1
    codes=[max(-127,min(127,math.floor(abs(f32(value/scale))+.5)*(1 if value>=0 else -1))) for value in row]
    decoded.append([f32(code*scale) for code in codes])
quantized=[z for row in mm(x,decoded) for z in row]
batch=[]
for n in range(2):
    left=[[(n*6+i*3+k-4)*.125 for k in range(3)] for i in range(2)]
    right=[[(5-(n*6+k*2+j))*.25 for j in range(2)] for k in range(3)]
    batch.extend(z for row in mm(left,right) for z in row)
amplitude=(1-.8*math.sin(math.pi/8)**2)**16
field=[max(i*3.125-100,0) for i in range(65)]
for step in range(100):
    update=[0.0]*65
    for i in range(1,64):
        diffusion=.2**2*i*i;drift=.05*i
        update[i]=.0005*(diffusion-drift)*field[i-1]+(1-.001*(diffusion+.05))*field[i]+.0005*(diffusion+drift)*field[i+1]
    update[64]=200-100*math.exp(-.05*.001*(step+1));field=update
normal=lambda x:.5*(1+math.erf(x/math.sqrt(2)))
d1=(.05+.5*.2**2)*.1/(.2*math.sqrt(.1));d2=d1-.2*math.sqrt(.1)
black_scholes=100*normal(d1)-100*math.exp(-.005)*normal(d2)
maxcut=max(sum(((state>>i)&1)!=((state>>j)&1) for i,j in [(0,1),(1,2),(2,3),(3,0)]) for state in range(16))
oracle=dict(quantized=quantized,vmap=batch,diffusion_amplitude=amplitude,finance_grid_value=field[32],finance_analytic_value=black_scholes,finance_boundary=field[64],maxcut=maxcut)
(w/'oracle.json').write_text(json.dumps(oracle,indent=2))
def verify(text,mode):
    lines=text.splitlines();assert len(lines)==7,lines
    neural=re.fullmatch(r'neural lowrank_error=([\d.]+) quant_error=([\d.]+) gradient_error=([\d.]+) packed_bytes=(\d+)',lines[0]);assert neural,lines[0]
    low,quant,grad,packed=map(float,neural.groups());assert low<1e-6 and quant<.002 and grad<1e-5 and packed==44
    for line,key,tolerance in [(lines[1],'quantized',2e-7),(lines[2],'vmap',1e-7)]:
        pieces=line.split();assert pieces[0]==key;actual=list(map(float,pieces[1:]));assert len(actual)==len(oracle[key]);assert max(abs(x-y) for x,y in zip(actual,oracle[key]))<tolerance,(key,actual,oracle[key])
    match=re.fullmatch(r'diffusion error=([\d.]+) mass=([-\d.]+) amplitude=([-\d.]+)',lines[3]);assert match
    error,mass,actual=map(float,match.groups());assert error<1e-5 and abs(mass)<1e-5 and abs(actual-amplitude)<1e-5
    match=re.fullmatch(r'finance value=([\d.]+) boundary=([\d.]+)',lines[4]);assert match
    value,boundary=map(float,match.groups());assert abs(value-field[32])<2e-4 and abs(value-black_scholes)<.15 and abs(boundary-field[64])<2e-5
    assert lines[5]==f'maxcut optimum={maxcut}'
    # Optional blank lines are not accepted; every output is asserted below.
    match=re.fullmatch(r'applications backend=(\d+) device=(\d+) submissions=(\d+) peak=(\d+) remaining=(\d+)',lines[-1]);assert match,lines[-1]
    backend,device,submissions,peak,remaining=map(int,match.groups());assert backend=={'cpu':1,'metal':2,'vulkan':3}[mode] and submissions==125 and 0<peak<=4194304 and remaining==0
    if mode=='cpu':assert device==0
    return dict(backend=backend,device=device,submissions=submissions,peak_bytes=peak,finance_error=abs(value-field[32]))
if a.verify_output:
    for capture in a.verify_output:
        verify(capture.read_text(), a.device)
    print(f'{len(a.verify_output)} captured {a.device} workloads passed the numerical oracle')
    sys.exit(0)
loader=w/'loader.o';run('portable loader',[cc,'-O2','-std=c11','-c',r/'runtime/compute/loader.c','-o',loader])
provider=a.provider.resolve() if a.provider else None;device=a.device
if a.metal:
    device='metal';provider=w/'metal.dylib'
    run('real Metal provider',['clang','-O2','-fobjc-arc','-dynamiclib',r/'runtime/metal/provider.m',r/'runtime/metal/bridge.m','-framework','Metal','-framework','Foundation','-o',provider])
link=[] if sys.platform=='darwin' else ['-l','dl','-l','m']
run('check applications',[c,'check',source,'--sysroot',r])
measurements=[]
def execution(binary,mode,library=None,environment=env):
    text=run(f'{binary.name}/{mode}',[binary,mode,*([library] if library else [])],environment)
    metrics=verify(text,mode);metrics['program_elapsed_seconds']=results[-1]['elapsed_seconds'];metrics['includes_device_initialization']=True
    measurements.append(metrics);(w/'measurements.json').write_text(json.dumps(measurements,indent=2))
for backend in a.backends:
    for opt in (0,2):
        binary=w/f'applications-{backend}-O{opt}'
        run(f'{backend}/O{opt}/build',[c,source,'--sysroot',r,'--backend',backend,f'-O{opt}','--link-object',loader,*link,'-o',binary])
        execution(binary,'cpu')
        if provider:execution(binary,device,provider)
if not a.no_sanitize:
    csource=w/'sanitized.c';binary=w/'sanitized'
    run('emit sanitizer C',[c,source,'--sysroot',r,'--emit-c','-o',csource])
    run('ASan UBSan build',[cc,'-std=c11','-O0','-gline-tables-only','-fno-omit-frame-pointer','-fsanitize=address,undefined',csource,r/'runtime/compute/loader.c',*link,'-o',binary])
    sanitizer={**env,'ASAN_OPTIONS':'halt_on_error=1:exitcode=86','UBSAN_OPTIONS':'halt_on_error=1:exitcode=87'}
    execution(binary,'cpu',environment=sanitizer)
    if provider:execution(binary,device,provider,sanitizer)
paths=[c,source,r/'stdlib/compute.aeth',r/'stdlib/compute_types.aeth',r/'stdlib/compute_ops.aeth',r/'stdlib/compute_sparse.aeth',r/'stdlib/compute_quant.aeth',r/'runtime/compute/provider.h',r/'runtime/compute/loader.c']
if provider:paths.append(provider)
(w/'hashes.json').write_text(json.dumps({str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in paths},indent=2))
print(f'{len(results)} application checks passed; GPU: {device if provider else "not requested"}; evidence: {w/"results.json"}')
