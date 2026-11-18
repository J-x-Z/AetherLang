#!/usr/bin/env python3
"""Check native errno immediately after real POSIX failures in self-hosted code."""
import argparse
import errno
import json
import platform
import subprocess
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', required=True, type=Path)
p.add_argument('--work-dir', required=True, type=Path)
a = p.parse_args()
tool = a.compiler.resolve()
work = a.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
records = []
def invoke(label, command, expected=0):
    cmd = list(map(str,command))
    result = subprocess.run(cmd,capture_output=True,text=True,timeout=30,cwd=work)
    records.append(dict(label=label,command=cmd,status=result.returncode,stdout=result.stdout,stderr=result.stderr))
    (work/'results.json').write_text(json.dumps(records,indent=2))
    assert result.returncode==expected,(label,result)

symbol = {'Darwin':'__error','Linux':'__errno_location'}[platform.system()]
source=work/'read.aeth'
source.write_text('extern "C"{fn close(fd:i32)->i32;fn fopen(s:*u8,mode:*u8)->*void;}'
                  'fn main(argc:i32,argv:**u8)->i32{if argc!=2{return 9;}'
                  'if close(-1)!=-1{return 1;}let first:i32=__errno_get();'
                  'if fopen(argv[1],"rb")!=0 as *void{return 2;}let second:i32=__errno_get();'
                  f'if first!={errno.EBADF} || second!={errno.ENOENT}{{return 3;}}return 0;}}')
for backend in ('c','llvm'):
    for level in (0,2,3):
        binary=work/f'read-{backend}-O{level}'
        invoke(f'build {backend} O{level}',[tool,'build',source,'--backend',backend,f'-O{level}','-o',binary])
        invoke(f'run {backend} O{level}',[binary,work/'absent'/'child'])
for explicit in (False,True):
    path=work/f'declaration-{explicit}.aeth'
    prefix='extern "C"{fn '+symbol+'()->*i32;}' if explicit else ''
    path.write_text(prefix+source.read_text())
    emitted=work/f'declaration-{explicit}.ll'
    invoke('emit with existing compatible declaration '+str(explicit),[tool,path,'--emit-llvm','-o',emitted])
    assert sum(line.startswith('declare ptr @'+symbol+'(') for line in emitted.read_text().splitlines())==1
bad=work/'bad.aeth'
for label,text in [('arity','fn main(){__errno_get(1);}'),
                   ('declaration','fn __errno_get()->i32{return 7;}fn main(){}'),
                   ('foreign conflict','extern "C"{fn '+symbol+'(x:i64)->i32;}'+source.read_text())]:
    bad.write_text(text)
    for backend in ('c','llvm'):
        invoke(backend+'/'+label,[tool,'check',bad,'--backend',backend],1)
print(f'{len(records)} native Core errno checks passed')
