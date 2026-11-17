#!/usr/bin/env python3
"""Concrete interface contracts and lexical module method visibility."""
import argparse
import json
import os
from pathlib import Path
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
root = Path(__file__).resolve().parent
records = []

def run(label, command, expected=0):
    command = list(map(str, command))
    r = subprocess.run(command, text=True, capture_output=True, env=env, timeout=30)
    records.append(dict(label=label, command=command, status=r.returncode, stdout=r.stdout, stderr=r.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert r.returncode == expected, f'{label}: {r.returncode}\n{r.stdout}\n{r.stderr}'
    return r

positive = root / 'regressions/interfaces/positive.aeth'
run('positive/check', [compiler, 'check', positive])
for backend in a.backends:
    for level in a.levels:
        executable = work / f'positive-{backend}-O{level}'
        run(f'positive/{backend}/O{level}/build', [compiler, positive, '--backend', backend, f'-O{level}', '-o', executable])
        r = run(f'positive/{backend}/O{level}/run', [executable])
        assert not r.stdout and not r.stderr

(work / 'secret.aeth').write_text('pub struct S{x:i32}impl S{fn hidden(self:&Self)->i32{7}fn private_static()->i32{8}}pub fn make()->S{S{x:7}}')
negative = {
    'private_dot': ('use secret::{S,make};fn main()->i32{let s:S=make();s.hidden()-7}', 'E2004'),
    'private_path': ('use secret::{S,make};fn main()->i32{let s:S=make();S::hidden(&s)-7}', 'E2004'),
    'private_static': ('use secret::S;fn main()->i32{S::private_static()-8}', 'E2004'),
    'inline_sibling': ('mod secret{pub struct S{x:i32}impl S{fn hidden(self:&Self)->i32{7}}pub fn make()->S{S{x:7}}}use secret::S;fn main()->i32{let s:S=secret::make();s.hidden()-7}', 'E2004'),
    'unknown_declaration_type': ('interface I{fn get()->Missing;}fn main()->i32{0}', 'E2002'),
    'unknown_interface': ('struct S{x:i32}impl Missing for S{}fn main()->i32{0}', 'E2002'),
    'unknown_target': ('interface I{}impl I for Missing{}fn main()->i32{0}', 'E2002'),
    'missing_method': ('interface I{fn get()->i32;}struct S{x:i32}impl I for S{}fn main()->i32{0}', 'E2104'),
    'extra_method': ('interface I{}struct S{x:i32}impl I for S{fn extra(){}}fn main()->i32{0}', 'E2104'),
    'wrong_return': ('interface I{fn get()->i32;}struct S{x:i32}impl I for S{fn get()->i64{1}}fn main()->i32{0}', 'E2100'),
    'wrong_receiver': ('interface I{fn get(self:&mut Self)->i32;}struct S{x:i32}impl I for S{fn get(self:&Self)->i32{1}}fn main()->i32{0}', 'E2100'),
    'duplicate_impl': ('interface I{}struct S{x:i32}impl I for S{}impl I for S{}fn main()->i32{0}', 'E2003'),
    'interface_effect': ('interface I{fn set(self:&mut Self) pure;}struct S{x:i32}impl I for S{fn set(self:&mut Self){self.x=7;}}fn main()->i32{0}', 'E2200'),
    'unsupported_generic_interface': ('trait I<T>{fn get(value:T)->T;}fn main()->i32{0}', 'E2104'),
}
for name, (source, code) in negative.items():
    path = work / f'{name}.aeth'
    path.write_text(source)
    r = run(f'{name}/check', [compiler, 'check', '--error-format', 'json', path], 1)
    diagnostic = json.loads(r.stderr)
    assert diagnostic['code'] == code and diagnostic.get('location'), (name, diagnostic)
    for backend in a.backends:
        run(f'{name}/{backend}/build', [compiler, path, '--backend', backend, '-o', work / f'{name}-{backend}'], 1)
print(f'{len(records)} checks passed; evidence: {work / "results.json"}')
