#!/usr/bin/env python3
"""Optional existing-CUDA-Q-library ABI, invalid-input and early-error regression tests."""
import argparse
import ctypes as c
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--library', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler, library = args.compiler.resolve(), args.library.resolve()
env = {**os.environ, 'TMPDIR': str(work)}
records = []

def save(record):
    records.append(record)
    (work / 'results.json').write_text(json.dumps(records, indent=2))

class Gate(c.Structure):
    _fields_ = [('kind', c.c_int32), ('target', c.c_uint32), ('control', c.c_uint32), ('angle', c.c_double)]
class Complex(c.Structure):
    _fields_ = [('re', c.c_double), ('im', c.c_double)]

lib = c.CDLL(str(library))
assert lib.aether_cudaq_abi_version() == 1
assert c.sizeof(Gate) == 24 and Gate.angle.offset == 16 and c.sizeof(Complex) == 16
state = lib.aether_cudaq_state
state.argtypes = [c.c_uint32, c.POINTER(Gate), c.c_uint64, c.POINTER(Complex), c.c_uint64,
                  c.c_void_p, c.c_uint64, c.POINTER(c.c_int32)]
state.restype = c.c_int32
for name, qubits, gate, count, capacity in [
    ('zero-qubits', 0, None, 0, 2), ('too-many-qubits', 25, None, 0, 2),
    ('wrong-capacity', 1, None, 0, 1), ('null-gates', 1, None, 1, 2),
    ('bad-kind', 1, Gate(11, 0, 0, 0), 1, 2), ('bad-target', 1, Gate(1, 1, 0, 0), 1, 2),
    ('bad-control', 1, Gate(8, 0, 0, 0), 1, 2), ('nan-angle', 1, Gate(1, 0, 0, float('nan')), 1, 2),
]:
    output = (Complex * 2)(Complex(123, 456), Complex(789, 321))
    error = c.create_string_buffer(b'Z' * 20, 21)
    gpu = c.c_int32(7)
    status = state(qubits, c.byref(gate) if gate else None, count, output, capacity, error, 4, c.byref(gpu))
    save(dict(name=name, status=status, error=error.value.decode(), on_gpu=gpu.value))
    assert status == 1 and gpu.value == 0, records[-1]
    assert error.raw[3] == 0 and error.raw[4:20] == b'Z' * 16
    assert (output[0].re, output[0].im, output[1].re, output[1].im) == (123, 456, 789, 321)

export = lib.aether_cudaq_quake
export.argtypes = [c.c_uint32, c.POINTER(Gate), c.c_uint64, c.c_uint64,
                   c.POINTER(c.c_void_p), c.POINTER(c.c_uint64), c.c_void_p, c.c_uint64]
export.restype = c.c_int32
release = lib.aether_cudaq_quake_free
release.argtypes = [c.c_void_p]
release.restype = None
for name, qubits, gate, count, limit, status in [
    ('zero-qubits', 0, None, 0, 8192, 1), ('invalid-width', 63, None, 0, 8192, 1),
    ('missing-gates', 3, None, 1, 8192, 1),
    ('bad-control', 3, Gate(8, 1, 1, 0), 1, 8192, 1),
    ('infinite-angle', 3, Gate(7, 1, 0, float('inf')), 1, 8192, 1),
    ('zero-budget', 1, None, 0, 0, 3), ('tiny-budget', 3, None, 0, 12, 3),
    ('no-state-allocation', 62, Gate(1, 61, 0, 0), 1, 8192, 0),
]:
    out, length = c.c_void_p(1), c.c_uint64(99)
    error = c.create_string_buffer(b'Z' * 20, 21)
    code = export(qubits, c.byref(gate) if gate else None, count, limit, c.byref(out),
                  c.byref(length), error, 4)
    save(dict(name='quake/'+name, status=code, length=length.value, error=error.value.decode()))
    assert code == status, records[-1]
    if code:
        assert out.value is None and length.value == 0
        assert error.raw[3] == 0 and error.raw[4:20] == b'Z' * 16
    else:
        assert 0 < length.value < limit and error.value == b''
        try:
            data = c.string_at(out, length.value + 1)
            assert data[-1:] == b'\0' and b'quake.veq<62>' in data[:-1]
        finally:
            release(out)
release(None)
for label in ('missing-pointer', 'missing-length'):
    out, length = c.c_void_p(1), c.c_uint64(99)
    error = c.create_string_buffer(80)
    rc = export(1, None, 0, 8192, None if label == 'missing-pointer' else c.byref(out),
                None if label == 'missing-length' else c.byref(length), error, len(error))
    save(dict(name='quake/'+label, status=rc, error=error.value.decode()))
    assert rc == 1 and b'output fields' in error.value
    assert length.value == 0 if label == 'missing-pointer' else out.value is None

repo = Path(__file__).resolve().parents[1]
source = repo / 'tests/regressions/quantum/cudaq_boundaries.aeth'
for backend in args.backends:
    binary = work / f'wrapper-{backend}'
    for label, command in [
        ('build', [compiler, 'build', source, '--sysroot', repo, '--backend', backend, '-O2', '--link-object', library, '-o', binary]),
        ('run', [binary]),
    ]:
        command = list(map(str, command))
        result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=30)
        save(dict(name=f'wrapper/{backend}/{label}', command=command, status=result.returncode,
                  stdout=result.stdout, stderr=result.stderr))
        assert result.returncode == 0, records[-1]
print(f'{len(records)} CUDA-Q ABI and error-path checks passed')
