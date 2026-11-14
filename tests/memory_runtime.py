#!/usr/bin/env python3
"""Native atomics, volatile accesses and Aether-owned concurrent workloads."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

parser = argparse.ArgumentParser()
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--levels', nargs='+', choices=['0', '2'], default=['0', '2'])
parser.add_argument('--check-x86', action='store_true', help='compile x86 port I/O objects; never execute privileged instructions')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
source = root / 'tests/regressions/memory'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
env = {**os.environ, 'TMPDIR': str(work)}
records = []
failed = False


def run(command):
    try:
        return subprocess.run(list(map(str, command)), capture_output=True, text=True,
                              timeout=30, env=env)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(command, 124, '', 'timed out after 30 seconds')


def record(name, phase, result, valid, **extra):
    global failed
    records.append(dict(name=name, phase=phase, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, passed=valid, **extra))
    failed |= not valid
    print(f'{"PASS" if valid else "FAIL"} {phase} {name}')
    if not valid:
        print(result.stderr or result.stdout or f'exit {result.returncode}', file=sys.stderr)


for name, expected in [('load_release', 'order'), ('store_acquire', 'order'),
                       ('fence_relaxed', 'order'), ('cas_stronger_failure', 'order'),
                       ('runtime_order', 'compile-time'), ('wrong_pointer_width', 'Type mismatch')]:
    result = run([compiler, 'check', source / 'invalid' / f'{name}.aeth'])
    record(name, 'check-rejected', result, result.returncode != 0 and expected.lower() in result.stderr.lower())

fixture = work / 'pthreads.o'
result = run([os.environ.get('CC', 'cc'), '-std=c11', '-O2', '-pthread', '-c', source / 'pthreads.c', '-o', fixture])
record('pthread platform fixture', 'build', result, result.returncode == 0)
if result.returncode == 0:
    for backend in args.backends:
        for level in args.levels:
            for name in ['operations', 'threaded']:
                binary = work / f'{name}-{backend}-O{level}'
                command = [compiler, 'build', source / f'{name}.aeth', '--backend', backend,
                           f'-O{level}', '-o', binary]
                if name == 'threaded':
                    command += ['--link-object', fixture, '-l', 'pthread']
                result = run(command)
                if result.returncode:
                    record(name, 'build', result, False, backend=backend, level=level)
                    continue
                start = time.perf_counter()
                result = run([binary])
                duration = time.perf_counter() - start
                record(name, 'run', result, result.returncode == 0 and not result.stdout and not result.stderr,
                       backend=backend, level=level, elapsed_seconds=duration)
            if backend == 'llvm':
                ir = work / f'operations-O{level}.ll'
                result = run([compiler, 'build', source / 'operations.aeth', '--backend', 'llvm',
                              f'-O{level}', '--emit-llvm', '-o', ir])
                patterns = [r'load volatile i8\b', r'store volatile i8\b', r'load volatile i64\b',
                            r'store volatile i64\b', r'atomicrmw add\b', r'atomicrmw sub\b',
                            r'cmpxchg\b', r'load atomic[^\n]*\bacquire\b',
                            r'store atomic[^\n]*\brelease\b', r'cmpxchg[^\n]*\bacq_rel acquire\b',
                            r'fence seq_cst\b', r'fence acquire\b', r'fence release\b']
                text = ir.read_text() if result.returncode == 0 else ''
                missing = [pattern for pattern in patterns if not re.search(pattern, text)]
                record('volatile and ordering instructions', 'llvm-ir', result,
                       result.returncode == 0 and not missing, level=level, missing_patterns=missing)

if args.check_x86:
    target = 'x86_64-apple-darwin' if sys.platform == 'darwin' else 'x86_64-unknown-linux-gnu'
    for backend in args.backends:
        result = run([compiler, 'build', source / 'x86_ports.aeth', '--backend', backend,
                      '--target', target, '--emit-obj', '-O2', '-o', work / f'x86-ports-{backend}.o'])
        record('x86 port I/O (compile only)', 'cross-object', result, result.returncode == 0,
               backend=backend, target=target)

(work / 'results.json').write_text(json.dumps(records, indent=2) + '\n')
raise SystemExit(1 if failed else 0)
