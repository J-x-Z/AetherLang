#!/usr/bin/env python3
"""Native POSIX pipeline, descriptor ownership and interrupted-syscall checks."""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import tempfile

parser=argparse.ArgumentParser()
parser.add_argument('--compiler',type=Path,required=True)
parser.add_argument('--work-dir',type=Path,required=True)
parser.add_argument('--backends',nargs='+',choices=['llvm','c'],default=['llvm','c'])
parser.add_argument('--levels',nargs='+',choices=['0','2'],default=['0','2'])
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
source=root/'tests/regressions/process'
work=args.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
compiler=args.compiler.resolve()
env={**os.environ,'TMPDIR':str(work)}
env.pop('AETHER_CHILD_ONLY',None)
records=[]

def run(command,**options):
    try:
        return subprocess.run(list(map(str,command)),capture_output=True,text=True,timeout=options.pop('timeout',30),
                              env=options.pop('env',env),**options)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(command,124,'','timed out: possible pipeline deadlock')

def low_descriptors():
    resource.setrlimit(resource.RLIMIT_NOFILE,(12,12))

injection=work/('process_faults.dylib' if sys.platform=='darwin' else 'process_faults.so')
flags=['-dynamiclib'] if sys.platform=='darwin' else ['-shared','-fPIC']
build=run([os.environ.get('CC','cc'),*flags,source/'faults.c','-o',injection,*([] if sys.platform=='darwin' else ['-ldl'])])
if build.returncode:raise SystemExit(build.stderr)
injection_key='DYLD_INSERT_LIBRARIES' if sys.platform=='darwin' else 'LD_PRELOAD'
failed=False
for backend in args.backends:
    for level in args.levels:
        binary=work/f'pipeline-{backend}-O{level}'
        build=run([compiler,'build',source/'pipeline.aeth','--backend',backend,f'-O{level}','-o',binary],timeout=120)
        if build.returncode:
            records.append(dict(name='build',backend=backend,level=level,exit=build.returncode,stderr=build.stderr))
            print(f'FAIL build {backend}/O{level}: {build.stderr}',file=sys.stderr)
            failed=True
            continue
        cases=[('pipeline','pipeline',None),('closed-stdin','closed-stdin',None),
               ('argv-literal','argv',None),('real-exit-127','exit-status',None),('signal-poll-reap','signal',None),
               ('environment-cwd-path','envcwd',None),('stdio-swap','swap',None),
               ('exec-failure-cleanup','exec-failure',None),('cwd-failure-cleanup','cwd-failure',None),
               ('invalid-fd-cleanup','fd-failure',None),('invalid-argument-cleanup','argument-failure',None),
               ('invalid-options-all-positions','invalid-options',None),
               ('invalid-options-before-pipes','invalid-options','forbid-pipe'),
               ('invalid-options-before-fork','invalid-options','forbid-fork'),
               ('close-failure-cleanup','close-failure','close-failure'),
               ('wait-eintr','pipeline','wait-eintr'),('read-eintr','pipeline','read-eintr'),
               ('write-eintr','pipeline','write-eintr'),('short-writes','pipeline','write-short'),
               ('zero-write-error','write-zero','write-zero'),
               ('fork-failure-cleanup','fork-failure','fork-failure'),
               ('pipe-failure-cleanup','pipe-failure','pipe-failure'),
               ('late-pipe-failure-cleanup','pipe-failure','late-pipe-failure'),
               ('exec-protocol-failure-cleanup','protocol-failure','read-failure'),
               ('descriptor-exhaustion','pipe-failure',None)]
        for name,mode,fault in cases:
            with tempfile.TemporaryDirectory(prefix='process case ',dir=work) as temp:
                directory=Path(temp)
                output=directory/'output'
                command=[binary,mode,output]
                options={'cwd':directory}
                if mode=='envcwd':command=[binary,mode,directory,binary.name,'PATH='+str(binary.parent)]
                if mode=='swap':command=[binary,mode,directory/'first',directory/'second']
                if fault:options['env']={**env,injection_key:str(injection),'AETHER_PROCESS_FAULT':fault}
                if name=='descriptor-exhaustion':options['preexec_fn']=low_descriptors
                result=run(command,**options)
                valid=result.returncode==0 and result.stdout=='' and result.stderr==''
                if mode in ['pipeline','closed-stdin']:
                    valid=valid and output.read_bytes()==b'pipeline-ok\n' if output.exists() else False
                if mode=='swap':
                    valid=valid and (directory/'first').read_bytes()==b'err' and (directory/'second').read_bytes()==b'out'
                valid=valid and not (directory/'INJECTED').exists()
                records.append(dict(name=name,backend=backend,level=level,exit=result.returncode,
                                    stdout=result.stdout,stderr=result.stderr,passed=valid))
                failed|=not valid
                print(f'{"PASS" if valid else "FAIL"} {name}/{backend}/O{level}',flush=True)
                if not valid:print(result.stderr or f'exit {result.returncode}',file=sys.stderr)
                (work/'results.json').write_text(json.dumps(records,indent=2)+'\n')
(work/'results.json').write_text(json.dumps(records,indent=2)+'\n')
raise SystemExit(1 if failed else 0)
