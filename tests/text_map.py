#!/usr/bin/env python3
"""Bounded bytes, strict UTF-8, owned byte-key maps and an actual file counter."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--only', nargs='+', choices=['native', 'injected', 'word_count', 'toml_consumer'])
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
fixtures = root / 'tests/regressions/text_map'
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
cc = env.get('CC', 'clang')
records = []
metadata = {'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
            'modules': {name: hashlib.sha256((root / 'stdlib' / (name + '.aeth')).read_bytes()).hexdigest()
                        for name in ['bytes', 'text', 'map', 'toml']}}

def run(label, command, expected_exit=0, expected=None):
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, timeout=90)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(command, 124, error.stdout or b'', error.stderr or b'timeout')
    passed = result.returncode == expected_exit and (expected is None or (result.stdout == expected and result.stderr == b''))
    records.append(dict(label=label, command=command, exit=result.returncode, expected_exit=expected_exit,
                        stdout=result.stdout.decode('utf-8', errors='replace'), stderr=result.stderr.decode('utf-8', errors='replace'),
                        stdout_hex=result.stdout.hex(), expected_hex=None if expected is None else expected.hex(), passed=passed))
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print((result.stderr or result.stdout).decode('utf-8', errors='replace') or f'exit={result.returncode}', flush=True)
    return passed

# Independent expected values use Python Unicode decoding and byte-key Counter.
# NUL stays in words; the Aether program explicitly defines ASCII whitespace.
content = ('red red 中 🦀 a\0b a\n' + ''.join(f'word{i % 200:03d} 中\t' for i in range(2500))).encode('utf-8')
input_file = work / '文本 binary input.txt'
input_file.write_bytes(content)
words = content.replace(b'\t', b' ').replace(b'\n', b' ').replace(b'\r', b' ').split(b' ')
counts = Counter(word for word in words if word)
expected = (f'bytes={len(content)} scalars={len(content.decode("utf-8"))} words={sum(counts.values())} '
            f'unique={len(counts)} squared={sum(n*n for n in counts.values())}\n').encode()
empty_file = work / 'empty.txt'; empty_file.write_bytes(b'')
invalid_file = work / 'invalid.txt'; invalid_file.write_bytes(b'valid prefix\xf0\x80\x80\x80')

def execute(label, binary, name):
    if name == 'word_count':
        run(label + '/run', [binary, input_file], expected=expected)
        run(label + '/empty', [binary, empty_file], expected=b'bytes=0 scalars=0 words=0 unique=0 squared=0\n')
        run(label + '/invalid-utf8', [binary, invalid_file], expected_exit=4, expected=b'')
    else:
        run(label + '/run', [binary], expected=b'')

selected = args.only or ['native', 'injected', 'word_count', 'toml_consumer']
allocator = work / 'failure_allocator.o'
ready = True
if 'injected' in selected:
    ready = run('failure-allocator/build', [cc, '-std=c11', '-O0', '-fno-builtin', '-c',
                                          fixtures / 'failure_allocator.c', '-o', allocator])
for name in selected:
    source = (root / 'tests/regressions/legacy_runtime' if name == 'toml_consumer' else fixtures) / f'{name}.aeth'
    if not run(name + '/check', [compiler, 'check', source, '-I', root / 'stdlib']):
        continue
    for backend in args.backends:
        for level in ['0', '2']:
            label = f'{name}/{backend}/O{level}'
            binary = work / f'{name}-{backend}-O{level}'
            command = [compiler, 'build', source, '-I', root / 'stdlib', '--backend', backend, f'-O{level}', '-o', binary]
            if name == 'injected':
                if not ready:
                    continue
                command += ['--link-object', allocator]
            if run(label + '/build', command):
                execute(label, binary, name)
    # Do not interpose malloc on sanitizer runtime allocations. Failure injection
    # is exercised separately with a tracked, bounded, deterministic allocator.
    if name != 'injected':
        generated = work / (name + '-sanitized.c')
        binary = work / (name + '-sanitized')
        if run(name + '/sanitizer-emit', [compiler, 'build', source, '-I', root / 'stdlib', '--emit-c', '-o', generated]):
            if run(name + '/sanitizer-build', [cc, '-std=gnu11', '-O0', '-g', '-fsanitize=address,undefined',
                                              '-fno-sanitize-recover=all', generated, '-o', binary]):
                execute(name + '/sanitizer', binary, name)
passed = sum(item['passed'] for item in records)
print(f'{passed}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(0 if passed == len(records) else 1)
