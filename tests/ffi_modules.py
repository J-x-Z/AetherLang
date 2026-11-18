#!/usr/bin/env python3
"""Check imports and run SDK consumers when their actual native libraries are installed."""
import argparse
import json
import os
import re
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
p.add_argument('--openblas', type=Path, default=os.environ.get('OPENBLAS_LIBRARY'))
p.add_argument('--metal', action='store_true', default=os.environ.get('AETHER_TEST_METAL') == '1')
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

def execute(name, library, objects=(), output=None):
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
            stdout = run([out], env)
            if output:
                assert re.fullmatch(output, stdout), stdout

for name in ['llvm_targets', 'jni_consumer', 'openblas', 'metal_compute']:
    run([compiler, 'check', fixture(name)])

prefix = os.getenv('AETHER_LLVM_PREFIX') or os.getenv('LLVM_SYS_211_PREFIX')
config = args.llvm_config or (Path(prefix) / 'bin/llvm-config' if prefix else shutil.which('llvm-config-21'))
if config:
    assert run([config,'--version']).startswith(('21.1.', '23.1.'))
    includes = run([config, '--includedir'])
    libdir = Path(run([config, '--libdir']))
    libraries = run([config, '--libnames', '--link-shared']).split()
    assert len(libraries) == 1, 'expected monolithic LLVM shared library'
    bridge = work / 'llvm-target-init.o'
    run([os.environ.get('CC', 'clang'), '-std=c11', '-I', includes, '-c', root / 'runtime/llvm/target_init.c', '-o', bridge])
    execute('llvm_targets', libdir / libraries[0], [bridge])
else:
    print('LLVM SDK execution skipped: set AETHER_LLVM_PREFIX or --llvm-config')

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
if args.openblas:
    execute('openblas', args.openblas.resolve())
else:
    print('OpenBLAS execution skipped: set OPENBLAS_LIBRARY or --openblas to an LP64 shared library')

if args.metal:
    if sys.platform != 'darwin':
        p.error('--metal needs a macOS host with a Metal device')
    bridge = work / 'libaether_metal.dylib'
    run(['clang', '-O2', '-fobjc-arc', '-dynamiclib', root / 'runtime/metal/bridge.m',
         '-framework', 'Metal', '-framework', 'Foundation',
         '-install_name', '@rpath/libaether_metal.dylib', '-o', bridge])
    execute('metal_compute', bridge, output=r'metal registry=[1-9][0-9]* values=257 first=-255 last=257')
else:
    print('Metal device execution skipped: enable --metal or AETHER_TEST_METAL=1')
print(f'{len(records)} module import, SDK and native program checks passed')
