#!/usr/bin/env python3
"""Check parameterized circuits with analytic amplitudes and allocator failure injection."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess

cli = argparse.ArgumentParser(description=__doc__)
cli.add_argument('--compiler', required=True, type=Path)
cli.add_argument('--work-dir', required=True, type=Path)
cli.add_argument('--bridge', type=Path, help='also run bound circuits through the real CUDA-Q SDK')
args = cli.parse_args()
repo = Path(__file__).resolve().parents[1]
fixtures = repo/'tests/regressions/quantum'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
env = {**os.environ, 'TMPDIR': str(work), 'UBSAN_OPTIONS': 'halt_on_error=1'}
records = []

def run(label, command):
    p = subprocess.run(list(map(str, command)), cwd=work, env=env, text=True,
                       capture_output=True, timeout=60)
    records.append(dict(label=label, command=p.args, exit=p.returncode, stdout=p.stdout, stderr=p.stderr))
    (work/'results.json').write_text(json.dumps(records, indent=2))
    assert p.returncode == 0, records[-1]

source = fixtures/'template.aeth'
for backend in ('c', 'llvm'):
    providers = [('reference', ['--module', 'template_eval='+str(fixtures/'template_eval_cpu.aeth')])]
    if args.bridge:
        providers.append(('cudaq', ['--module', 'template_eval='+str(fixtures/'template_eval_cudaq.aeth'),
                                    '--link-object', args.bridge.resolve()]))
    for provider, link in providers:
        name = provider+'-'+backend
        binary = work/name
        run(name+' build', [compiler, 'build', source, '--sysroot', repo, '--backend', backend,
                            '-O2', *link, '-o', binary])
        run(name+' analytic amplitudes and independent lifetimes', [binary])

    flag, ext = ('--emit-c', 'c') if backend == 'c' else ('--emit-llvm', 'll')
    gen = work/('faults.'+ext)
    run('emit fault test '+backend, [compiler, 'build', fixtures/'template_alloc.aeth', '--sysroot', repo,
                                     flag, '-O0', '-o', gen])
    # Same scoped allocator instrumentation as quantum_values.py.
    code = gen.read_text()
    for name in ('malloc', 'realloc', 'free'):
        pattern = rf'\b{name}\b' if backend == 'c' else rf'@{name}\b'
        code = re.sub(pattern, ('test_' if backend == 'c' else '@test_')+name, code)
    if backend == 'c':
        code = ('#include <stdint.h>\nvoid *test_malloc(uint64_t); void *test_realloc(void *,uint64_t);'
                ' void test_free(void *);\n' + code)
    gen.write_text(code)
    binary = work/('allocation-'+backend)
    run('link sanitizer '+backend, [os.environ.get('CC','clang'), '-g', '-O1', '-fsanitize=address,undefined',
                                    gen, repo/'tests/regressions/math/allocation_shim.c', '-lm', '-o', binary])
    run('allocation failure and retry '+backend, [binary])
print(f'{len(records)} template checks passed')
