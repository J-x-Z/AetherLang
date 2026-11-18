#!/usr/bin/env python3
"""Exercise native artifact publication and JXZ with an Aether-built compiler."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', required=True, type=Path)
parser.add_argument('--work-dir', required=True, type=Path)
parser.add_argument('--jxz', type=Path)
parser.add_argument('--cc', default='clang')
options = parser.parse_args()
compiler = options.compiler.resolve()
cc = shutil.which(options.cc)
if not cc:
    parser.error('host C/LLVM compiler unavailable')
root = Path(__file__).resolve().parents[1]
work = options.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
case = Path(tempfile.mkdtemp(prefix='core driver Ω ', dir=work))
env = {**os.environ, 'CC': cc, 'TMPDIR': str(case)}
records = []
def run(label, args, status=0, **kwargs):
    args = list(map(str, args))
    process = subprocess.run(args, cwd=kwargs.pop('cwd', case), env=kwargs.pop('env', env), timeout=150,
                             text=True, capture_output=True, **kwargs)
    records.append(dict(label=label, args=args, exit=process.returncode,
                        stdout=process.stdout, stderr=process.stderr))
    (work/'results.json').write_text(json.dumps(dict(
        compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(), checks=records), indent=2))
    assert process.returncode == status, (label, status, process.returncode, process.stdout, process.stderr)
    return process

module = case/'external package'/'entry.aeth'
module.parent.mkdir()
module.write_text('pub fn value()->i32 { return 9; }')
src = case/'-entry Ω.aeth'
src.write_text('use package::value;extern "C" {fn offset(x:i32)->i32;}'
               'fn main()->i32{return offset(value());}')
foreign = case/'native.c'
foreign.write_text('int offset(int x) {return 3*x+4;}')
obj = case/'@native file.o'
run('independent native object', [cc, '-c', foreign, '-o', obj])
mapping = ['--module', 'package='+str(module)]
for backend in ('llvm', 'c'):
    run(backend+'/check', [compiler,'check','--backend',backend,*mapping,'--',src.name])
    executable = case/('@program '+backend)
    run(backend+'/build with native object', [compiler,'build','--backend',backend,'-O','2',*mapping,
        '--link-object',obj.name,'-o',executable.name,'--',src.name])
    run(backend+'/execute', [executable],31)
    product = case/(backend+'.o')
    run(backend+'/emit object', [compiler,'--emit-obj','--backend',backend,'-O0',*mapping,
                                  '-o',product,'--',src.name])
    run(backend+'/independent link', [cc,product,obj,'-lm','-o',executable])
    run(backend+'/independent execution',[executable],31)
    # The native compiler previously required a separate emit/cc script.
    service = case/('accumulator-'+backend)
    run(backend+'/existing service', [compiler,'build',root/'examples/upgrade/accumulator.aeth',
        '--sysroot',root,'--backend',backend,'-O2','-o',service])
    journal=case/('journal-'+backend)
    child=run(backend+'/service event loop', [service,journal],input='add 4\nget\nquit\n')
    assert child.stdout=='READY\nOK 4\nVALUE 4 1\nBYE 4 1\n' and journal.read_bytes()==b'add 4\n', child
    old=executable.read_bytes()
    for which,tool in [('missing',case/'missing cc'),('unsuccessful',shutil.which('false')),
                       ('empty output',shutil.which('true'))]:
        run(backend+'/'+which, [compiler,'build',*mapping,'--link-object',obj,'-o',executable,
                                '--backend',backend,'--',src.name],
            1,env={**env,'CC':str(tool)})
        assert executable.read_bytes()==old and not list(case.glob('*.tmp.*'))
    saved=product.read_bytes()
    run(backend+'/empty object rejected', [compiler, '--emit-obj','--backend',backend,*mapping,
        '-o',product,'--',src.name],1,env={**env,'CC':str(shutil.which('true'))})
    assert product.read_bytes()==saved
# Link a deterministic POSIX wait implementation into one compiler generation.
# Two consecutive interruptions must keep waiting for exactly the original pid.
wait_source = case/'wait.c'
wait_source.write_text("""#include <sys/wait.h>
#include <errno.h>
pid_t waitpid(pid_t pid, int *status, int flags) {
    static unsigned attempts;
    if (flags || pid <= 0) { errno = EINVAL; return -1; }
    if (attempts++ < 2) { errno = EINTR; return -1; }
    return wait4(pid, status, 0, 0);
}
""")
wait_obj = case/'wait.o'
generation = case/'interrupted-driver'
run('compile interrupted wait implementation',[cc,'-c',wait_source,'-o',wait_obj])
run('link interrupted compiler generation',[compiler,'build',root/'src_aether/core/main.aeth',
    '--backend','llvm','--link-object',wait_obj,'-o',generation])
simple = case/'wait-main.aeth'
simple.write_text('fn main()->i32{return 23;}')
wait_result = case/'wait-result'
run('build after two interruptions',[generation,'build',simple,'-o',wait_result])
run('child result after two interruptions',[wait_result],23)
foreign_aggregate=case/'aggregate.aeth'
foreign_aggregate.write_text('struct P{x:i64} extern "C"{fn consume(p:P)->i32;} fn main()->i32{return 0;}')
for mode in ('check','build'):
    wrong=run('LLVM aggregate rejected by '+mode,[compiler,mode,foreign_aggregate,'--backend','llvm'],1)
    assert 'foreign aggregate parameter' in wrong.stdout
run('C aggregate available',[compiler,'check',foreign_aggregate,'--backend','c'])
run('duplicate binding', [compiler,'check',*mapping,*mapping,'--',src.name],1)
run('invalid binding', [compiler,'check','--module','fn='+str(module),'--',src.name],1)
run('exact binding does not expose sibling roots', [compiler,'check','--module','unrelated='+str(module),'--',src.name],1)
run('disallow object overwrite', [compiler,'build',*mapping,'--link-object',obj,'-o',obj,'--',src.name],1)
run('disallow imported source overwrite', [compiler,'build',*mapping,'-o',module,'--',src.name],1)
conflict = case/'package'
conflict.mkdir()
(conflict/'mod.aeth').write_text('pub fn value()->i32 { return 7; }')
run('reject local conflict', [compiler,'check',*mapping,'--',src.name],1)
assert not list(case.glob('*.tmp.*'))
if options.jxz:
    project = case/'project'
    lib = project/'dependency Ω'
    (project/'src').mkdir(parents=True)
    (lib/'src').mkdir(parents=True)
    (project/'tests').mkdir()
    (lib/'Jxz.toml').write_text("[package]\nname='mathlib'\nversion='0.1.0'\n")
    (lib/'src/lib.aeth').write_text('pub fn value()->i32 {return 17;}')
    (project/'Jxz.toml').write_text("[package]\nname='driverproject'\nversion='0.1.0'\n"
                                    "[dependencies]\nmathlib={path='dependency Ω'}\n")
    (project/'src/main.aeth').write_text('use mathlib::value;fn main()->i32{return value()+3;}')
    (project/'tests/test_value.aeth').write_text('use mathlib::value;fn main()->i32{if value()==17{return 0;}return 1;}')
    for backend in ('c','llvm'):
        command = [options.jxz.resolve()]
        settings = {**env,'AETHC':str(compiler),'AETHER_BACKEND':backend}
        for argv,status in [(['build'],0),(['run'],20),(['test'],0),(['clean'],0)]:
            run('JXZ/'+backend+'/'+argv[0],command+argv,status,cwd=project,env=settings)
    run('no dependencies leaked into the caller directory', [compiler,'check',root/'examples/upgrade/accumulator.aeth','--sysroot',root])
print(f'{len(records)} native Core driver checks passed')
