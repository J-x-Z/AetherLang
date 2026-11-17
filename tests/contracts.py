#!/usr/bin/env python3
"""Executable requires/ensures, lexical result, pure predicates and every exit."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
p.add_argument('--levels', nargs='+', type=int, default=[0, 2, 3])
a = p.parse_args()
compiler = a.compiler.resolve()
work = a.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
root = Path(__file__).resolve().parent / 'regressions/contracts'
records = []


def run(label, command, expected=0, trap=False):
    command = list(map(str, command))
    r = subprocess.run(command, text=True, capture_output=True, env=env, timeout=30)
    records.append(dict(label=label, command=command, status=r.returncode, stdout=r.stdout, stderr=r.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    # __builtin_trap/llvm.trap must execute: neither ordinary nonzero return,
    # abort/assert, nor an accidental segmentation fault satisfies this oracle.
    accepted = {-signal.SIGILL, -signal.SIGTRAP} if trap else {expected}
    assert r.returncode in accepted, f'{label}: {r.returncode}\n{r.stdout}\n{r.stderr}'
    return r


for name in ['values', 'traps']:
    run(f'{name}/check', [compiler, 'check', root / f'{name}.aeth'])
    for backend in a.backends:
        for level in a.levels:
            exe = work / f'{name}-{backend}-O{level}'
            run(f'{name}/{backend}/O{level}/build', [compiler, root / f'{name}.aeth', '--backend', backend, f'-O{level}', '-o', exe])
            if name == 'values':
                r = run(f'{name}/{backend}/O{level}/run', [exe])
                assert not r.stdout and not r.stderr
            else:
                for case in range(8):
                    run(f'{name}/{backend}/O{level}/case{case}', [exe, case], trap=True)

# NDEBUG cannot disable language contracts.
if 'c' in a.backends:
    c = work / 'traps-ndebug.c'
    exe = work / 'traps-ndebug'
    run('NDEBUG/emit', [compiler, root / 'traps.aeth', '--emit-c', '-o', c])
    run('NDEBUG/build', ['clang', '-std=c11', '-DNDEBUG', '-O3', c, '-lm', '-o', exe])
    for case in range(8):
        run(f'NDEBUG/case{case}', [exe, case], trap=True)

negative = {
    'predicate_type': ('fn f()->i32[requires 1]{1}', 'E2100'),
    'result_in_requires': ('fn f()->i32[requires result==1]{1}', 'E2001'),
    'result_in_body': ('fn f()->i32[ensures result==1]{result}', 'E2001'),
    'direct_assignment': ('fn f(n:mut i32)[requires {n=2;true}]{}', 'E2200'),
    'external_read': ('fn f(p:*i32)[requires *p>0]{}', 'E2200'),
    'volatile_read': ('fn f(p:*volatile i32)[requires *p>0]{}', 'E2200'),
    'global_read': ('static mut X:i32=1;fn f()[requires X>0]{}', 'E2200'),
    'io_call': ('fn impure()->bool{puts("effect");true}fn f()[requires impure()]{}', 'E2200'),
    'allocation_call': ('fn f()[requires malloc(8)!=0 as *u8]{}', 'E2200'),
    'panic_call': ('fn f()->i32[ensures {assert(result>0);true}]{1}', 'E2200'),
    'volatile_helper': ('@volatile fn observe(x:i32)->bool{x>0}fn f(x:i32)[requires observe(x)]{}', 'E2200'),
    'write_call': ('fn impure(p:*i32)->bool{*p=1;true}fn f(p:*i32)[requires impure(p)]{}', 'E2200'),
    'read_method': ('struct S{x:i32}impl S{fn good(self:&Self)->bool{self.x>0}}fn f(s:S)[requires s.good()]{}', 'E2200'),
    'early_return': ('fn f()[requires {return;true}]{}', 'E2104'),
    'local_declaration': ('fn f()[requires {let x:i32=1;true}]{}', 'E2104'),
    'loop': ('fn f()[requires {loop{};true}]{}', 'E2104'),
    'unsafe': ('fn f()[requires unsafe{true}]{}', 'E2104'),
    'invariant': ('fn f()[invariant true]{}', 'E2104'),
}
for name, (source, code) in negative.items():
    path = work / f'{name}.aeth'
    path.write_text(source + '\nfn main()->i32{0}\n')
    r = run(f'{name}/check', [compiler, 'check', '--error-format', 'json', path], 1)
    diagnostic = json.loads(r.stderr)
    assert diagnostic['code'] == code and diagnostic.get('location'), (name, diagnostic)
    for backend in a.backends:
        run(f'{name}/{backend}/build', [compiler, path, '--backend', backend, '-o', work / f'{name}-{backend}'], 1)
print(f'{len(records)} contract checks passed; evidence: {work / "results.json"}')
