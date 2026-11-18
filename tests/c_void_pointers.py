#!/usr/bin/env python3
"""N05: C opaque pointers interoperate with stdio while Unit stays zero-sized."""
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
parser.add_argument('--levels', nargs='+', choices=['0', '2'], default=['0', '2'])
parser.add_argument('--clang', default=os.environ.get('CLANG', 'clang'))
parser.add_argument('--cases', nargs='+', choices=['file_handles', 'byte_file_handles', 'unit_sentinel', 'unit_values', 'zero_sized'],
                    default=['file_handles', 'byte_file_handles', 'unit_sentinel', 'unit_values', 'zero_sized'])
parser.add_argument('--sanitize', action='store_true', help='Enable ASan/UBSan for generated C')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
fixtures = root / 'tests/regressions/c_void_pointers'
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
metadata = {'compiler': str(compiler),
            'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
            'requested_backends': args.backends, 'sanitizers': args.sanitize}
records = []
failed = False
cases = {
    'file_handles': (fixtures / 'file_handles.aeth', b'void-pointers=ok\n'),
    'byte_file_handles': (root / 'tests/regressions/ffi_variadic.aeth', b''),
    'unit_sentinel': (fixtures / 'unit_sentinel.aeth', b'unit-sentinel=ok\n'),
    # Reuse the established semantic oracles for by-value Unit aggregates,
    # Result<(), E>, Vec<()>, empty structs and zero-length arrays.
    'unit_values': (root / 'tests/regressions/system/unit_values.aeth', b''),
    'zero_sized': (root / 'tests/regressions/generics/zero_sized.aeth', b''),
}


def record(label, command, expected_stdout=None):
    global failed
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, timeout=60)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(command, 124, error.stdout or b'', error.stderr or b'timed out')
    passed = result.returncode == 0
    if expected_stdout is not None:
        passed = passed and result.stdout == expected_stdout and result.stderr == b''
    item = dict(label=label, command=command, exit=result.returncode,
                stdout=result.stdout.decode('utf-8', errors='replace'),
                stderr=result.stderr.decode('utf-8', errors='replace'), passed=passed)
    if expected_stdout is not None:
        item['expected_stdout'] = expected_stdout.decode('utf-8')
    records.append(item)
    failed |= not passed
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(item['stderr'] or item['stdout'], flush=True)
    return passed


for backend in args.backends:
    for level in args.levels:
        for name in args.cases:
            source, output = cases[name]
            label = f'{name}/{backend}/O{level}'
            binary = work / f'{name}-{backend}-O{level}'
            if backend == 'c':
                generated = binary.with_suffix('.c')
                if not record(label + '/emit-c', [compiler, 'build', source, '--backend', 'c',
                                                 f'-O{level}', '--emit-c', '-o', generated]):
                    continue
                # Only the pointer diagnostic is promoted: unrelated generated
                # C warnings must not obscure this regression or be suppressed.
                command = [args.clang, '-std=gnu11', '-Werror=incompatible-pointer-types',
                           f'-O{level}', generated, '-o', binary, '-lm']
                if name == 'file_handles':
                    command += ['-include', fixtures / 'void_contract.h']
                if args.sanitize:
                    command += ['-fsanitize=address,undefined', '-fno-sanitize-recover=all',
                                '-fno-omit-frame-pointer']
                built = record(label + '/strict-clang', command)
            else:
                built = record(label + '/build', [compiler, 'build', source, '--backend', backend,
                                                 f'-O{level}', '-o', binary])
            if built:
                record(label + '/run', [binary], output)

print(f'{sum(item["passed"] for item in records)}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(1 if failed else 0)
