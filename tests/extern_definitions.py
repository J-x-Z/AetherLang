#!/usr/bin/env python3
"""Compatible extern C/unique no_mangle definitions, native callbacks and conflicts."""
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
parser.add_argument('--levels', nargs='+', choices=['0', '2', '3'], default=['0', '2', '3'])
args = parser.parse_args()
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
fixtures = Path(__file__).resolve().parent / 'regressions/extern_definitions'
env = {**os.environ, 'TMPDIR': str(work)}
metadata = {'compiler': str(compiler),
            'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest()}
records = []
failed = False


def record(label, command, validate, expected_stdout=None):
    global failed
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, timeout=30)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(command, 124, error.stdout or b'', error.stderr or b'timed out')
    try:
        passed = bool(validate(result))
        reason = ''
    except (AssertionError, ValueError, KeyError, TypeError) as error:
        passed, reason = False, str(error)
    item = dict(label=label, command=command, exit=result.returncode,
                stdout=result.stdout.decode('utf-8', errors='replace'),
                stderr=result.stderr.decode('utf-8', errors='replace'), passed=passed)
    if expected_stdout is not None:
        item['expected_stdout'] = expected_stdout.decode('utf-8')
    if reason:
        item['validation_error'] = reason
    records.append(item)
    failed |= not passed
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(item['stderr'] or reason or item['stdout'], flush=True)
    return passed


def successful(result):
    return result.returncode == 0


def rejected(result, expected_code=None):
    assert result.returncode == 1, f'expected diagnostic exit 1, got {result.returncode}'
    diagnostic = json.loads(result.stderr)
    code = diagnostic['code']
    # These fixtures use supported syntax. Parse/link errors do not prove the
    # declaration merge rules are enforced by the frontend.
    assert code.startswith('E2'), diagnostic
    if expected_code is not None:
        assert code == expected_code, diagnostic
    return True


native = work / 'callback.o'
native_built = record('native/callback-object', [env.get('CC', 'cc'), '-c', fixtures / 'callback.c',
                                               '-o', native], successful)
expected = b'extern-merge=ok\n'
for filename in ['decl_first.aeth', 'def_first.aeth', 'cross/decl_first.aeth', 'cross/def_first.aeth']:
    source = fixtures / filename
    name = filename.removesuffix('.aeth').replace('/', '-')
    record(name + '/check', [compiler, 'check', source], successful)
    if native_built:
        for backend in args.backends:
            for level in args.levels:
                label = f'{name}/{backend}/O{level}'
                binary = work / f'{name}-{backend}-O{level}'
                if record(label + '/build', [compiler, 'build', source, '--backend', backend,
                                             f'-O{level}', '--link-object', native, '-o', binary], successful):
                    record(label + '/run', [binary], lambda result: result.returncode == 0
                           and result.stdout == expected and result.stderr == b'', expected)

for source in sorted((fixtures / 'invalid').glob('*.aeth')):
    code = 'E2200' if source.stem == 'pure_claim_hides_body' else None
    validate = lambda result: rejected(result, code)
    record(source.stem + '/check', [compiler, 'check', source, '--error-format', 'json'], validate)
    for backend in args.backends:
        record(source.stem + '/' + backend + '/build', [compiler, 'build', source, '--backend', backend,
                                                       '--error-format', 'json', '-o', work / 'invalid-output'], validate)

print(f'{sum(item["passed"] for item in records)}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(1 if failed else 0)
