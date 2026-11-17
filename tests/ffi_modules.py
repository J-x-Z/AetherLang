#!/usr/bin/env python3
"""Check imports and run SDK consumers when their actual native libraries are installed."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', required=True, type=Path)
p.add_argument('--work-dir', required=True, type=Path)
p.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
p.add_argument('--llvm-config', type=Path)
p.add_argument('--java-home', type=Path)
args = p.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
records = []

def run(cmd, env=None):
    cmd = list(map(str, cmd))
    r = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=45)
    records.append(dict(command=cmd, status=r.returncode, stdout=r.stdout, stderr=r.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert not r.returncode, f'{cmd}: {r.stderr} {r.stdout}'
    return r.stdout.strip()

def fixture(name):
    return root / 'tests/regressions/ffi_modules' / (name + '.aeth')

def execute(name, library, objects=()):
    env = os.environ.copy()
    key = 'DYLD_LIBRARY_PATH' if sys.platform == 'darwin' else 'LD_LIBRARY_PATH'
    env[key] = os.pathsep.join(filter(None, [str(library.parent), env.get(key)]))
    for backend in args.backends:
        for opt in (0, 2):
            out = work / f'{name}-{backend}-O{opt}'
            cmd = [compiler, fixture(name), '--backend', backend, f'-O{opt}', '-o', out]
            for obj in [*objects, library]:
                cmd.extend(['--link-object', obj])
            run(cmd)
            run([out], env)

for name in ['llvm_targets', 'jni_consumer']:
    run([compiler, 'check', fixture(name)])

prefix = os.environ.get('LLVM_SYS_211_PREFIX')
config = args.llvm_config or (Path(prefix) / 'bin/llvm-config' if prefix else shutil.which('llvm-config-21'))
if config:
    assert run([config,'--version']).startswith('21.')
    includes = run([config, '--includedir'])
    libdir = Path(run([config, '--libdir']))
    libraries = run([config, '--libnames', '--link-shared']).split()
    assert len(libraries) == 1, 'expected monolithic LLVM shared library'
    bridge = work / 'llvm-target-init.o'
    run([os.environ.get('CC', 'clang'), '-std=c11', '-I', includes, '-c', root / 'runtime/llvm/target_init.c', '-o', bridge])
    execute('llvm_targets', libdir / libraries[0], [bridge])
else:
    print('LLVM SDK execution skipped: set LLVM_SYS_211_PREFIX or --llvm-config')

java = args.java_home or os.environ.get('JAVA_HOME')
if not java and sys.platform == 'darwin':
    probe = subprocess.run(['/usr/libexec/java_home'], capture_output=True, text=True)
    if probe.returncode == 0:
        java = probe.stdout.strip()
if not java and shutil.which('javac'):
    java = Path(shutil.which('javac')).resolve().parents[1]
if java:
    library = Path(java) / 'lib/server' / ('libjvm.dylib' if sys.platform == 'darwin' else 'libjvm.so')
    execute('jni_consumer', library)
else:
    print('JVM execution skipped: set JAVA_HOME or --java-home')
print(f'{len(records)} module import, SDK and native program checks passed')
