#!/usr/bin/env python3
"""Bidirectional aggregate C ABI: native execution or explicitly cross-object only.

Includes odd-sized/packed values, INTEGER/SSE unions, mixed register classes,
whole-aggregate register rollback, and the hidden sret register. Each native
Aether optimization level is linked against independently compiled C O0/O2/O3.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', required=True, type=Path)
p.add_argument('--work-dir', required=True, type=Path)
p.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
p.add_argument('--target', help='Cross-object verification only; never reports native execution')
a = p.parse_args()
repo = Path(__file__).resolve().parent.parent
fixtures = repo / 'tests/regressions/aggregates'
work = a.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = a.compiler.resolve()
cc = os.environ.get('CC', 'clang')
env = {**os.environ, 'TMPDIR': str(work)}
records = []
mode = 'cross-object-only' if a.target else 'native'
target = ['--target', a.target] if a.target else []
cc_target = ['-target', a.target, '-ffreestanding'] if a.target else []

def run(label, cmd):
    r = subprocess.run(list(map(str, cmd)), env=env, capture_output=True, text=True, timeout=60)
    records.append(dict(label=label, mode=mode, command=list(map(str, cmd)), status=r.returncode,
                        stdout=r.stdout, stderr=r.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert r.returncode == 0, f'{label}: {r.returncode}\n{r.stdout}\n{r.stderr}'
    return r

host_supported = ((platform.system() == 'Darwin' and platform.machine() in ('arm64', 'aarch64')) or
                  (platform.system() in ('Darwin', 'Linux') and platform.machine() in ('x86_64', 'AMD64')))
if not a.target and 'llvm' in a.backends and not host_supported:
    p.error('native aggregate LLVM ABI is not supported on this host; select --backends c or --target for cross-object verification')

for case in ['abi', 'sysv']:
    source = fixtures / f'{case}.aeth'
    for copt in [0, 2, 3]:
        helper = work / f'{case}-native-O{copt}.o'
        run(f'{case}/C oracle/O{copt}/object', [cc, '-std=c11', *cc_target, f'-O{copt}', '-c', fixtures / f'{case}.c', '-o', helper])
    for backend in a.backends:
        for opt in [0, 2, 3]:
            if a.target:
                obj = work / f'{case}-{backend}-O{opt}.o'
                run(f'{case}/{backend}/O{opt}/cross object', [compiler, source, '--sysroot', repo,
                    '--backend', backend, *target, f'-O{opt}', '--emit-obj', '-o', obj])
            else:
                for copt in [0, 2, 3]:
                    binary = work / f'{case}-{backend}-O{opt}-oracleO{copt}'
                    run(f'{case}/{backend}/O{opt}/oracleO{copt}/build', [compiler, 'build', source,
                        '--sysroot', repo, '--backend', backend, f'-O{opt}', '--link-object',
                        work / f'{case}-native-O{copt}.o', '-o', binary])
                    r = run(f'{case}/{backend}/O{opt}/oracleO{copt}/native run', [binary])
                    assert not r.stdout and not r.stderr

sysv = (a.target and a.target.startswith('x86_64-') and ('-linux-' in a.target or '-apple-' in a.target)) or (
    not a.target and platform.machine() in ('x86_64', 'AMD64'))
if 'llvm' in a.backends and sysv:
    ir = work / 'sysv-O0.ll'
    run('SysV explicit register and stack signature', [compiler, fixtures / 'sysv.aeth', '--sysroot', repo,
        '--backend', 'llvm', *target, '-O0', '--emit-llvm', '-o', ir])
    text = ir.read_text()
    def signature(name):
        match = re.search(r'^define[^\n]*@aeth_' + name + r'\([^\n]+', text, re.M)
        assert match, name
        return match.group()
    for name in ['spill5', 'spill_sse', 'spill_gp', 'result_spill']:
        assert 'byval(' in signature(name) and 'align 8' in signature(name), signature(name)
    assert 'sret(' in signature('result_spill'), signature('result_spill')
    assert re.search(r'@aeth_mixed\(double[^,]*, i64', signature('mixed')), signature('mixed')
    assert re.search(r'@aeth_merge\(i64', signature('merge')), signature('merge')
    records.append(dict(label='SysV signature assertions', mode=mode, status=0))
    (work / 'results.json').write_text(json.dumps(records, indent=2))

print(f'{len(records)} checks passed ({mode}); evidence: {work / "results.json"}')
