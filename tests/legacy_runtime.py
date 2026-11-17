#!/usr/bin/env python3
"""Allocators, bounded TOML parsing and scalar DAGs with real ownership/failures."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--only', nargs='+', choices=['allocator', 'toml_consumer', 'graph_consumer', 'injected'])
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
fixtures = root / 'tests/regressions/legacy_runtime'
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
cc = env.get('CC', 'clang')
records = []
metadata = {'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
            'modules': {name: hashlib.sha256((root / 'stdlib' / (name + '.aeth')).read_bytes()).hexdigest()
                        for name in ['alloc', 'toml', 'graph']}}


def run(label, command, execute=False):
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        result = subprocess.CompletedProcess(command, 124, '', 'timeout')
    passed = result.returncode == 0 and (not execute or (result.stdout == '' and result.stderr == ''))
    records.append(dict(label=label, command=command, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, passed=passed))
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(result.stderr or result.stdout or f'exit={result.returncode}', flush=True)
    return passed


allocator = work / 'failure_allocator.o'
ready = run('failure-allocator/build', [cc, '-std=c11', '-O0', '-fno-builtin', '-c',
                                       fixtures / 'failure_allocator.c', '-o', allocator])
for name in (args.only or ['allocator', 'toml_consumer', 'graph_consumer', 'injected']):
    source = fixtures / f'{name}.aeth'
    if not run(name + '/check', [compiler, 'check', source, '-I', root / 'stdlib']):
        continue
    for backend in args.backends:
        for level in ['0', '2']:
            label = f'{name}/{backend}/O{level}'
            binary = work / f'{name}-{backend}-O{level}'
            command = [compiler, 'build', source, '-I', root / 'stdlib', '--backend', backend, f'-O{level}', '-o', binary]
            if name in ['graph_consumer', 'injected']:
                command += ['--link-library', 'm']
            if name == 'injected':
                if not ready:
                    continue
                command += ['--link-object', allocator]
            if run(label + '/build', command):
                run(label + '/run', [binary], execute=True)
    if name != 'injected':
        generated = work / (name + '-sanitized.c')
        binary = work / (name + '-sanitized')
        if run(name + '/sanitizer-emit', [compiler, 'build', source, '-I', root / 'stdlib', '--emit-c', '-o', generated]):
            if run(name + '/sanitizer-build', [cc, '-std=gnu11', '-O0', '-g', '-fsanitize=address,undefined',
                                              '-fno-sanitize-recover=all', generated, '-lm', '-o', binary]):
                run(name + '/sanitizer-run', [binary], execute=True)

passed = sum(item['passed'] for item in records)
print(f'{passed}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(0 if passed == len(records) else 1)
