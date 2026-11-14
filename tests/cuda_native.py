#!/usr/bin/env python3
"""Build and run real CUDA/cuBLAS GEMMs; exit 77 means GPU validation skipped."""
import argparse
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
parser.add_argument('--cuda-lib-dir', type=Path, help='Directory containing CUDA runtime and cuBLAS shared libraries')
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
source = Path(__file__).resolve().parent / 'regressions/cuda/gpu_matrix.aeth'
env = {**os.environ, 'TMPDIR': str(work)}
link = ['--link-library', 'cublas', '--link-library', 'cudart']
if args.cuda_lib_dir:
    directory = args.cuda_lib_dir.resolve()
    if not directory.is_dir():
        parser.error(f'CUDA library directory does not exist: {directory}')
    link = []
    for name in ['cublas', 'cudart']:
        candidates = [directory / f'lib{name}.so', directory / f'lib{name}.dylib']
        library = next((path for path in candidates if path.is_file()), None)
        if library is None:
            # Wheel installations may only contain a versioned SONAME file.
            versioned = [path for path in directory.glob(f'lib{name}.so.*') if path.is_file()]
            versioned.sort(key=lambda path: tuple(int(part) for part in re.findall(r'\d+', path.name)))
            library = versioned[-1] if versioned else None
        if library is None:
            parser.error(f'lib{name} shared library not found in {directory}')
        link += ['--link-object', str(library)]
    for variable in ['LD_LIBRARY_PATH', 'DYLD_LIBRARY_PATH']:
        env[variable] = str(directory) + (os.pathsep + env[variable] if env.get(variable) else '')

evidence = dict(compiler=str(compiler), source=str(source), backends=args.backends,
                cuda_lib_dir=str(args.cuda_lib_dir.resolve()) if args.cuda_lib_dir else None,
                checks=[])


def save():
    (work / 'results.json').write_text(json.dumps(evidence, indent=2) + '\n')


def run(backend, level, phase, command):
    command = list(map(str, command))
    started = time.monotonic()
    try:
        result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=120)
        code, stdout, stderr = result.returncode, result.stdout, result.stderr
    except subprocess.TimeoutExpired as error:
        def decoded(value):
            return value.decode(errors='replace') if isinstance(value, bytes) else value or ''
        code, stdout, stderr = 124, decoded(error.stdout), decoded(error.stderr) + '\nTIMEOUT'
    except OSError as error:
        code, stdout, stderr = 127, '', str(error)
    status = 'passed' if code == 0 else 'skipped' if phase == 'run' and code == 77 else 'failed'
    if phase == 'run' and code == 0:
        match = re.search(r'cuda-gemm m=17 n=13 k=19 sgemm-max=(\S+) dgemm-max=(\S+)', stdout)
        try:
            errors = [float(value) for value in match.groups()] if match else []
            valid = len(errors) == 2 and all(math.isfinite(value) and 0 <= value <= tolerance
                                            for value, tolerance in zip(errors, [1e-5, 1e-10]))
        except ValueError:
            valid = False
        if not valid:
            status = 'failed'
            stderr += '\nMissing or invalid completed GPU GEMM result'
    record = dict(backend=backend, optimization=level, phase=phase, command=command,
                  exit=code, status=status, elapsed_seconds=time.monotonic() - started,
                  stdout=stdout, stderr=stderr)
    evidence['checks'].append(record)
    save()
    label = 'GPU' if phase == 'run' else 'build'
    print(f'{status.upper()} {label} {backend}/O{level} (exit {code})', flush=True)
    return status


for backend in args.backends:
    for level in [0, 2]:
        binary = work / f'cuda-gemm-{backend}-O{level}'
        status = run(backend, level, 'build', [compiler, 'build', source, '--backend', backend,
                                             f'-O{level}', *link, '-o', binary])
        if status == 'passed':
            run(backend, level, 'run', [binary])

failed = sum(record['status'] == 'failed' for record in evidence['checks'])
passed = sum(record['phase'] == 'run' and record['status'] == 'passed' for record in evidence['checks'])
skipped = sum(record['status'] == 'skipped' for record in evidence['checks'])
evidence['summary'] = dict(gpu_passed=passed, gpu_skipped=skipped, failed_checks=failed,
                           status='failed' if failed else 'skipped' if skipped else 'passed')
save()
if failed:
    print(f'GPU validation FAILED: {failed} failed checks, {passed} passed GPU programs, {skipped} skipped')
    raise SystemExit(1)
if skipped:
    print(f'GPU validation SKIPPED or incomplete: {passed} passed GPU programs, {skipped} missing-device skips')
    raise SystemExit(77)
print(f'{passed} native CUDA GPU programs passed')
