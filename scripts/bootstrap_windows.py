#!/usr/bin/env python3
"""Prepare and cross-link the Aether-written compiler for Windows x64; never run it."""
import argparse,hashlib,json,os,re,shutil,struct,subprocess,tempfile
from pathlib import Path
from windows_support import inspect_pe
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',required=True,type=Path,help='Existing native Aether-built seed')
p.add_argument('--work-dir',required=True,type=Path)
p.add_argument('--core-source',type=Path)
p.add_argument('--zig',default='zig')
a=p.parse_args();repo=Path(__file__).resolve().parent.parent
source=(a.core_source or repo/'src_aether/core').resolve();seed=a.compiler.resolve()
base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='run-',dir=base));(base/'latest.json').write_text(json.dumps(dict(run_dir=str(w),results=str(w/'results.json')),indent=2))
selected=w/'source';shutil.copytree(source,selected)
# Platform selection is the only target-dependent source change.
shutil.copyfile(selected/'platform_windows.aeth',selected/'host_platform.aeth')
cache=base/'zig-cache';cache.mkdir(exist_ok=True);local=w/'zig-local';local.mkdir()
env={**os.environ,'ZIG_GLOBAL_CACHE_DIR':str(cache),'ZIG_LOCAL_CACHE_DIR':str(local),'TMPDIR':str(w)}
records=[]
report=dict(native_windows_executed=False,status='in-progress',seed=str(seed),seed_sha256=hashlib.sha256(seed.read_bytes()).hexdigest(),target='x86_64-windows-gnu',source_hashes={str(f.relative_to(selected)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(selected.glob('*.aeth'))},checks=records)
def save(): (w/'results.json').write_text(json.dumps(report,indent=2))
def run(label,cmd):
 r=subprocess.run(list(map(str,cmd)),capture_output=True,timeout=240,env=env);records.append(dict(label=label,command=list(map(str,cmd)),status=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace')));save();assert r.returncode==0,(label,r.stderr.decode(errors='replace'));return r

# Exercise the exact quoting algorithm on this host, using an independent
# Microsoft argv grammar decoder. This is not Windows process execution.
quote_source=w/'quote-source';quote_source.mkdir()
for name in ('model.aeth','host_command.aeth'):shutil.copyfile(selected/name,quote_source/name)
(quote_source/'main.aeth').write_text('use model::*;use host_command::*; fn main(argc:i32,argv:**u8)->i32{let result:*u8=boot_windows_command(argv+1);if result==0 as *u8{return 1;}printf("%s",result);free(result as *void);return 0;}')
quote_binary=w/'quote-host'
run('host builds exact argv encoder',[seed,'build',quote_source/'main.aeth','--backend','c','-O2','-o',quote_binary])
cases=[[],[''],['a'],['a','b'],['space value',''],['Ω中文😀'],['; touch X','$HOME','&','|'],['a"b'],['\\'],['C:\\Program Files\\LLVM\\'],['a\\"b'],['a\\\\"b'],['line\nnext','a\tb'],['"','\\"','\\\\"','\\\\\\"'],['x '*30,'\\'*70]]
def decode(command):
 args=[];i=0
 while i<len(command):
  while i<len(command) and command[i] in ' \t':i+=1
  if i==len(command):break
  arg=[];quoted=False
  while i<len(command):
   if not quoted and command[i] in ' \t':break
   n=0
   while i<len(command) and command[i]=='\\':n+=1;i+=1
   if i<len(command) and command[i]=='"':
    arg.extend('\\'*(n//2))
    if n%2:arg.append('"')
    else:quoted=not quoted
    i+=1
   else:
    arg.extend('\\'*n)
    if i<len(command):
     if not quoted and command[i] in ' \t':break
     arg.append(command[i]);i+=1
  assert not quoted,command
  args.append(''.join(arg))
 return args

for case in cases:
 r=run('host argv encoder oracle',[quote_binary,*case]);assert decode(r.stdout.decode())==case,(case,r.stdout)

run('seed checks selected platform',[seed,'check',selected/'aethc.aeth'])
products=[]
for backend in ['c','llvm']:
 emitted=w/('compiler.c' if backend=='c' else 'compiler.ll')
 run('Aether emits '+backend,[seed,selected/'aethc.aeth','--emit-'+backend,'-o',emitted])
 for level in [0,2]:
  binary=w/f'aethc-{backend}-O{level}.exe'
  run(f'cross link {backend} O{level}',[a.zig,'cc','-target','x86_64-windows-gnu',f'-O{level}',emitted,'-lbcrypt','-lshell32','-o',binary]);products.append(inspect_pe(binary, {'CreateProcessW','GetCommandLineW','MoveFileExW','_errno'}))
# Compare the actual Aether-emitted foreign records to the target SDK definitions.
s=(w/'compiler.c').read_text();m=re.search(r'extern int32_t CreateProcessW\(([^;]+)\);',s);assert m
names=re.findall(r'(a\d+_s\d+)\*',m.group(1));assert len(names)==2
out=['#include <windows.h>','#include <stdint.h>','#include <stddef.h>','_Static_assert(sizeof(void*)==8,"x64");','_Static_assert(sizeof(long)==4,"LLP64");','_Static_assert(sizeof(wchar_t)==2,"UTF16");']
fields=[['cb','lpReserved','lpDesktop','lpTitle','dwX','dwY','dwXSize','dwYSize','dwXCountChars','dwYCountChars','dwFillAttribute','dwFlags','wShowWindow','cbReserved2','lpReserved2','hStdInput','hStdOutput','hStdError'],['hProcess','hThread','dwProcessId','dwThreadId']]
for name,api,expected in zip(names,['STARTUPINFOW','PROCESS_INFORMATION'],fields):
 block=re.search(r'struct '+name+r' \{.*?\};',s,re.S).group(0);out.append(block);members=re.findall(r'\b(a\d+_m\d+);',block);assert len(members)==len(expected)
 out.extend([f'_Static_assert(sizeof(struct {name})==sizeof({api}),"size");',f'_Static_assert(_Alignof(struct {name})==_Alignof({api}),"alignment");'])
 for f,g in zip(members,expected):out.append(f'_Static_assert(offsetof(struct {name},{f})==offsetof({api},{g}),"{api}.{g}");')
oracle=w/'layout.c';oracle.write_text('\n'.join(out)+'\n');run('Windows SDK layout oracle',[a.zig,'cc','-target','x86_64-windows-gnu','-std=c11','-c',oracle,'-o',w/'layout.obj'])
shutil.copyfile(w/'aethc-llvm-O2.exe',w/'aethc.exe');report.update(status='cross-compiled-and-linked; native execution pending',products=products,artifact=str(w/'aethc.exe'));save();print(w/'results.json')
