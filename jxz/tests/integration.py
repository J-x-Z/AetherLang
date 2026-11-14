#!/usr/bin/env python3
"""Real JXZ command tests. All projects and artifacts stay under --work-dir."""
import argparse
import os
import resource
import signal
from pathlib import Path
import subprocess
import tempfile
import unittest

PARSER = argparse.ArgumentParser()
PARSER.add_argument('--jxz', required=True)
PARSER.add_argument('--aethc', required=True)
PARSER.add_argument('--work-dir', required=True)
PARSER.add_argument('--backend', choices=['llvm', 'c'])
ARGS, REST = PARSER.parse_known_args()
JXZ = str(Path(ARGS.jxz).resolve())
AETHC = str(Path(ARGS.aethc).resolve())
WORK = Path(ARGS.work_dir).resolve()
WORK.mkdir(parents=True, exist_ok=True)
ENV = {**os.environ, 'AETHC': AETHC, 'TMPDIR': str(WORK)}
if ARGS.backend:
    ENV['AETHER_BACKEND'] = ARGS.backend


class LocalCommands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='jxz project ', dir=WORK)
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)

    def run_jxz(self, *args, expected=0, env=None):
        result = subprocess.run([JXZ, *args], cwd=self.root,
                                env={**ENV, **(env or {})}, capture_output=True,
                                text=True, timeout=60)
        self.assertEqual(result.returncode, expected,
                         f'{args}:\n{result.stdout}\n{result.stderr}')
        return result

    def manifest(self, name='program', source=None, target='target', src='src'):
        # Literal TOML strings make the test's shell metacharacters literal.
        text = f"[package]\nname = '{name}'\nversion = '0.1.0'\n[build]\nsrc = '{src}'\ntarget = '{target}'\n"
        if source is not None:
            text += f"main = '{source}'\n"
        (self.root / 'Jxz.toml').write_text(text)

    def source(self, path='src/main.aeth', text='fn main() -> i32 { return 0; }'):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)
        return file

    def test_init_build_run_clean_preserve_source(self):
        (self.root / '.gitignore').write_text('keep-me\n')
        self.run_jxz('init', 'sample')
        self.run_jxz('build', '--release')
        result = self.run_jxz('run')
        self.assertIn('Hello, AetherLang!', result.stdout)
        self.assertTrue((self.root / 'target/sample').is_file())
        self.source('src/native.c', '/* user source */')
        (self.root / 'target/keep-me').write_text('owned by user')
        self.run_jxz('clean')
        self.assertFalse((self.root / 'target/sample').exists())
        self.assertEqual((self.root / 'src/native.c').read_text(), '/* user source */')
        self.assertTrue((self.root / 'target/keep-me').exists())
        self.assertEqual((self.root / '.gitignore').read_text(), 'keep-me\n')
        self.run_jxz('init', expected=1)

    def test_custom_entry_and_long_literal_paths(self):
        directory = '/'.join(['source ' + ('a' * 90)] * 6)
        source = directory + '/entry ; touch INJECTED.aeth'
        target = 'output ; touch INJECTED/nested dir'
        self.manifest(name='an app', source=source, target=target)
        self.source(source, 'fn main() -> i32 { return 7; }')
        self.run_jxz('build')
        self.assertTrue((self.root / target / 'an app').exists())
        self.run_jxz('run', expected=7)
        self.assertFalse((self.root / 'INJECTED').exists())
        self.run_jxz('clean')
        self.assertFalse((self.root / target / 'an app').exists())

    def test_run_preserves_argument_boundaries_and_exit_status(self):
        self.manifest()
        self.source(text='''extern "C" { fn strcmp(a: *u8, b: *u8) -> i32; }
fn main(argc: i32, argv: **u8) -> i32 {
    if argc != 3 { return 20; }
    if strcmp(argv[1], "hello world\\0" as *u8) != 0 { return 21; }
    if strcmp(argv[2], "; touch INJECTED\\0" as *u8) != 0 { return 22; }
    return 37;
}''')
        self.run_jxz('run', '--', 'hello world', '; touch INJECTED', expected=37)
        self.assertFalse((self.root / 'INJECTED').exists())

    def test_signal_status(self):
        self.manifest()
        self.source(text='extern "C" { fn raise(sig: i32) -> i32; } fn main() -> i32 { raise(15); return 0; }')
        self.run_jxz('run', expected=143)

    def test_src_setting_and_absolute_output(self):
        target = str(self.root / 'absolute output')
        self.manifest(src='custom sources', target=target)
        self.source('custom sources/main.aeth')
        self.run_jxz('run')
        self.assertTrue((Path(target) / 'program').exists())

    def test_compiler_errors_are_not_reported_as_success(self):
        self.manifest()
        self.source()
        self.run_jxz('build', env={'AETHC': '/no/such/aethc'}, expected=127)
        self.run_jxz('build', env={'CC': '/no/such/cc'}, expected=1)
        self.source(text='fn main() -> i32 { return true; }')
        self.run_jxz('run', expected=1)

    def test_bad_manifest_rejected_without_side_effects(self):
        self.source()
        variants = [
            '[package-extra]\nname = "wrong"\n',
            '[package]\nname = "first"\nname = "second"\n',
            '[package]\nname = "unterminated\n',
            '[package]\nname = "bad" garbage\n',
            '[package]\nname = "../bad"\n',
            '[package]\nname = "ok"\n[build]\ntarget = ""\n',
            '[package]\nname = "ok"\x00\n',
            '[package]\nname = "ok"\n[dependencies]\nfoo = "1.0"\n',
        ]
        for text in variants:
            with self.subTest(text=text):
                (self.root / 'Jxz.toml').write_text(text)
                self.run_jxz('build', expected=1)
                self.assertFalse((self.root / 'target').exists())

    def test_test_failure_and_stale_outputs(self):
        self.manifest(target='test output')
        self.source('tests/test_pass with space.aeth')
        self.source('tests/nested/test_line\nbreak.aeth')
        self.run_jxz('test')
        self.source('tests/test_exit.aeth', 'fn main() -> i32 { return 9; }')
        self.run_jxz('test', expected=1)
        (self.root / 'tests/test_exit.aeth').unlink()
        self.source('tests/test_bad.aeth', 'fn main() -> i32 { return false; }')
        stale = self.root / 'test output/.jxz-test'
        stale.write_text('#!/bin/sh\ntouch STALE_RAN\n')
        stale.chmod(0o755)
        self.run_jxz('test', expected=1)
        self.assertFalse((self.root / 'STALE_RAN').exists())
        self.assertFalse(stale.exists())

    def test_all_dependency_manifest_forms_are_rejected(self):
        self.source()
        declarations = [
            '[dependencies]\nmissing = "1.0"\n',
            '[dependencies.missing]\npath = "/does/not/exist"\n',
            '["dependencies".missing]\npath = "/does/not/exist"\n',
            "['dependencies'.missing]\npath = '/does/not/exist'\n",
            '[target.unix.dependencies]\nmissing = "1.0"\n',
            '[[dependencies]]\nname = "missing"\n',
        ]
        for declaration in declarations:
            with self.subTest(declaration=declaration):
                (self.root / 'Jxz.toml').write_text('[package]\nname = "sample"\n' + declaration)
                self.run_jxz('build', expected=1)
        for declaration in ('dependencies = { missing = "1.0" }', 'dependencies.missing = "1.0"'):
            (self.root / 'Jxz.toml').write_text(declaration + '\n[package]\nname = "sample"\n')
            self.run_jxz('build', expected=1)
        self.assertFalse((self.root / 'target').exists())

    def test_clean_tracks_every_successful_output_and_legacy_records(self):
        self.source()
        for name in ('first', 'second', 'second'):
            self.manifest(name=name)
            self.run_jxz('build')
        (self.root / 'target/keep-me').write_text('user data')
        (self.root / 'target/legacy-program').write_text('old JXZ output')
        (self.root / 'target/.jxz-output-name').write_text('legacy-program')
        self.assertEqual((self.root / 'target/.jxz-outputs').read_text(), 'first\nsecond\n')
        self.run_jxz('clean')
        self.assertEqual({p.name for p in (self.root / 'target').iterdir()}, {'keep-me'})
        self.run_jxz('clean')

    def test_output_record_write_failure_preserves_previous_collection(self):
        self.source()
        self.manifest(name='first')
        self.run_jxz('build')
        record = self.root / 'target/.jxz-outputs'
        before = record.read_bytes()
        self.manifest(name='second')
        # Stub only the compiler subprocess so the file limit targets the
        # ownership transaction, not the already-covered native compiler.
        compiler = self.root / 'compiler-success'
        compiler.write_text('#!/bin/sh\nexit 0\n')
        compiler.chmod(0o755)
        def fail_writes():
            signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
            resource.setrlimit(resource.RLIMIT_FSIZE, (1, 1))
        result = subprocess.run([JXZ, 'build'], cwd=self.root, capture_output=True,
                                text=True, timeout=20, preexec_fn=fail_writes,
                                env={**ENV, 'AETHC': str(compiler)})
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(record.read_bytes(), before)
        self.assertFalse((self.root / 'target/.jxz-outputs.tmp').exists())
        self.run_jxz('clean')
        self.assertFalse((self.root / 'target/first').exists())

    def test_clean_rejects_incomplete_or_corrupt_ownership_records(self):
        self.manifest()
        (self.root / 'target').mkdir()
        keep = self.root / 'target/keep-me'
        keep.write_text('user data')
        for marker, content in [('.jxz-outputs', b'keep-me'), ('.jxz-output-name', b'keep-me\x00junk')]:
            with self.subTest(marker=marker):
                record = self.root / 'target' / marker
                record.write_bytes(content)
                self.run_jxz('clean', expected=1)
                self.assertEqual(keep.read_text(), 'user data')
                record.unlink()

    def test_clean_does_not_delete_unowned_output(self):
        self.manifest(name='main.aeth', target='src')
        self.source()
        self.run_jxz('clean')
        self.assertTrue((self.root / 'src/main.aeth').is_file())

    def test_init_refuses_to_overwrite_source(self):
        self.source(text='// keep existing source')
        self.run_jxz('init', expected=1)
        self.assertEqual((self.root / 'src/main.aeth').read_text(), '// keep existing source')
        self.assertFalse((self.root / 'Jxz.toml').exists())

    def test_unimplemented_dependency_commands_fail(self):
        for command in ('add', 'remove', 'update', 'install'):
            self.run_jxz(command, 'anything', expected=2)
        self.assertEqual(list(self.root.iterdir()), [])


if __name__ == '__main__':
    unittest.main(argv=['integration.py', *REST])
