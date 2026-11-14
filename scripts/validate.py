#!/usr/bin/env python3
"""Build and execute the upgrade workloads; every assertion is a semantic oracle.

Outputs live under --work-dir, never next to sources. The supplied compiler must
already be built. No downloads or recursive build invocations.
"""
import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--backends', nargs='+', default=['llvm', 'c'])
p.add_argument('--levels', nargs='+', type=int, default=[0, 2, 3])
a = p.parse_args()
a.compiler = a.compiler.resolve()
a.work_dir.mkdir(parents=True, exist_ok=True)
a.work_dir = a.work_dir.resolve()
env = dict(os.environ, TMPDIR=str(a.work_dir))
records = []

def command(args, **kw):
    start = time.perf_counter()
    result = subprocess.run(list(map(str, args)), capture_output=True, text=True, timeout=60, env=env, **kw)
    elapsed = time.perf_counter() - start
    if result.returncode:
        raise AssertionError(f'{args}: exit {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result, elapsed

def build(source, backend, level):
    binary = a.work_dir / f'{source.name}-{backend}-O{level}'
    result, elapsed = command([a.compiler, 'build', source, '--backend', backend, '-O', level, '-o', binary])
    return binary, elapsed

def oracle_compute():
    state = 123456789
    values = []
    for _ in range(32768):
        state ^= (state << 13) & ((1 << 64) - 1)
        state ^= state >> 7
        state ^= (state << 17) & ((1 << 64) - 1)
        values.append(state & 1048575)
    return sum(values), min(values), max(values)

expected = oracle_compute()
for backend in a.backends:
    for level in a.levels:
        binary, compiled = build(ROOT / 'examples/upgrade/compute.aeth', backend, level)
        result, runtime = command([binary])
        fields = result.stdout.strip().split()
        assert fields[0] == 'compute' and tuple(map(int, fields[1:4])) == expected, result.stdout
        assert abs(float(fields[4]) - 33332.833335) < 0.000001, result.stdout
        records.append(dict(program='compute.aeth', backend=backend, optimization=level,
                            compile_seconds=compiled, run_seconds=runtime, bytes=binary.stat().st_size,
                            stdout=result.stdout.strip()))

# Keep a service alive across separate input events and check each flushed reply.
def service(binary, journal):
    proc = subprocess.Popen([str(binary), str(journal)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, env=env, bufsize=0)
    sel = selectors.DefaultSelector()
    sel.register(proc.stdout, selectors.EVENT_READ)
    def line(expected):
        actual = bytearray()
        deadline = time.monotonic() + 10
        while not actual.endswith(b'\n'):
            assert time.monotonic() < deadline and sel.select(1), ('no service response', expected)
            byte = proc.stdout.read(1)
            assert byte, (expected, bytes(actual), proc.poll())
            actual.extend(byte)
        assert actual.decode() == expected + '\n', (expected, actual)
    try:
        line('READY')
        commands = [('add 12', 'OK 12'), ('add -5', 'OK 7'), ('add 1000000001', 'ERR number'),
                    ('add 1junk', 'ERR number'), ('wat', 'ERR command'), ('get', 'VALUE 7 2'),
                    ('x' * 5000, 'ERR too-long'), ('reset', 'RESET')]
        for request, reply in commands:
            proc.stdin.write((request + '\n').encode()); proc.stdin.flush(); line(reply)
        # 2000 events cover repeated parsing/allocation-free processing and flushing.
        for i in range(2000):
            proc.stdin.write(b'add 1\n'); proc.stdin.flush(); line(f'OK {i + 1}')
        proc.stdin.write(b'quit\n'); proc.stdin.flush(); line('BYE 2000 2000')
        proc.stdin.close()
        assert proc.wait(timeout=10) == 0, proc.stderr.read()
        assert journal.read_bytes() == b'add 12\nadd -5\n' + b'add 1\n' * 2000
    finally:
        sel.close()
        if proc.poll() is None: proc.kill(); proc.wait()

for backend in a.backends:
    for level in [0, 2]:
        binary, compiled = build(ROOT / 'examples/upgrade/accumulator.aeth', backend, level)
        journal = a.work_dir / f'journal-{backend}-{level}.txt'
        journal.unlink(missing_ok=True)
        start = time.perf_counter(); service(binary, journal); elapsed = time.perf_counter() - start
        eof, _ = command([binary, a.work_dir / f'eof-{backend}-{level}.txt'], input='add 2\nget\n')
        assert eof.stdout == 'READY\nOK 2\nVALUE 2 1\n'
        bad = subprocess.run([str(binary), str(a.work_dir / 'missing-dir' / 'journal')], capture_output=True, env=env, timeout=10)
        assert bad.returncode == 4
        records.append(dict(program='accumulator.aeth', backend=backend, optimization=level,
                            compile_seconds=compiled, event_seconds=elapsed, events=2008,
                            bytes=binary.stat().st_size))

# Script is an input language to the same checked pipeline, including both backends.
script_outputs = {
    'compute': '338350\n25\n',
    'branches': 'quoted: "Aether"\\path\n',
    'float_math': '3.25\n',
}
for name, expected_output in script_outputs.items():
    source = ROOT / 'tests/regressions/script' / (name + '.ath')
    for backend in a.backends:
        for level in [0, 2]:
            binary, compiled = build(source, backend, level)
            result, runtime = command([binary])
            assert result.stdout == expected_output, (source.name, result.stdout)
            records.append(dict(program=source.name, backend=backend, optimization=level,
                                compile_seconds=compiled, run_seconds=runtime, bytes=binary.stat().st_size))
for name in ['uninitialized', 'bad_indent']:
    result = subprocess.run([str(a.compiler), 'check', str(ROOT / 'tests/regressions/script' / (name + '.ath'))],
                            capture_output=True, env=env, timeout=10)
    assert result.returncode != 0, name

(a.work_dir / 'results.json').write_text(json.dumps(records, indent=2) + '\n')
print(f'PASS: {len(records)} build/run combinations; both deterministic computation and persistent protocol checks passed')
