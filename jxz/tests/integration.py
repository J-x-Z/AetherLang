#!/usr/bin/env python3
"""Real JXZ command tests. All projects and artifacts stay under --work-dir."""
import argparse
import json
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

    def build_settings(self, text):
        with (self.root / 'Jxz.toml').open('a') as manifest:
            manifest.write(text + '\n')

    def capturing_compiler(self):
        compiler = self.root / 'compiler stub Ω ; touch INJECTED'
        compiler.write_text('''#!/usr/bin/env python3
import json, os, sys
with open(os.environ['JXZ_CAPTURE_ARGS'], 'w') as output:
    json.dump(sys.argv[1:], output)
sys.exit(23)
''')
        compiler.chmod(0o755)
        capture = self.root / 'compiler-argv.json'
        return capture, {'AETHC': str(compiler), 'JXZ_CAPTURE_ARGS': str(capture)}

    def assert_compiler_args(self, actual, source, output, module_paths, link_objects):
        self.assertEqual(actual[0], 'build')
        self.assertEqual(actual[-2:], ['--', source])
        options = actual[1:-2]
        self.assertEqual(len(options) % 2, 0, actual)
        pairs = list(zip(options[::2], options[1::2]))
        # Compare every argument, allowing unrelated option groups to move.
        # Each configured path must remain one argument, in its array's order.
        def normalized(path):
            return './' + path if path.startswith(('-', '@')) else path
        modules = [normalized(path) for path in module_paths]
        objects = [normalized(path) for path in link_objects]
        expected = [('-O', '0'), ('-o', output)]
        if ENV.get('AETHER_BACKEND'):
            expected.append(('--backend', ENV['AETHER_BACKEND']))
        expected += [('-I', path) for path in modules]
        expected += [('--link-object', path) for path in objects]
        self.assertCountEqual(pairs, expected)
        self.assertEqual([value for flag, value in pairs if flag == '-I'], modules)
        self.assertEqual([value for flag, value in pairs if flag == '--link-object'], objects)

    def test_configured_modules_and_native_object_build_run_test(self):
        first = '-modules α ; touch INJECTED'
        second = 'modules β [$HOME]*'
        native = '@native 物件 $(touch INJECTED) [*].o'
        self.manifest()
        self.build_settings(
            f'module_paths = ["{first}", \'{second}\',] # independent roots\n'
            'link_objects = [\n'
            '  # A real object, resolved relative to this project.\n'
            f'  "{native}", # trailing comma and raw Unicode\n'
            ']')
        self.source(first + '/answer.aeth', '''use value::number;
pub fn answer() -> i32 { number() + 20 }
''')
        self.source(second + '/value.aeth', '''extern "C" { fn native_number() -> i32; }
pub fn number() -> i32 { native_number() }
''')
        c_source = self.source('native source.c', '#include <stdint.h>\nint32_t native_number(void) { return 22; }\n')
        native_build = subprocess.run([ENV.get('CC', 'cc'), '-c', str(c_source),
                                       '-o', str(self.root / native)],
                                      cwd=self.root, env=ENV, capture_output=True,
                                      text=True, timeout=30)
        self.assertEqual(native_build.returncode, 0, native_build.stderr)
        self.source(text='''use answer::answer;
extern "C" { fn printf(format: *u8, ...) -> i32; }
fn main() -> i32 {
    let value: i32 = answer();
    printf("configured=%d\\n\\0" as *u8, value);
    if value == 42 { 0 } else { 9 }
}''')
        self.source('tests/test_配置 空格.aeth', '''use answer::answer;
fn main() -> i32 { if answer() == 42 { 0 } else { 9 } }
''')
        self.run_jxz('build')
        self.assertTrue((self.root / 'target/program').is_file())
        self.assertIn('configured=42\n', self.run_jxz('run').stdout)
        tested = self.run_jxz('test')
        self.assertIn('tests/test_配置 空格.aeth', tested.stdout)
        self.assertFalse((self.root / 'INJECTED').exists())

    def test_build_arrays_preserve_exact_argv_and_compiler_failure(self):
        self.source()
        test_source = 'tests/test_配置 空格.aeth'
        self.source(test_source)
        capture, env = self.capturing_compiler()
        modules = ['-root α', '~/$HOME/[a]* ; touch INJECTED', 'quoted "root"\\name']
        objects = ['-native Ω.o', '@response [*].o',
                   str(self.root / 'absolute object ; $(touch INJECTED).o')]
        cases = [
            ('empty', [], [], 'module_paths = []\nlink_objects = [ # empty\n]\n'),
            ('inline', modules, objects,
             'module_paths = ' + json.dumps(modules, ensure_ascii=False) + '\n'
             'link_objects = ' + json.dumps(objects, ensure_ascii=False) + '\n'),
            ('multiline', modules, objects,
             'module_paths = [ # roots\n'
             + ''.join('  ' + json.dumps(path, ensure_ascii=False) + ', # root\n' for path in modules)
             + ']\nlink_objects = [\n'
             + ''.join("  '" + path + "', # literal object\n" for path in objects)
             + '] # final trailing comma accepted\n'),
        ]
        # Cross several parser reallocations and the former fixed argv capacity.
        many_roots = [f'module {i} Ω' for i in range(21)]
        many_objects = [f'object {i} Ω.o' for i in range(17)]
        cases.append(('growth', many_roots, many_objects,
                      'module_paths = ' + json.dumps(many_roots, ensure_ascii=False) + '\n'
                      'link_objects = ' + json.dumps(many_objects, ensure_ascii=False) + '\n'))
        for name, roots, native_objects, settings in cases:
            with self.subTest(syntax=name):
                self.manifest()
                self.build_settings(settings)
                for command in ('build', 'run', 'test'):
                    with self.subTest(command=command):
                        capture.unlink(missing_ok=True)
                        # test aggregates failed tests as 1; build/run preserve
                        # the compiler's precise nonzero exit status.
                        self.run_jxz(command, expected=1 if command == 'test' else 23, env=env)
                        self.assertTrue(capture.is_file(), 'compiler was not invoked')
                        self.assert_compiler_args(
                            json.loads(capture.read_text()),
                            test_source if command == 'test' else 'src/main.aeth',
                            './target/.jxz-test' if command == 'test' else 'target/program',
                            roots, native_objects)
                        self.assertFalse((self.root / 'INJECTED').exists())

    def test_invalid_build_arrays_rejected_before_compiler(self):
        self.source()
        capture, env = self.capturing_compiler()
        invalid = {
            'scalar': '"single path"',
            'number': '[1]',
            'valid-then-number': '["allocated first", 1]',
            'boolean': '[true]',
            'nested-array': '[["first"]]',
            'empty-basic-path': '[""]',
            'empty-literal-path': "['']",
            'escaped-newline': '["line\\nbreak"]',
            'escaped-tab': '["tab\\tpath"]',
            'escaped-carriage-return': '["carriage\\rreturn"]',
            'raw-del': '["bad\x7fpath"]',
            'missing-comma': '["first" "second"]',
            'unclosed-array': '["first",',
            'unclosed-string': '["first]',
            'extra-token': '["first"] garbage',
        }
        for key in ('module_paths', 'link_objects'):
            declarations = [(label, f'{key} = {value}') for label, value in invalid.items()]
            declarations.append(('duplicate-key', f'{key} = []\n{key} = [\n]'))
            for label, declaration in declarations:
                with self.subTest(key=key, syntax=label):
                    self.manifest()
                    other = 'link_objects' if key == 'module_paths' else 'module_paths'
                    self.build_settings(other + ' = ["already allocated"]')
                    self.build_settings(declaration)
                    self.run_jxz('build', expected=1, env=env)
                    self.assertFalse(capture.exists(), 'invalid config reached the compiler')
                    self.assertFalse((self.root / 'target').exists())

    def test_missing_and_corrupt_link_objects_fail_real_link(self):
        self.source()
        self.source('tests/test_link.aeth')
        for name in ('missing object Ω.o', 'corrupt object Ω.o'):
            with self.subTest(object=name):
                self.manifest()
                self.build_settings('module_paths = []\nlink_objects = ["' + name + '"]')
                if name.startswith('corrupt'):
                    (self.root / name).write_bytes(b'this is not a native object\n')
                for command in ('build', 'test'):
                    with self.subTest(command=command):
                        result = self.run_jxz(command, expected=1)
                        self.assertIn(name, result.stdout + result.stderr)
                        self.assertFalse((self.root / 'target/program').exists())
                        self.assertFalse((self.root / 'target/.jxz-test').exists())

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
