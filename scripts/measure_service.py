#!/usr/bin/env python3
"""Measure live RSS and open descriptors at fixed points in one real service."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--backend', choices=['c', 'llvm'], required=True)
parser.add_argument('--work-dir', type=Path, required=True)
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
source = Path(__file__).resolve().parents[1] / 'examples/upgrade/accumulator.aeth'
binary = work / 'accumulator'
env = dict(os.environ, TMPDIR=str(work))
subprocess.run([compiler, source, '--backend', args.backend, '-O2', '-o', binary],
               env=env, capture_output=True, check=True, timeout=90)


def snapshot(pid, events):
    rss_kib = subprocess.check_output(['ps', '-o', 'rss=', '-p', str(pid)], text=True)
    fd_dir = Path('/proc') / str(pid) / 'fd'
    if fd_dir.is_dir():
        descriptors = len(list(fd_dir.iterdir()))
    else:
        listing = subprocess.check_output(['lsof', '-nP', '-a', '-p', str(pid), '-Ff'],
                                          text=True, timeout=10)
        descriptors = sum(line.startswith('f') and line[1:].isdecimal() for line in listing.splitlines())
    if descriptors < 3:
        raise RuntimeError(f'incomplete descriptor snapshot for process {pid}')
    return dict(events=events, rss_bytes=int(rss_kib) * 1024, descriptors=descriptors)


def await_reply(process, stream, reply):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline and process.poll() is None:
        end = stream.seek(0, os.SEEK_END)
        if end >= len(reply):
            stream.seek(-len(reply), os.SEEK_END)
            if stream.read() == reply:
                return
        time.sleep(0.002)
    raise RuntimeError(f'service lost at {reply!r}, status {process.poll()}')


checkpoints = (2000, 20000, 100000)
rows = []
with tempfile.TemporaryDirectory(dir=work) as scratch:
    journal = Path(scratch) / 'journal'
    replies = Path(scratch) / 'replies'
    with replies.open('wb') as output, replies.open('rb') as stream:
        with subprocess.Popen([binary, journal], env=env, stdin=subprocess.PIPE, stdout=output) as process:
            try:
                await_reply(process, stream, b'READY\n')
                rows.append(snapshot(process.pid, 0))
                previous = 0
                for count in checkpoints:
                    process.stdin.write(b'add 1\n' * (count - previous) + b'get\n')
                    process.stdin.flush()
                    await_reply(process, stream, f'VALUE {count} {count}\n'.encode())
                    rows.append(snapshot(process.pid, count))
                    previous = count
                process.stdin.write(b'quit\n')
                process.stdin.close()
                if process.wait(timeout=10) != 0:
                    raise RuntimeError('service exited unsuccessfully')
            finally:
                if process.poll() is None:
                    process.kill()
    with replies.open('rb') as stream:
        assert stream.readline() == b'READY\n'
        for event in range(1, checkpoints[-1] + 1):
            assert stream.readline() == f'OK {event}\n'.encode(), event
            if event in checkpoints:
                assert stream.readline() == f'VALUE {event} {event}\n'.encode(), event
        assert stream.read() == f'BYE {event} {event}\n'.encode()
    assert journal.read_bytes() == b'add 1\n' * checkpoints[-1]
report = dict(compiler=str(compiler), compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
              source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(), backend=args.backend,
              optimization=2, host=list(os.uname())[:3], samples=rows)
(work / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
print(rows)
