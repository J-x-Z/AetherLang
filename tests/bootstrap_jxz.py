#!/usr/bin/env python3
"""Build JXZ in Aether, then use a self-hosted compiler for actual local projects."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True,help='self-hosted compiler that builds JXZ')
p.add_argument('--project-compiler',type=Path,help='JXZ AETHC; defaults to the same self-hosted compiler')
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['llvm','c'],default=['llvm','c'])
a=p.parse_args();root=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
builder=a.compiler.resolve();project_compiler=(a.project_compiler or a.compiler).resolve();records=[]
metadata=dict(jxz_builder=str(builder),jxz_builder_sha256=hashlib.sha256(builder.read_bytes()).hexdigest(),
              project_compiler=str(project_compiler),project_compiler_sha256=hashlib.sha256(project_compiler.read_bytes()).hexdigest(),
              jxz_source={str(f.relative_to(root)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted((root/'jxz/src').glob('*.aeth'))},
              untested_commands=['lsp'],backends=a.backends)

def record(name,passed,**facts):
    records.append(dict(name=name,passed=bool(passed),**facts))
    (w/'results.json').write_text(json.dumps(dict(metadata=metadata,checks=records),indent=2)+'\n')
    print(('PASS ' if passed else 'FAIL ')+name,flush=True)
    if not passed:raise SystemExit(name)

def run(name,cmd,cwd,env,expected=0,contains=None):
    cmd=list(map(str,cmd));started=time.monotonic()
    try:r=subprocess.run(cmd,cwd=cwd,env=env,capture_output=True,text=True,timeout=120)
    except subprocess.TimeoutExpired:record(name,False,command=cmd,error='timeout')
    record(name,r.returncode==expected and (contains is None or contains in r.stdout+r.stderr),command=cmd,
           cwd=str(cwd),exit=r.returncode,stdout=r.stdout,stderr=r.stderr,elapsed_seconds=time.monotonic()-started)
    return r

for backend in a.backends:
    area=w/backend;area.mkdir(exist_ok=True)
    env={**os.environ,'TMPDIR':str(area),'AETHC':str(project_compiler),'AETHER_BACKEND':backend,'AETHER_PATH':str(root/'stdlib')}
    jxz=area/'jxz';run(backend+'/build-jxz',[builder,'build',root/'jxz/src/main.aeth','--sysroot',root,'--backend',backend,'-O2','-o',jxz],area,env)
    case=area/'project Ω space';case.mkdir(exist_ok=True)
    def command(label,*args,expected=0,contains=None):return run(backend+'/'+label,[jxz,*args],case,env,expected,contains)
    command('init','init','demo')
    record(backend+'/init-files',(case/'Jxz.toml').is_file() and (case/'src/main.aeth').is_file() and (case/'tests').is_dir())
    command('build','build');command('hello-run','run',contains='Hello, AetherLang!')
    main=case/'src/main.aeth'
    main.write_text('extern "C"{fn strcmp(a:*u8,b:*u8)->i32;} fn main(argc:i32,argv:**u8)->i32{if argc!=3{return 80;}if strcmp(argv[1],"hello Ω world")!=0{return 81;}if strcmp(argv[2],"; touch INJECTED")!=0{return 82;}return 37;}')
    command('argv-exit','run','--','hello Ω world','; touch INJECTED',expected=37)
    record(backend+'/argv-no-shell-expansion',not (case/'INJECTED').exists())
    main.write_text('extern "C"{fn raise(sig:i32)->i32;}fn main()->i32{raise(15);return 0;}')
    command('signal-exit','run',expected=143)
    test=case/'tests/test_space Ω.aeth';test.write_text('fn main()->i32{return 0;}')
    command('test-pass','test',contains='test_space Ω.aeth')
    test.write_text('fn main()->i32{return 9;}');command('test-exit-visible','test',expected=1)
    test.write_text('fn main()->i32{return true;}');command('test-type-error','test',expected=1)
    record(backend+'/failed-test-not-published',not (case/'target/.jxz-test').exists())
    test.write_text('fn main()->i32{return 0;}')
    saved=(case/'target/demo').read_bytes();main.write_text('fn main()->i32{return false;}')
    command('build-type-error','build',expected=1)
    record(backend+'/failed-build-preserved',(case/'target/demo').read_bytes()==saved)
    # Actual package roots, a diamond graph and sibling source resolution.
    deps=case/'deps';common=deps/'common Ω';left=deps/'left';right=deps/'right'
    for folder,name in [(common,'common'),(left,'left'),(right,'right')]:
        (folder/'src').mkdir(parents=True);(folder/'Jxz.toml').write_text(f"[package]\nname='{name}'\n")
    (common/'src/lib.aeth').write_text('use helper::base;pub fn number()->i32{base()}')
    helper=common/'src/helper.aeth';helper.write_text('pub fn base()->i32{7}')
    for folder,extra in [(left,10),(right,12)]:
        with (folder/'Jxz.toml').open('a') as f:f.write("[dependencies]\ncommon={path='../common Ω'}\n")
        (folder/'src/lib.aeth').write_text(f'use common::number;pub fn value()->i32{{number()+{extra}}}')
    with (case/'Jxz.toml').open('a') as f:f.write("\n[dependencies]\nleft={path='deps/left'}\nright={path='deps/right'}\n")
    main.write_text('use left::value as a;use right::value as b;fn main()->i32{a()+b()}')
    command('diamond-run','run',expected=36)
    helper.write_text('pub fn base()->i32{9}');command('dependency-change','run',expected=40)
    test.write_text('use left::value;fn main()->i32{if value()!=19{return 1;}return 0;}')
    command('dependency-tests','test',contains='test_space Ω.aeth')
    record(backend+'/dependency-record',(case/'target/.jxz-dependencies').is_file())
    user=case/'target/user.txt';user.write_text('owned by project user')
    command('clean','clean');record(backend+'/clean-ownership',not (case/'target/demo').exists() and user.read_text()=='owned by project user' and helper.is_file())
    command('rebuild-after-clean','run',expected=40)
print(f'{len(records)} self-hosted JXZ checks passed')
