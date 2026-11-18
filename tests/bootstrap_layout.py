#!/usr/bin/env python3
"""Exercise Aether-computed layout against separate host C translation units."""
import argparse
import hashlib
import json
import os
import platform
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', required=True, type=Path)
parser.add_argument('--reference-compiler', type=Path)
parser.add_argument('--work-dir', required=True, type=Path)
args = parser.parse_args()
tool, work = args.compiler.resolve(), args.work_dir.resolve()
work.mkdir(exist_ok=True, parents=True)
sources = Path(__file__).resolve().parent / 'regressions'
cc = os.environ.get('CC', 'clang')
records = []
env = dict(os.environ, UBSAN_OPTIONS='halt_on_error=1', ASAN_OPTIONS='abort_on_error=1')


def run(command, expected=0, diagnostic=None):
    result = subprocess.run(list(map(str, command)), cwd=work, env=env,
                            capture_output=True, text=True, timeout=90)
    records.append(dict(command=result.args, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work/'results.json').write_text(json.dumps(dict(host=platform.platform(),
        sha256=hashlib.sha256(tool.read_bytes()).hexdigest(), commands=records), indent=2))
    assert result.returncode == expected, records[-1]
    if diagnostic:
        assert diagnostic in result.stdout + result.stderr, records[-1]


for name in ('aggregates/abi', 'aggregates/sysv', 'system/globals_unions',
             'system/union_payload_bytes', 'system/unit_values', 'bootstrap/layout/packed'):
    source = (sources/name).with_suffix('.aeth')
    oracle = source.with_suffix('.c')
    native_objects = []
    if oracle.exists():
        obj = work/(source.stem+'.o')
        run([cc, '-std=c11', '-O2', '-c', oracle, '-o', obj])
        native_objects = ['--link-object', obj]
    for backend in ('c', 'llvm'):
        for level in (0, 2, 3):
            executable = work/f'{source.stem}-{backend}-{level}'
            run([tool, 'build', source, '--backend', backend, f'-O{level}',
                 *native_objects, '-o', executable])
            run([executable])
        generated = work/(source.stem + ('.ll' if backend == 'llvm' else '.c'))
        run([tool, source, '--emit-'+backend, '-o', generated])
        if source.stem == 'packed' and backend == 'llvm' and platform.machine() == 'x86_64':
            line = next(line for line in generated.read_text().splitlines() if line.startswith('declare ') and '@flip(' in line)
            assert line.startswith('declare float @flip(float '), line
        executable = work/f'{source.stem}-sanitized-{backend}'
        run([cc, '-g', '-O1', '-fsanitize=address,undefined', '-fno-omit-frame-pointer',
             generated, *([oracle] if oracle.exists() else []),
             *(['-Wl,-no_fixup_chains'] if source.stem == 'packed' and platform.system() == 'Darwin' else []),
             '-o', executable])
        run([executable])

if args.reference_compiler:
    source = sources/'bootstrap/layout/packed.aeth'
    for backend in ('c', 'llvm'):
        executable = work/f'reference-packed-{backend}'
        run([args.reference_compiler.resolve(), 'build', source, '--backend', backend,
             '-O2', '--link-object', work/'packed.o', '-o', executable])
        run([executable])

for i, (code, message) in enumerate([
    ('@repr(bad) struct R{x:u8}', 'unknown record representation'),
    ('@repr(C) fn f(){}', 'repr requires a struct or union'),
    ('union U{}', 'union requires at least one field'),
    ('union U{a:u8,b:u16}fn f(){let u:U=U{};}', 'exactly one field'),
    ('union U{a:u8,b:u16}fn f(){let u:U=U{a:3,b:4};}', 'exactly one field'),
    ('union U{a:u8}fn f(x:U)->u8{return x.a;}', 'unsafe block required'),
    ('struct R{x:u8}fn f()->usize{return offset_of(R,y);}', 'unknown layout field'),
    ('fn f()->usize{return offset_of(u8,y);}', 'offset_of requires a record'),
]):
    source = work/f'invalid-{i}.aeth'
    source.write_text(code+'\n')
    run([tool, 'check', source], 1, message)
print(f'{len(records)} layout checks passed on {platform.system()} {platform.machine()}')
