#!/usr/bin/env python3
"""Native Aether graph JIT acceptance, content-addressed cache and independent oracle."""
import argparse, fcntl, hashlib, json, math, os, re, shutil, subprocess, sys, time, tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--alternate-compiler',type=Path);p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--no-sanitize',action='store_true');a=p.parse_args()
r=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
env={**os.environ,'TMPDIR':str(w)};cc=Path(shutil.which(os.environ.get('CC','clang'))).resolve();compiler=w/'frozen compiler ü.aethc';shutil.copy2(a.compiler.resolve(),compiler)
source=r/'tests/regressions/compute/jit.aeth';results=[]
cases=Path(tempfile.mkdtemp(prefix='cases-',dir=w))
def run(label,command,ok=True,environment=env):
 command=list(map(str,command));start=time.perf_counter();value=subprocess.run(command,capture_output=True,text=True,timeout=120,env=environment)
 results.append(dict(label=label,command=command,exit=value.returncode,stdout=value.stdout,stderr=value.stderr,elapsed_seconds=time.perf_counter()-start));(w/'results.json').write_text(json.dumps(results,indent=2))
 assert (value.returncode==0)==ok,(label,value.returncode,value.stdout,value.stderr)
 return value
link=[] if sys.platform=='darwin' else ['-l','dl','-l','m']
objects=[]
for name in ('jit','loader'):
 obj=w/f'{name}.o';run(f'{name} platform bridge',[cc,'-std=c11','-O2','-c',r/f'runtime/compute/{name}.c','-o',obj]);objects+=['--link-object',obj]
outer=[]
for backend in a.backends:
 binary=w/f'host-{backend}'
 run(f'{backend} host build',[compiler,source,'--sysroot',r,'--backend',backend,'-O2',*objects,*link,'-o',binary]);outer.append(binary)
x=[[-1,.25,.5],[0,.75,-.5],[1,.5,-.25],[-.5,-.25,.75]];weights=[[.5,-.25],[.75,.125],[-.5,.625]];bias=[.125,-.25]
def execute(label,binary,cache,backend='llvm',opt=2,mode='cold',tool=compiler,linker=cc,ok=True,environment=env):
 value=run(label,[binary,tool,linker,cache,backend,str(opt),mode],ok,environment)
 if not ok:return None
 lines=value.stdout.splitlines();assert len(lines)==2,lines
 actual=list(map(float,lines[0].removeprefix('jit values ').split()));factor=.75 if mode=='changed' else .5;samples=2 if mode=='short' else 4
 expected=[math.tanh(factor*(sum(x[i][k]*weights[k][j] for k in range(3))+bias[j]))**2 for i in range(samples) for j in range(2)]
 assert len(actual)==len(expected) and max(abs(x-y) for x,y in zip(actual,expected))<2e-7,(actual,expected)
 match=re.fullmatch(r'jit key=([0-9a-f]{64}) cache_hit=([01])',lines[1]);assert match and int(match[2])==(mode=='warm'),lines[1]
 return match[1]
cache=cases/'cache with spaces ü';keys=set()
for backend in a.backends:
 for opt in (0,2):
  key=execute(f'{backend}/O{opt}/cold',outer[0],cache,backend,opt);assert key not in keys;keys.add(key)
  assert execute(f'{backend}/O{opt}/warm',outer[0],cache,backend,opt,'warm')==key
backend=a.backends[-1];key=execute('shared cache across host backends',outer[-1],cache,backend,2,'warm')
changed=execute('graph IR invalidates',outer[0],cache,backend,2,'changed');assert changed not in keys;keys.add(changed)
short=execute('vmap shape invalidates',outer[0],cache,backend,2,'short');assert short not in keys;keys.add(short)
# Detect corrupted bytes before dlopen, preserve the invalid entry, rebuild it.
artifact=cache/key/'kernel.so';artifact.write_bytes(artifact.read_bytes()+b'corrupt')
assert execute('artifact SHA invalidation',outer[0],cache,backend)==key
assert list(cache.glob('.invalid-*'))
if a.alternate_compiler:
 alternate=a.alternate_compiler.resolve();assert hashlib.sha256(alternate.read_bytes()).digest()!=hashlib.sha256(compiler.read_bytes()).digest()
 replacement=w/'replacement compiler';shutil.copy2(alternate,replacement)
 changed_compiler=execute('compiler SHA invalidates',outer[0],cache,backend,tool=replacement);assert changed_compiler not in keys
 # Same compiler bytes at a different path must identify the same artifact.
 same=w/'same bytes different path';shutil.copy2(compiler,same)
 assert execute('compiler identity is content',outer[0],cache,backend,mode='warm',tool=same)==key
# Failed compilation cannot publish a partial cache entry or leak a private stage.
failed=cases/'failed cache';false=Path(shutil.which('false')).resolve()
execute('compiler/linker error propagates',outer[0],failed,backend,linker=false,ok=False)
assert sorted(x.name for x in failed.iterdir())==['owner']
unknown=cases/'unowned';unknown.mkdir();sentinel=unknown/'keep';sentinel.write_bytes(b'user data')
execute('unowned cache preserved',outer[0],unknown,backend,ok=False);assert sentinel.read_bytes()==b'user data'
symlink=cases/'cache symlink';symlink.symlink_to(cache,target_is_directory=True)
execute('cache symlink rejected',outer[0],symlink,backend,ok=False)
held=os.open(cache,os.O_RDONLY)
try:
 fcntl.flock(held,fcntl.LOCK_EX|fcntl.LOCK_NB)
 execute('concurrent cache use is explicit busy',outer[0],cache,backend,mode='warm',ok=False)
finally:
 os.close(held)
if not a.no_sanitize:
 csource=w/'host-sanitized.c';binary=w/'host-sanitized'
 run('emit sanitizer host',[compiler,source,'--sysroot',r,'--emit-c','-o',csource])
 run('sanitizer host build',[cc,'-std=c11','-O0','-gline-tables-only','-fno-omit-frame-pointer','-fsanitize=address,undefined',csource,r/'runtime/compute/jit.c',r/'runtime/compute/loader.c',*link,'-o',binary])
 # A real compiler adapter adds instrumentation to both generated C object and
 # dynamic library. Runtime still executes argv; it never constructs a shell.
 adapter=w/'asan-cc';adapter.write_text('#!'+sys.executable+'\nimport os,sys\nos.execv('+repr(str(cc))+','+repr([str(cc),'-fsanitize=address,undefined'])+'+sys.argv[1:])\n');adapter.chmod(0o755)
 sanitizer={**env,'ASAN_OPTIONS':'halt_on_error=1:exitcode=86','UBSAN_OPTIONS':'halt_on_error=1:exitcode=87'}
 execute('sanitized host and generated kernel',binary,cases/'sanitized cache','c',2,linker=adapter,environment=sanitizer)
paths=[compiler,source,r/'stdlib/compute_graph.aeth',r/'stdlib/compute_jit.aeth',r/'stdlib/compute_jit_source.aeth',r/'runtime/compute/jit.c']
(w/'hashes.json').write_text(json.dumps({str(v):hashlib.sha256(v.read_bytes()).hexdigest() for v in paths},indent=2))
print(f'{len(results)} graph JIT checks passed; evidence: {w/"results.json"}')
