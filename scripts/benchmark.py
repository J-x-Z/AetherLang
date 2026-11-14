#!/usr/bin/env python3
"""Comparable baseline/current measurements on the same scalar workload.

Use C-only release compilers for --baseline and --current. Emission is timed
separately from clang. Identical host compiler flags then isolate generated-code
quality from the old driver's omission of native optimization flags.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
p = argparse.ArgumentParser()
p.add_argument('--baseline', type=Path, required=True)
p.add_argument('--current', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--samples', type=int, default=5)
a = p.parse_args()
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
    return dict(seconds=elapsed, rss_bytes=int(match[1]) if match else None)

report = {'source': str(source), 'host': os.uname().sysname + ' ' + os.uname().machine,
          'samples': a.samples, 'expected_stdout': '652496\n', 'versions': {}, 'results': {}}
for tool in ['clang', 'rustc']:
    report['versions'][tool] = subprocess.check_output([tool, '--version'], text=True).splitlines()[0]
for name, compiler in [('baseline', a.baseline.resolve()), ('current', a.current.resolve())]:
    cfile = a.work_dir / (name + '.c')
    args = [compiler, '--backend', 'c', '--emit-c', '-O', '0', '-o', cfile, source]
    timed(args) # one untimed cache warm-up, then identical measured samples
    emission = [timed(args) for _ in range(a.samples)]
    entry = dict(compiler_bytes=compiler.stat().st_size, c_bytes=cfile.stat().st_size,
                 emission_samples=emission,
                 emission_seconds_median=statistics.median(x['seconds'] for x in emission),
                 emission_rss_bytes_max=max((x['rss_bytes'] or 0) for x in emission), programs={})
    for opt in [0, 2]:
        binary = a.work_dir / f'{name}-O{opt}'
        built = timed(['clang', '-w', f'-O{opt}', cfile, '-o', binary])
        timed([binary], '652496\n')
        samples = [timed([binary], '652496\n') for _ in range(a.samples)]
        entry['programs'][f'O{opt}'] = dict(bytes=binary.stat().st_size, native_build=built, samples=samples,
                                          run_seconds_median=statistics.median(x['seconds'] for x in samples))
    report['results'][name] = entry
(a.work_dir / 'measurements.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report['results'], indent=2))
