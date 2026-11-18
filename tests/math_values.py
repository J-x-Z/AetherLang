#!/usr/bin/env python3
"""Tensor view ownership, reverse AD, finite differences, training and allocation rollback."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
from ir_toolchain import emitted_source_compiler

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
root = Path(__file__).resolve().parent / 'regressions/math'
env = {**os.environ, 'TMPDIR': str(work)}
cc = os.environ.get('CC') or shutil.which('clang') or 'cc'
records = []

def run(label, command, environment=None):
    result = subprocess.run(list(map(str, command)), env=environment or env,
                            text=True, capture_output=True, timeout=60)
    records.append(dict(label=label, command=list(map(str, command)), status=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert result.returncode == 0, f'{label}: {result.returncode}\n{result.stdout}\n{result.stderr}'
    return result

source = root / 'tensor_autodiff.aeth'
run('math/check', [compiler, 'check', source])
outputs = []
for backend in args.backends:
    libm = work / f'hosted-libm-{backend}'
    run(f'libm/{backend}/build', [compiler, 'build', root / 'hosted_libm.aeth', '--backend', backend, '-O2', '-o', libm])
    run(f'libm/{backend}/run', [libm])
    for opt in [0, 2]:
        executable = work / f'math-{backend}-O{opt}'
        run(f'math/{backend}/O{opt}/build', [compiler, 'build', source, '--backend', backend, f'-O{opt}', '-o', executable])
        result = run(f'math/{backend}/O{opt}/run', [executable])
        outputs.append(result.stdout)
assert len(set(outputs)) == 1, outputs
for backend in args.backends:
    executable = work / f'compatibility-{backend}'
    run(f'compatibility/{backend}/build', [compiler, 'build', root / 'compatibility.aeth', '--backend', backend, '-O2', '-o', executable])
    run(f'compatibility/{backend}/run', [executable])
sanitized_source = work / 'math-sanitized.c'
run('math/emit-c', [compiler, 'build', source, '--emit-c', '-o', sanitized_source])
sanitized = work / 'math-sanitized'
run('math/sanitizer/build', [cc, '-std=c11', '-O1', '-g', '-fsanitize=address,undefined', sanitized_source, '-lm', '-o', sanitized])
run('math/sanitizer/run', [sanitized], {**env, 'UBSAN_OPTIONS': 'halt_on_error=1'})

failure_source = root / 'allocation_failure.aeth'
run('allocation/check', [compiler, 'check', failure_source])
# Redirect only the emitted program's allocator symbols. The shim counts live
# allocations and injects NULL at each boundary; numerical kernels stay Aether.
for backend in args.backends:
    extension, flag = ('c', '--emit-c') if backend == 'c' else ('ll', '--emit-llvm')
    emitted = work / f'allocation-{backend}.{extension}'
    run(f'allocation/{backend}/emit', [compiler, 'build', failure_source, flag, '-O0', '-o', emitted])
    text = emitted.read_text()
    for name in ['malloc', 'realloc', 'free']:
        pattern = rf'\b{name}\b' if backend == 'c' else rf'@{name}\b'
        replacement = f'test_{name}' if backend == 'c' else f'@test_{name}'
        text = re.sub(pattern, replacement, text)
    if backend == 'c':
        text = '#include <stdint.h>\nvoid *test_malloc(uint64_t); void *test_realloc(void *, uint64_t); void test_free(void *);\n' + text
    emitted.write_text(text)
    executable = work / f'allocation-{backend}'
    run(f'allocation/{backend}/build', [*emitted_source_compiler(backend, cc), '-O1', '-g', '-fsanitize=address,undefined', emitted, root / 'allocation_shim.c', '-lm', '-o', executable])
    run(f'allocation/{backend}/run', [executable], {**env, 'UBSAN_OPTIONS': 'halt_on_error=1'})
print(f'{len(records)} math checks passed')
print(outputs[0], end='')
