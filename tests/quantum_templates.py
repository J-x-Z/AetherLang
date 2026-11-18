#!/usr/bin/env python3
"""Check parameterized circuits with analytic amplitudes and allocator failure injection."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

cli = argparse.ArgumentParser(description=__doc__)
cli.add_argument('--compiler', required=True, type=Path)
cli.add_argument('--work-dir', required=True, type=Path)
cli.add_argument('--backends', nargs='+', choices=['c','llvm'], default=['c','llvm'])
cli.add_argument('--metal', action='store_true', help='also execute bound circuits on a real macOS Metal device')
cli.add_argument('--bridge', type=Path, help='also run bound circuits through the real CUDA-Q SDK')
args = cli.parse_args()
if len(set(args.backends)) != len(args.backends):
    cli.error('each backend may be selected only once')
if args.metal and sys.platform != 'darwin':
    cli.error('--metal requires macOS and a Metal device')
repo = Path(__file__).resolve().parents[1]
fixtures = repo/'tests/regressions/quantum'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
env = {**os.environ, 'TMPDIR': str(work), 'UBSAN_OPTIONS': 'halt_on_error=1'}
records = []

def run(label, command, extra_env=None):
    p = subprocess.run(list(map(str, command)), cwd=work, env={**env, **(extra_env or {})}, text=True,
                       capture_output=True, timeout=60)
    records.append(dict(label=label, command=p.args, exit=p.returncode, stdout=p.stdout, stderr=p.stderr))
    (work/'results.json').write_text(json.dumps(records, indent=2))
    assert p.returncode == 0, records[-1]
    return p.stdout

source = fixtures/'template.aeth'
providers = [('reference', 'cpu', [], {})]
if args.bridge:
    providers.append(('cudaq', 'cudaq', ['--link-object', args.bridge.resolve()], {}))
if args.metal:
    loader = work/'loader.o'
    library = work/'libaether_compute_metal.dylib'
    run('build compute loader', [os.environ.get('CC','clang'), '-std=c11', '-O2', '-c',
                                 repo/'runtime/compute/loader.c', '-o', loader])
    run('build Metal provider', ['clang', '-O2', '-fobjc-arc', '-dynamiclib', repo/'runtime/metal/provider.m',
                                 repo/'runtime/metal/bridge.m', '-framework', 'Metal', '-framework', 'Foundation',
                                 '-o', library])
    providers.append(('metal', 'metal', ['--link-object', loader], {'AETHER_TEMPLATE_METAL_PROVIDER':str(library)}))
for backend in args.backends:
    for provider, fixture, link, extra in providers:
        name = provider+'-'+backend
        binary = work/name
        run(name+' build', [compiler, 'build', source, '--sysroot', repo, '--backend', backend,
                            '--module', 'template_eval='+str(fixtures/f'template_eval_{fixture}.aeth'),
                            '-O2', *link, '-o', binary])
        output = run(name+' analytic amplitudes and independent lifetimes', [binary], extra)
        assert output.count('amplitudes=8 max_component_error=') == 5, output
        if provider == 'metal':
            assert output.count('submitted=5 completed=5 remaining=0') == 5, output

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
