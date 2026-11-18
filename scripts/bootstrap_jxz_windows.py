#!/usr/bin/env python3
"""Cross-link the native Aether JXZ Windows host. No Windows/Wine execution."""
import argparse, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path
from windows_support import inspect_pe
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',required=True,type=Path);p.add_argument('--work-dir',required=True,type=Path)
p.add_argument('--jxz-source',type=Path);p.add_argument('--stdlib',type=Path);p.add_argument('--host-source',type=Path);p.add_argument('--zig',default='zig')
a=p.parse_args();repo=Path(__file__).resolve().parent.parent;seed=a.compiler.resolve();base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='run-',dir=base));(base/'latest.json').write_text(json.dumps({'run_dir':str(w)},indent=2))
source=w/'source';source.mkdir();stdlib=w/'stdlib';shutil.copytree(a.stdlib or repo/'stdlib',stdlib)
for f in (a.jxz_source or repo/'jxz/src').glob('*.aeth'):shutil.copyfile(f,source/f.name)
for name in ['windows_api.aeth','host_command.aeth']:shutil.copyfile((a.host_source or repo/'src_aether/core')/name,source/name)
shutil.copyfile(source/'host_windows.aeth',source/'host.aeth')
records=[];report={'native_windows_executed':False,'status':'in-progress','seed_sha256':hashlib.sha256(seed.read_bytes()).hexdigest(),'source_hashes':{str(f.relative_to(w)):hashlib.sha256(f.read_bytes()).hexdigest() for folder in [source,stdlib] for f in sorted(folder.rglob('*.aeth'))},'checks':records}
cache=base/'zig-cache';cache.mkdir(exist_ok=True);env={**os.environ,'TMPDIR':str(w),'ZIG_GLOBAL_CACHE_DIR':str(cache),'ZIG_LOCAL_CACHE_DIR':str(w/'zig-local')}
def save():(w/'results.json').write_text(json.dumps(report,indent=2))
def run(label,cmd):
 r=subprocess.run(list(map(str,cmd)),env=env,capture_output=True,timeout=240);records.append({'label':label,'command':list(map(str,cmd)),'status':r.returncode,'stdout':r.stdout.decode(errors='replace'),'stderr':r.stderr.decode(errors='replace')});save();assert r.returncode==0,(label,r.stdout,r.stderr)
run('check Windows JXZ',[seed,'check',source/'main.aeth','-I',stdlib]);products=[]
for backend in ['c','llvm']:
 emitted=w/('jxz.c' if backend=='c' else 'jxz.ll');run('emit '+backend,[seed,source/'main.aeth','-I',stdlib,'--emit-'+backend,'-o',emitted])
 for opt in [0,2]:
  out=w/f'jxz-{backend}-O{opt}.exe';run(f'cross link {backend} O{opt}',[a.zig,'cc','-target','x86_64-windows-gnu',f'-O{opt}',emitted,'-lbcrypt','-lshell32','-o',out]);products.append(inspect_pe(out,{'CreateProcessW','FindFirstFileW','GetCommandLineW','CreateHardLinkW','_errno'}))
# Actual generated foreign-record offsets are checked against the target SDK.
s=(w/'jxz.c').read_text();m=re.search(r'extern void\* FindFirstFileW\(([^;]+)\);',s);assert m
name=re.findall(r'(a\d+_s\d+)\*',m.group(1))[-1];block=re.search(r'struct '+name+r' \{.*?\};',s,re.S).group(0)
# Array fields may use generated wrapper typedefs; include their layouts too.
prefix=s[:s.index('extern ')];definitions='\n'.join(line for line in prefix.splitlines() if not line.startswith('#'))
oracle=w/'layout.c';oracle.write_text('#include <windows.h>\n_Static_assert(DELETE==65536 && CREATE_NEW==1 && FILE_FLAG_DELETE_ON_CLOSE==67108864,"directory lock flags");\n#include <stdint.h>\n#include <stdbool.h>\n#include <stddef.h>\n'+definitions+'\n'+f'_Static_assert(sizeof(struct {name})==sizeof(WIN32_FIND_DATAW),"FindData size");\n'+f'_Static_assert(_Alignof(struct {name})==_Alignof(WIN32_FIND_DATAW),"FindData alignment");\n')
# Fundamental layout and every preceding scalar field determine the filename offset.
fields=re.findall(r'\b(a\d+_m\d+)(?:\[\d+\])?;',block);assert len(fields)==13,block
oracle.write_text(oracle.read_text()+f'_Static_assert(offsetof(struct {name},{fields[0]})==offsetof(WIN32_FIND_DATAW,dwFileAttributes),"attributes");\n'+f'_Static_assert(offsetof(struct {name},{fields[11]})==offsetof(WIN32_FIND_DATAW,cFileName),"name");\n'+f'_Static_assert(offsetof(struct {name},{fields[12]})==offsetof(WIN32_FIND_DATAW,cAlternateFileName),"alternate");\n')
run('Windows enumeration SDK layout',[a.zig,'cc','-target','x86_64-windows-gnu','-std=c11','-c',oracle,'-o',w/'layout.obj'])
shutil.copyfile(w/'jxz-llvm-O2.exe',w/'jxz.exe');report.update(status='cross-compiled-and-linked; native Windows execution pending',products=products,artifact=str(w/'jxz.exe'));save();print(w/'results.json')
