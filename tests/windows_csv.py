#!/usr/bin/env python3
"""Validate the native Windows CSV CLI and binary input ownership."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', required=True, type=Path)
parser.add_argument('--cc', required=True, type=Path)
parser.add_argument('--sysroot', required=True, type=Path, help='installed Windows sysroot')
parser.add_argument('--work-dir', required=True, type=Path)
args = parser.parse_args()
if os.name != 'nt':
    parser.error('run on Windows with a native compiler and linker')
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler, cc, sysroot = args.compiler.resolve(), args.cc.resolve(), args.sysroot.resolve()
env = {**os.environ, 'PATH': str(cc.parent) + os.pathsep + os.environ['PATH']}
receipt = dict(host=platform.platform(), compiler=str(compiler),
               sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(), checks=[])

def run(name, command, *, stdin=None):
    with (stdin or Path(os.devnull)).open('rb') as stream:
        result = subprocess.run(list(map(str, command)), stdin=stream, capture_output=True,
                                cwd=work, env=env, timeout=60)
    record = dict(name=name, command=result.args, stdin=str(stdin) if stdin else None,
                  status=result.returncode, stdout=result.stdout.decode('utf-8', 'replace'),
                  stderr=result.stderr.decode('utf-8', 'replace'))
    receipt['checks'].append(record)
    (work/'results.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
    assert result.returncode == 0, record
    return record['stdout']

def build(source, name, backend):
    exe = work / f'{name}-{backend}.exe'
    run('build ' + exe.name, [compiler, 'build', source, '--cc', cc, '--sysroot', sysroot,
                              '--backend', backend, '-O2', '-o', exe])
    return exe

sample = work/'samples data.csv'
sample.write_bytes(b'note,"x, time",y\r\n"embedded \x1a,\r\nline",-2,0\r\n'
                   b'"escaped ""quote""",-1,1\r\nbase,0,5\r\nnext,1,5\r\nlast,2,9\r\nmissing,,7')
sentinel = work/'ctrl-z.bin'
sentinel.write_bytes(b'\x1a')
# Centered sums xx=10, yy=52 and xy=22 for the five complete pairs.
expected = dict(n=5, skipped=1, mean_x=0, mean_y=4, variance_x=2.5, variance_y=13,
                covariance=5.5, slope=11/5, intercept=4, r_squared=121/130)
for backend in ('c', 'llvm'):
    app = build(root/'examples/analysis/regress.aeth', 'regress', backend)
    file_fit = json.loads(run('fit from file', [app, sample, 'x, time', 'y']))
    pipe_fit = json.loads(run('fit from redirected stdin', [app, '-', 'x, time', 'y'], stdin=sample))
    assert file_fit == pipe_fit and file_fit.keys() == expected.keys(), (file_fit, pipe_fit)
    for key, value in expected.items():
        assert math.isclose(file_fit[key], value, rel_tol=1e-12, abs_tol=1e-12), (key, file_fit)
    mode = build(root/'tests/regressions/platform/windows_stdio_binary.aeth', 'fd-mode', backend)
    run('duplicate preserves stdin text mode', [mode], stdin=sentinel)
receipt['passed'] = True
(work/'results.json').write_text(json.dumps(receipt, indent=2), encoding='utf-8')
print(f"{len(receipt['checks'])} native Windows CSV build and execution checks passed")
