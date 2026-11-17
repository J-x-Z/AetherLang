#!/usr/bin/env python3
"""Comparable baseline/current measurements on the same scalar workload.

Use matching release builds for --baseline and --current. Emission is timed
separately from clang. --backend llvm handles the baseline object-only driver
and the upgraded explicit --emit-obj interface.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
p = argparse.ArgumentParser()
p.add_argument('--baseline', type=Path, required=True)
p.add_argument('--current', type=Path)
p.add_argument('--only', choices=['baseline', 'current', 'both'], default='both',
               help='Measure releases in separate phases with identical settings')
p.add_argument('--target', default='native', help='Shared deployment/ABI target')
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--samples', type=int, default=5)
p.add_argument('--backend', choices=['c', 'llvm'], default='c')
a = p.parse_args()
if a.only != 'baseline' and a.current is None:
    p.error('--current is required unless --only baseline')
if a.samples < 1:
    p.error('--samples must be positive')
a.work_dir.mkdir(parents=True, exist_ok=True)
a.work_dir = a.work_dir.resolve()
source = Path(__file__).resolve().parents[1] / 'examples/upgrade/benchmark.aeth'
env = dict(os.environ, TMPDIR=str(a.work_dir))

def timed(command, expected=None):
    full = list(map(str, command))
    if Path('/usr/bin/time').is_file() and os.uname().sysname == 'Darwin':
        full = ['/usr/bin/time', '-l'] + full
    start = time.perf_counter()
    r = subprocess.run(full, capture_output=True, text=True, env=env, timeout=60)
    elapsed = time.perf_counter() - start
    if r.returncode: raise RuntimeError(f'{command}\n{r.stdout}\n{r.stderr}')
    if expected is not None: assert r.stdout == expected, r.stdout
    match = re.search(r'(\d+)\s+maximum resident set size', r.stderr)
    return dict(command=list(map(str, command)), seconds=elapsed,
                rss_bytes=int(match[1]) if match else None, stderr=r.stderr)

report = {'source': str(source), 'host': os.uname().sysname + ' ' + os.uname().machine,
          'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
          'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
          'target': a.target,
          'environment': {key: env.get(key) for key in ['SDKROOT', 'MACOSX_DEPLOYMENT_TARGET', 'DEVELOPER_DIR', 'PATH']},
          'backend': a.backend, 'samples': a.samples, 'expected_stdout': '652496\n', 'versions': {}, 'results': {}}
for tool in ['clang']:
    report['versions'][tool] = subprocess.check_output([tool, '--version'], text=True).splitlines()[0]
if a.only == 'current':
    previous = a.work_dir / 'measurements.json'
    if not previous.is_file(): p.error('--only current requires previous baseline measurements in --work-dir')
    baseline = json.loads(previous.read_text())
    for key in ['source_sha256', 'script_sha256', 'host', 'backend', 'samples', 'target', 'environment', 'versions']:
        if baseline.get(key) != report[key]: p.error(f'baseline/current measurement settings differ: {key}')
    if baseline['results']['baseline']['compiler_sha256'] != hashlib.sha256(a.baseline.resolve().read_bytes()).hexdigest():
        p.error('baseline compiler has changed since measurement')
    report['results']['baseline'] = baseline['results']['baseline']
compilers = [('baseline', a.baseline.resolve())] if a.only == 'baseline' else [('current', a.current.resolve())] if a.only == 'current' else [('baseline', a.baseline.resolve()), ('current', a.current.resolve())]
for name, compiler in compilers:
    cfile = a.work_dir / (name + ('.c' if a.backend == 'c' else '.o'))
    if a.backend == 'c':
        mode = ['--emit-c']
    else:
        # Historical seeds emit an object by default. Newer baselines, just
        # like the current compiler, link an executable unless asked for one.
        help_text = subprocess.check_output([compiler, '--help'], text=True, env=env)
        mode = ['--emit-obj'] if '--emit-obj' in help_text else []
    target_args = [] if a.target == 'native' else ['--target', a.target]
    args = [compiler, '--backend', a.backend, *target_args, *mode, '-O', '0', '-o', cfile, source]
    timed(args) # one untimed cache warm-up, then identical measured samples
    emission = [timed(args) for _ in range(a.samples)]
    measured_rss = [x['rss_bytes'] for x in emission if x['rss_bytes'] is not None]
    entry = dict(compiler=str(compiler), compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                 compiler_bytes=compiler.stat().st_size, emitted_bytes=cfile.stat().st_size,
                 emission_samples=emission,
                 emission_seconds_median=statistics.median(x['seconds'] for x in emission),
                 emission_rss_bytes_max=max(measured_rss) if measured_rss else None, programs={})
    for opt in [0, 2]:
        binary = a.work_dir / f'{name}-O{opt}'
        if a.backend == 'llvm':
            timed([compiler, '--backend', 'llvm', *target_args, *mode, '-O', str(opt), '-o', cfile, source])
        native_target = [] if a.target == 'native' else [f'--target={a.target}']
        built = timed(['clang', *native_target, f'-O{opt}', cfile, '-o', binary])
        timed([binary], '652496\n')
        samples = [timed([binary], '652496\n') for _ in range(a.samples)]
        entry['programs'][f'O{opt}'] = dict(bytes=binary.stat().st_size, native_build=built, samples=samples,
                                          run_seconds_median=statistics.median(x['seconds'] for x in samples))
    report['results'][name] = entry
    (a.work_dir / f'measurements.{name}.json').write_text(json.dumps({**report, 'results': {name: entry}}, indent=2) + '\n')
(a.work_dir / 'measurements.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report['results'], indent=2))
