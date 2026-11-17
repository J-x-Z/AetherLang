#!/usr/bin/env python3
"""Aligned Vec growth, allocator failure rollback, ownership and ZST semantics."""
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
parser.add_argument('--stdlib', type=Path, help='Explicit library snapshot for reproducing the original failure')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
fixtures = root / 'tests/regressions/vec_alignment'
library = (args.stdlib or root / 'stdlib').resolve()
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
cc = env.get('CC', 'clang')
records = []
failed = False
metadata = {'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
            'stdlib': str(library), 'vec_sha256': hashlib.sha256((library / 'vec.aeth').read_bytes()).hexdigest()}


def run(label, command, execute=False):
    global failed
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, timeout=60)
    except subprocess.TimeoutExpired:
        result = subprocess.CompletedProcess(command, 124, '', 'timeout')
    passed = result.returncode == 0 and (not execute or (result.stdout == '' and result.stderr == ''))
    records.append(dict(label=label, command=command, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, passed=passed))
    failed |= not passed
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(result.stderr or result.stdout or f'exit={result.returncode}', flush=True)
    return passed


allocator = work / 'alignment_allocator.o'
ready = run('allocator/build', [cc, '-std=c11', '-O0', '-fno-builtin', '-c',
                                 fixtures / 'alignment_allocator.c', '-o', allocator])
for name in ['native', 'injected']:
    source = fixtures / f'{name}.aeth'
    run(name + '/check', [compiler, 'check', source, '-I', library])
    if name == 'injected' and not ready:
        continue
    for backend in args.backends:
        for level in ['0', '2']:
            label = f'{name}/{backend}/O{level}'
            binary = work / f'{name}-{backend}-O{level}'
            command = [compiler, 'build', source, '-I', library, '--backend', backend,
                       f'-O{level}', '-o', binary]
            if name == 'injected':
                command += ['--link-object', allocator]
            if run(label + '/build', command):
                run(label + '/run', [binary], execute=True)

# Use the real system allocator with ASan; interposing malloc would disable its
# allocation-boundary tracking. The Aether workload still grows/copies/frees.
generated = work / 'native-sanitized.c'
binary = work / 'native-sanitized'
if run('native/sanitizer-emit', [compiler, 'build', fixtures / 'native.aeth', '-I', library,
                                '--emit-c', '-o', generated]):
    if run('native/sanitizer-build', [cc, '-std=gnu11', '-O1', '-g', '-fsanitize=address,undefined',
                                      '-fno-sanitize-recover=all', generated, '-o', binary]):
        run('native/sanitizer-run', [binary], execute=True)

print(f'{sum(item["passed"] for item in records)}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(1 if failed else 0)
