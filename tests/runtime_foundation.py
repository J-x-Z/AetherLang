#!/usr/bin/env python3
"""Native foundation and targeted I/O failures; no Cargo invocation or downloads."""
import argparse
import errno
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys
import tempfile
import unittest

PARSER = argparse.ArgumentParser()
PARSER.add_argument('--compiler', type=Path, required=True)
PARSER.add_argument('--work-dir', type=Path, required=True)
PARSER.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
ARGS, REST = PARSER.parse_known_args()
if REST and REST[0] == "--":
    REST = REST[1:]
ROOT = Path(__file__).resolve().parents[1]
WORK = ARGS.work_dir.resolve()
WORK.mkdir(parents=True, exist_ok=True)
COMPILER = ARGS.compiler.resolve()
ENV = {**os.environ, 'TMPDIR': str(WORK)}
RECORDS = []
BINARIES = {}
INJECT = WORK / ('io_faults.dylib' if sys.platform == 'darwin' else 'io_faults.so')
INJECT_ENV = 'DYLD_INSERT_LIBRARIES' if sys.platform == 'darwin' else 'LD_PRELOAD'


def command(args, **kwargs):
    return subprocess.run(list(map(str, args)), capture_output=True, text=True,
                          timeout=20, env=kwargs.pop('env', ENV), **kwargs)


def limit_file_size(size):
    def setup():
        signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
        resource.setrlimit(resource.RLIMIT_FSIZE, (size, size))
    return setup


class RuntimeFoundation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        flags = ['-dynamiclib'] if sys.platform == 'darwin' else ['-shared', '-fPIC']
        result = command([os.environ.get('CC', 'cc'), *flags,
                          ROOT / 'tests/regressions/runtime/io_faults.c', '-o', INJECT,
                          *([] if sys.platform == 'darwin' else ['-ldl'])])
        if result.returncode:
            raise AssertionError(result.stderr)
        for backend in ARGS.backends:
            for name, source in [('foundation', ROOT / 'tests/regressions/runtime/foundation.aeth'),
                                 ('directories', ROOT / 'tests/regressions/runtime/directories.aeth'),
                                 ('byte_stream', ROOT / 'tests/regressions/runtime/byte_stream.aeth'),
                                 ('file_errors', ROOT / 'tests/regressions/runtime/file_errors.aeth'),
                                 ('accumulator', ROOT / 'examples/upgrade/accumulator.aeth')]:
                binary = WORK / f'{name}-{backend}'
                result = command([COMPILER, 'build', source, '--backend', backend, '-O2', '-o', binary])
                if result.returncode:
                    raise AssertionError(result.stderr)
                BINARIES[name, backend] = binary

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='runtime case ', dir=WORK)
        self.work = Path(self.temp.name)
        self.addCleanup(self.temp.cleanup)

    def record(self, backend, name, result, expected):
        RECORDS.append(dict(test=name, backend=backend, exit=result.returncode,
                            stdout=result.stdout, stderr=result.stderr))
        self.assertEqual(result.returncode, expected, f'{name}/{backend}: {result.stdout}\n{result.stderr}')

    def test_foundation_end_to_end(self):
        for backend in ARGS.backends:
            with self.subTest(backend=backend):
                result = command([BINARIES['foundation', backend], self.work, 'hello world', '--', '-literal'])
                self.record(backend, 'foundation', result, 0)
                self.assertEqual(result.stdout, 'literal %% text\n')

    def test_binary_streams_keep_length_and_errors(self):
        for backend in ARGS.backends:
            for count in (0, 3, 4096, 8193, 9000, 9001):
                with self.subTest(backend=backend, count=count):
                    data = bytes(i % 251 for i in range(count))
                    target = self.work / f'{backend}-{count}'
                    result = subprocess.run([BINARIES['byte_stream', backend], target, self.work/'missing'],
                                            input=data, capture_output=True, timeout=10, env=ENV)
                    self.assertEqual(result.returncode, 27 if count > 9000 else 0, result.stderr)
                    if count <= 9000:
                        self.assertEqual(target.read_bytes(), data)
                    else:
                        self.assertFalse(target.exists())
                    RECORDS.append(dict(test='bounded-binary-stream', backend=backend, bytes=count, exit=result.returncode))

    def test_recursive_directory_errors_and_search_permissions(self):
        file = self.work / 'regular'
        file.write_bytes(b'original')
        locked = self.work / 'locked'
        searchable = self.work / 'searchable'
        locked.mkdir()
        searchable.mkdir()
        (searchable/'existing').mkdir()
        locked.chmod(0o500)
        searchable.chmod(0o300)
        try:
            for backend in ARGS.backends:
                for arg, code in [('', errno.EINVAL),('/', 0),(str(self.work)+'/multi//a/b/', 0),
                                  (str(file), errno.ENOTDIR),(str(file/'child'), errno.ENOTDIR)]:
                    self.record(backend,'mkdir-result',command([BINARIES['directories', backend], arg]),code)
                if os.geteuid() != 0:
                    self.record(backend,'mkdir-denied',command([BINARIES['directories', backend], locked/'child']),errno.EACCES)
                    self.record(backend,'mkdir-searchable',command([BINARIES['directories', backend], searchable/'existing'/backend]),0)
                    self.assertTrue((searchable/'existing'/backend).is_dir())
            self.assertEqual(file.read_bytes(), b'original')
            self.assertTrue((self.work/'multi/a/b').is_dir())
        finally:
            locked.chmod(0o700)
            searchable.chmod(0o700)

    def test_file_write_checks_buffered_close_errors(self):
        for backend in ARGS.backends:
            result = command([BINARIES['file_errors', backend], 'write', self.work / backend],
                             preexec_fn=limit_file_size(0))
            self.record(backend, 'file-buffered-close', result, 0)
            self.assertEqual((self.work / backend).stat().st_size, 0)

    def test_file_write_line_checks_newline_errors(self):
        for backend in ARGS.backends:
            result = command([BINARIES['file_errors', backend], 'line', self.work / backend],
                             preexec_fn=limit_file_size(3))
            self.record(backend, 'file-newline', result, 0)
            self.assertEqual((self.work / backend).read_bytes(), b'abc')

    def test_service_detects_initial_stdout_error(self):
        readonly = self.work / 'readonly'
        readonly.write_bytes(b'sentinel')
        for backend in ARGS.backends:
            with readonly.open('rb') as output:
                result = subprocess.run([str(BINARIES['accumulator', backend]), str(self.work / backend)],
                                        input='quit\n', text=True, stdout=output, stderr=subprocess.PIPE,
                                        env=ENV, timeout=10)
            self.record(backend, 'service-stdout', result, 8)
            self.assertEqual(readonly.read_bytes(), b'sentinel')

    def test_service_keeps_journal_write_error_before_close_error(self):
        for backend in ARGS.backends:
            result = command([BINARIES['accumulator', backend], self.work / backend],
                             input='add 12\nquit\n', preexec_fn=limit_file_size(0))
            self.record(backend, 'service-journal-first-error', result, 7)
            self.assertEqual(result.stdout, 'READY\n')
            self.assertEqual((self.work / backend).stat().st_size, 0)

    def test_service_detects_final_output_and_close_errors(self):
        faults = [({'AETHER_FAULT_BYE': '1'}, 8),
                  ({'AETHER_FAULT_FINAL_FLUSH': '1'}, 8),
                  ({'AETHER_FAULT_CLOSE': '1'}, 9),
                  ({'AETHER_FAULT_CLOSE': '2'}, 10),
                  ({'AETHER_FAULT_CLOSE': 'both'}, 9)]
        for backend in ARGS.backends:
            for index, (fault, expected) in enumerate(faults):
                with self.subTest(backend=backend, fault=fault):
                    env = {**ENV, INJECT_ENV: str(INJECT), **fault}
                    result = command([BINARIES['accumulator', backend], self.work / f'{backend}-{index}'],
                                     input='quit\n', env=env)
                    self.record(backend, 'service-' + next(iter(fault)) + '-' + next(iter(fault.values())), result, expected)


if __name__ == '__main__':
    suite = unittest.main(argv=['runtime_foundation.py', *REST], exit=False)
    (WORK / 'results.json').write_text(json.dumps(RECORDS, indent=2) + '\n')
    raise SystemExit(0 if suite.result.wasSuccessful() else 1)
