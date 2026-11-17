#!/usr/bin/env python3
"""Compile the unchanged accumulator service with self-hosted Core C/LLVM output."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import sys
import time
import unittest

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True,help='self-hosted Core compiler')
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
a=p.parse_args()
root=Path(__file__).resolve().parents[1];work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
compiler=a.compiler.resolve();cc=shutil.which(os.environ.get('CC','clang'))
records=[];env={**os.environ,'TMPDIR':str(work)}

def check(name,good,**facts):
    records.append(dict(name=name,passed=bool(good),**facts))
    (work/'results.json').write_text(json.dumps(records,indent=2)+'\n')
    print(f'{"PASS" if good else "FAIL"} {name}',flush=True)
    if not good:raise SystemExit(name)

def run(name,cmd,expected=0,**options):
    cmd=list(map(str,cmd));start=time.monotonic()
    r=subprocess.run(cmd,capture_output=True,text=True,env=env,timeout=30,**options)
    check(name,r.returncode==expected,command=cmd,exit=r.returncode,stdout=r.stdout,stderr=r.stderr,seconds=time.monotonic()-start)
    return r

# Import the established failure oracle rather than rewriting its semantics.
saved=sys.argv
sys.argv=['runtime_foundation.py','--compiler',str(compiler),'--work-dir',str(work/'faults')]
spec=importlib.util.spec_from_file_location('foundation_oracle',root/'tests/runtime_foundation.py')
foundation=importlib.util.module_from_spec(spec);sys.dont_write_bytecode=True;spec.loader.exec_module(foundation)
sys.argv=saved
labels=[]
for backend in a.backends:
    source=work/('accumulator.'+('ll' if backend=='llvm' else 'c'))
    run('emit-'+backend,[compiler,root/'examples/upgrade/accumulator.aeth','-I',root/'stdlib',
                         *(['--emit-llvm'] if backend=='llvm' else []),'-o',source])
    for level in ['0','2']:
        label=backend+'-O'+level;labels.append(label);binary=work/('accumulator-'+label)
        run('native-build-'+label,[cc,'-O'+level,source,'-o',binary])
        foundation.BINARIES['accumulator',label]=binary
        journal=work/('journal-'+label)
        if journal.exists():journal.unlink()
        protocol='get\nadd 12\nadd -5\nadd x\n'+'x'*5000+'\nget\nreset\nadd 3\nquit\n'
        result=run('protocol-'+label,[binary,journal],input=protocol)
        check('protocol-output-'+label,result.stdout=='READY\nVALUE 0 0\nOK 12\nOK 7\nERR number\nERR too-long\nVALUE 7 2\nRESET\nOK 3\nBYE 3 1\n')
        check('journal-'+label,journal.read_bytes()==b'add 12\nadd -5\nadd 3\n')
        run('missing-journal-'+label,[binary,work/'absent'/label],input='quit\n',expected=4)

for backend in a.backends:
    source=work/('standard_cli.'+('ll' if backend=='llvm' else 'c'))
    run('stdlib-emit-'+backend,[compiler,root/'tests/regressions/bootstrap/standard_cli.aeth','-I',root/'stdlib',
                              *(['--emit-llvm'] if backend=='llvm' else []),'-o',source])
    for level in ['0','2']:
        label=backend+'-O'+level;binary=work/('standard_cli-'+label)
        run('stdlib-build-'+label,[cc,'-O'+level,source,'-o',binary])
        result=run('stdlib-run-'+label,[binary,'-v','hello world','--','-literal'])
        check('stdlib-output-'+label,result.stdout=='standard-cli=ok\n')

foundation.ARGS.backends=labels
flags=['-dynamiclib'] if sys.platform=='darwin' else ['-shared','-fPIC']
run('fault-injection-library',[cc,*flags,root/'tests/regressions/runtime/io_faults.c','-o',foundation.INJECT,
                               *([] if sys.platform=='darwin' else ['-ldl'])])
foundation.RuntimeFoundation.setUpClass=classmethod(lambda cls:None)
methods=['test_service_detects_initial_stdout_error','test_service_keeps_journal_write_error_before_close_error',
         'test_service_detects_final_output_and_close_errors']
suite=unittest.TestSuite(foundation.RuntimeFoundation(method) for method in methods)
result=unittest.TextTestRunner(verbosity=1).run(suite)
(work/'fault-results.json').write_text(json.dumps(foundation.RECORDS,indent=2)+'\n')
check('existing-service-failure-oracle',result.wasSuccessful(),cases=len(foundation.RECORDS),
      failures=len(result.failures),errors=len(result.errors))

# Keep a real process alive between requests and require flushed responses.
label=labels[-1];binary=foundation.BINARIES['accumulator',label]
journal=work/'interactive-journal'
if journal.exists():journal.unlink()
child=subprocess.Popen([binary,journal],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
def line():
    if not select.select([child.stdout],[],[],5)[0]:raise AssertionError('response not flushed within five seconds')
    return child.stdout.readline()
try:
    check('interactive-ready',line()==b'READY\n' and child.poll() is None)
    for command,expected in [(b'add 9\n',b'OK 9\n'),(b'get\n',b'VALUE 9 1\n'),(b'quit\n',b'BYE 9 1\n')]:
        child.stdin.write(command);child.stdin.flush()
        check('interactive-'+command.decode().strip(),line()==expected)
    child.stdin.close();child.wait(timeout=5)
    check('interactive-closed',child.returncode==0 and child.stderr.read()==b'' and journal.read_bytes()==b'add 9\n')
finally:
    if child.poll() is None:child.kill();child.wait()
    child.stdout.close();child.stderr.close()
print(f'Self-hosted production service complete: {len(records)} checks + {len(foundation.RECORDS)} failure cases')

# A current capability receipt, never a passing test for an unsupported feature.
probe=[str(compiler),str(root/'jxz/src/main.aeth'),'-I',str(root/'stdlib'),'--check']
result=subprocess.run(probe,env=env,capture_output=True,text=True,timeout=30)
(work/'jxz-capability.json').write_text(json.dumps(dict(command=probe,exit=result.returncode,
    check_supported=result.returncode==0,native_jxz_verified=False,stdout=result.stdout,stderr=result.stderr),indent=2)+'\n')
