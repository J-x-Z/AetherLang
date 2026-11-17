#!/usr/bin/env python3
"""Self-hosted Core -> direct LLVM IR -> three native generations, no Rust calls."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess
import sys
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True, help='an already self-hosted Core compiler, not Rust aethc')
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--cc', default=os.environ.get('CC', 'clang'))
a = p.parse_args()
root = Path(__file__).resolve().parents[1]
w = a.work_dir.resolve()
w.mkdir(parents=True, exist_ok=True)
cc = shutil.which(a.cc)
if not cc:
    raise SystemExit(f'Native compiler not found: {a.cc}')
records = []
source = w / 'core-source'
source.mkdir(exist_ok=True)
files = sorted((root / 'src_aether/core').glob('*.aeth'))
for file in files:
    shutil.copyfile(file, source / file.name)
entry = source / 'main.aeth'
metadata = dict(seed=str(a.compiler.resolve()), seed_sha256=hashlib.sha256(a.compiler.read_bytes()).hexdigest(),
                source_files={f.name:hashlib.sha256(f.read_bytes()).hexdigest() for f in files},
                source_bytes=sum(f.stat().st_size for f in files), source_loading='compiler-native-module-graph',
                host=sys.platform, cc=cc, commit=subprocess.run(['git','rev-parse','HEAD'],cwd=root,capture_output=True,text=True).stdout.strip())
trap = w / 'trap-bin'
trap.mkdir(exist_ok=True)
trap_record = w / 'unexpected-aethc'
for name in ['aethc', 'aethc-c']:
    target = trap / name
    target.write_text('#!/bin/sh\nprintf "unexpected compiler call\\n" >> "$BOOTSTRAP_TRAP_RECORD"\nexit 109\n')
    target.chmod(0o755)
env = {**os.environ, 'TMPDIR':str(w), 'PATH':str(trap)+os.pathsep+os.environ.get('PATH',''),
       'AETHC':str(trap/'aethc'), 'BOOTSTRAP_TRAP_RECORD':str(trap_record)}


def check(name, passed, **facts):
    records.append(dict(name=name,passed=bool(passed),**facts))
    (w/'results.json').write_text(json.dumps(dict(metadata=metadata,checks=records),indent=2)+'\n')
    print(f'{"PASS" if passed else "FAIL"} {name}',flush=True)
    if not passed:
        raise SystemExit('LLVM bootstrap stopped: '+name)


def run(name,command,expected=0,timeout=180,**options):
    command=list(map(str,command));start=time.monotonic()
    try:
        r=subprocess.run(command,cwd=w,env=options.pop('env',env),capture_output=True,text=True,timeout=timeout,**options)
    except subprocess.TimeoutExpired:
        check(name,False,command=command,error='timeout')
    (w/(name+'.stdout')).write_text(r.stdout)
    (w/(name+'.stderr')).write_text(r.stderr)
    good=r.returncode in expected if isinstance(expected,list) else r.returncode==expected
    check(name,good and not trap_record.exists(),command=command,exit=r.returncode,expected=expected,
          seconds=time.monotonic()-start,stdout=r.stdout,stderr=r.stderr)
    return r


def equal(name,*paths):
    data=[path.read_bytes() for path in paths]
    check(name,all(item==data[0] for item in data),sha256=hashlib.sha256(data[0]).hexdigest(),bytes=len(data[0]))


seed = a.compiler.resolve()
stages={i:w/f'stage{i}' for i in range(1,5)}
run('core-seed-check',[seed,entry,'--check'])
run('core-seed-emit-c',[seed,entry,'-o',w/'stage1.c'])
run('build-stage1',[cc,'-O2',w/'stage1.c','-lm','-o',stages[1]])
for i in range(1,4):
    run(f'stage{i}-check-self',[stages[i],entry,'--check'])
    run(f'stage{i}-emit-llvm',[stages[i],entry,'--emit-llvm','-o',w/f'stage{i+1}.ll'])
    run(f'build-stage{i+1}',[cc,'-O2',w/f'stage{i+1}.ll','-lm','-o',stages[i+1]])
run('stage4-check-self',[stages[4],entry,'--check'])
equal('llvm-three-generations-identical',w/'stage2.ll',w/'stage3.ll',w/'stage4.ll')
run('llvm-built-compiler-emits-c',[stages[4],entry,'-o',w/'roundtrip.c'])
equal('c-backend-roundtrip-identical',w/'stage1.c',w/'roundtrip.c')
run('shared-semantics',[sys.executable,root/'tests/core_semantics.py','--compiler',stages[4],
                        '--work-dir',w/'semantics','--cc',cc])
run('llvm-module-projects',[sys.executable,root/'tests/bootstrap_modules.py','--compiler',stages[2],
                           '--compare',stages[4],'--format','llvm','--work-dir',w/'modules','--cc',cc])
fixtures={'recursive_heap':'heap=49995000 factorial=3628800 fib=6765\n',
          'file_io':'file-bytes=5\n','effects_numbers':'effects=123 wrap=4 decimal=12\n',
          'typed_values':'typed-values=ok\n','foreign_names':'','llvm_abi':'llvm-abi=ok\n'}
froot=root/'tests/regressions/bootstrap'
for name,expected in fixtures.items():
    output=w/(name+'.ll')
    run(name+'-emit',[stages[4],froot/(name+'.aeth'),'--emit-llvm','-o',output])
    companion=froot/(name+'.c');extra=[companion] if companion.is_file() else []
    for level in ['0','2']:
        binary=w/(name+'-O'+level)
        run(name+'-build-O'+level,[cc,'-O'+level,output,*extra,'-lm','-o',binary])
        command=[binary]
        if name=='file_io': command += [w/'io-original',w/'io-renamed']
        result=run(name+'-run-O'+level,command)
        check(name+'-output-O'+level,result.stdout==expected and result.stderr=='')
        if name=='file_io':check(name+'-cleanup-O'+level,not (w/'io-original').exists() and not (w/'io-renamed').exists())

# Reuse the independent UTF-8 byte oracle created by the semantic corpus, but
# this time execute LLVM output, rather than its separate C-backend check.
run('unicode-emit-llvm',[stages[4],w/'semantics/string_escape_bytes.aeth','--emit-llvm','-o',w/'unicode.ll'])
for level in ['0','2']:
    run('unicode-build-O'+level,[cc,'-O'+level,w/'unicode.ll','-o',w/('unicode-O'+level)])
    run('unicode-run-O'+level,[w/('unicode-O'+level)])
for case,signature,message in [
    ('return','fn foreign()->S;','foreign aggregate return ABI'),
    ('parameter','fn foreign(value:S);','foreign aggregate parameter ABI'),
]:
    source=w/('unsupported-'+case+'.aeth')
    source.write_text('struct S{x:i32}extern "C"{'+signature+'}fn main()->i32{return 0;}')
    output=w/('preserved-'+case+'.ll');output.write_text('USER ORIGINAL\n')
    result=run('reject-foreign-aggregate-'+case,[stages[4],source,'--emit-llvm','-o',output],expected=1)
    check('foreign-aggregate-diagnostic-'+case,message in result.stdout+result.stderr and str(source) in result.stdout+result.stderr)
    check('foreign-aggregate-output-preserved-'+case,output.read_text()=='USER ORIGINAL\n')

void_source=w/'void-main.aeth';void_source.write_text('fn main(){return;}')
run('void-main-emit',[stages[4],void_source,'--emit-llvm','-o',w/'void.ll'])
run('void-main-build',[cc,'-O2',w/'void.ll','-o',w/'void-main'])
run('void-main-run',[w/'void-main'])
loop_source=w/'loop-storage.aeth'
loop_source.write_text('extern "C"{fn printf(format:*u8,...)->i32;}fn main()->i32{'
                      'let mut i:i64=0;let mut sum:i64=0;while i<1000000{'
                      'let value:i64=i;let pointer:*i64=&value;sum=sum+*pointer;i=i+1;}'
                      'printf("loop=%lld\\n",sum);return 0;}')
run('loop-storage-emit',[stages[4],loop_source,'--emit-llvm','-o',w/'loop.ll'])
for level in ['0','2']:
    run('loop-storage-build-O'+level,[cc,'-O'+level,w/'loop.ll','-o',w/('loop-O'+level)])
    result=run('loop-storage-run-O'+level,[w/('loop-O'+level)])
    check('loop-storage-output-O'+level,result.stdout=='loop=499999500000\n')

run('trap-emit',[stages[4],froot/'arithmetic_trap.aeth','--emit-llvm','-o',w/'trap.ll'])
def no_core():
    resource.setrlimit(resource.RLIMIT_CORE,(0,0))
for level in ['0','2']:
    binary=w/('trap-O'+level)
    run('trap-build-O'+level,[cc,'-O'+level,w/'trap.ll','-o',binary])
    for case in range(4):
        run(f'trap-{case}-O{level}',[binary,str(case)],expected=[-signal.SIGILL,-signal.SIGTRAP],preexec_fn=no_core)

# Sanitizing the C-built compiler instruments the new Aether-written LLVM emitter.
# Clang IR input alone does not establish C-front-end UBSan instrumentation.
flags=['-O1','-g','-fsanitize=address,undefined','-fno-omit-frame-pointer','-fno-sanitize-recover=all']
sanenv={**env,'ASAN_OPTIONS':'detect_leaks=0:abort_on_error=1','UBSAN_OPTIONS':'halt_on_error=1'}
run('build-sanitized-emitter',[cc,*flags,w/'stage1.c','-lm','-o',w/'sanitized'])
run('sanitized-emit-self',[w/'sanitized',entry,'--emit-llvm','-o',w/'sanitized.ll'],env=sanenv)
equal('sanitized-llvm-identical',w/'sanitized.ll',w/'stage2.ll')
run('sanitized-module-emission',[sys.executable,root/'tests/bootstrap_modules.py','--compiler',w/'sanitized',
                                '--format','llvm','--work-dir',w/'modules-sanitized','--cc',cc],env=sanenv)
run('existing-production-consumers',[sys.executable,root/'tests/bootstrap_production.py','--compiler',stages[4],
                                      '--work-dir',w/'production'])
for backend in ['c','llvm']:
    suffix='ll' if backend=='llvm' else 'c'
    output=w/('sanitized-standard.'+suffix)
    run('sanitized-standard-'+backend,[w/'sanitized',root/'tests/regressions/bootstrap/standard_cli.aeth',
                                      '-I',root/'stdlib',*(['--emit-llvm'] if backend=='llvm' else []),'-o',output],env=sanenv)
    equal('sanitized-standard-identical-'+backend,output,w/'production'/('standard_cli.'+suffix))
metadata['artifacts']={file.name:dict(bytes=file.stat().st_size,sha256=hashlib.sha256(file.read_bytes()).hexdigest())
                      for file in [*stages.values(),w/'stage2.ll',w/'stage3.ll',w/'stage4.ll']}
metadata['complete']=True
check('no-external-rust-compiler',not trap_record.exists())
print(f'Core LLVM bootstrap complete: {len(records)} checks; evidence {w/"results.json"}')
