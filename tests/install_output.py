#!/usr/bin/env python3
"""Relocatable stdlib discovery and transactional compiler output regressions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--only', choices=['install', 'output'], help='Run one independent group')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
results_path = work / 'results.json'
work = Path(tempfile.mkdtemp(prefix='case-', dir=work))
compiler = args.compiler.resolve()
env = {key: value for key, value in os.environ.items() if key not in ['AETHER_PATH', 'CC']}
env['TMPDIR'] = str(work)
records = []
failed = False
metadata = {'compiler': str(compiler), 'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
            'case_directory': str(work)}


def save():
    results_path.write_text(json.dumps({**metadata, 'records': records}, indent=2) + '\n')


def run(label, command, expected=0, validate=None, cwd=None, environment=None, **options):
    global failed
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=cwd or work, env=environment or env,
                                capture_output=True, text=True, timeout=60, **options)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(command, 124, '', 'timeout')
    passed = result.returncode == expected
    detail = None
    if validate:
        try:
            validate(result)
        except (AssertionError, ValueError, KeyError) as error:
            passed = False
            detail = str(error)
    records.append(dict(label=label, command=command, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, passed=passed, validation_error=detail))
    failed |= not passed
    save()
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(result.stderr or detail or result.stdout, flush=True)
    return passed


def checked(condition, reason):
    assert condition, reason


if args.only != 'output':
    # One binary copy is moved through both supported layouts. The probe module
    # exists only in this installation; the real Vec/Option code also must run.
    prefix = work / 'installation Ω'
    (prefix / 'bin').mkdir(parents=True, exist_ok=True)
    installed = prefix / 'bin/aethc'
    shutil.copy2(compiler, installed)
    shutil.copytree(root / 'stdlib', prefix / 'stdlib', dirs_exist_ok=True)
    (prefix / 'stdlib/install_probe.aeth').write_text('pub fn marker() -> i32 { return 7; }\n')
    consumer = work / 'consumer'
    consumer.mkdir(exist_ok=True)
    source = consumer / 'main.aeth'
    source.write_text('''use install_probe::marker;
use vec::{Vec, vec_new, vec_push, vec_get, vec_free};
use option::Option;
fn main() -> i32 {
    let mut values: Vec<i64> = vec_new();
    if !vec_push(&values as *Vec<i64>, 40) || !vec_push(&values as *Vec<i64>, 2) { return 1; }
    let a: i64 = match vec_get(&values as *Vec<i64>, 0) { Option::Some(v) => v, Option::None => -100 };
    let b: i64 = match vec_get(&values as *Vec<i64>, 1) { Option::Some(v) => v, Option::None => -100 };
    vec_free(&values as *Vec<i64>);
    return if a + b == 42 && marker() == 7 { 0 } else { 2 };
}
''')
    elsewhere = work / 'unrelated cwd'
    elsewhere.mkdir(exist_ok=True)
    run('install/prefix-check', [installed, 'check', source], cwd=elsewhere)
    # execvp skips a directory with the compiler's name. Stdlib discovery must
    # skip it too, including a searchable directory without listing permission.
    decoy = work / 'earlier PATH Ω'
    (decoy / 'aethc').mkdir(parents=True)
    path_env = {**env, 'PATH': str(decoy)+os.pathsep+str(prefix/'bin')+os.pathsep+env.get('PATH','')}
    for mode in [0o755, 0o111]:
        (decoy/'aethc').chmod(mode)
        try:
            run('install/path-directory-'+oct(mode), ['aethc','check',source], cwd=elsewhere, environment=path_env)
        finally:
            (decoy/'aethc').chmod(0o755)
    moved = work / 'relocated β'
    prefix.rename(moved)
    prefix = moved
    installed = prefix / 'bin/aethc'
    for backend in args.backends:
        binary = consumer / ('program-' + backend)
        if run('install/relocated-build-' + backend, [installed, 'build', source, '--backend', backend,
                                                     '-o', binary], cwd=elsewhere):
            run('install/relocated-run-' + backend, [binary], cwd=elsewhere)
    installed.rename(prefix / 'aethc')
    installed = prefix / 'aethc'
    run('install/flat-package-check', [installed, 'check', source], cwd=elsewhere)
    conventional = prefix / 'lib/aetherlang/stdlib'
    conventional.parent.mkdir(parents=True, exist_ok=True)
    (prefix / 'stdlib').rename(conventional)
    installed.rename(prefix / 'bin/aethc')
    installed = prefix / 'bin/aethc'
    run('install/conventional-prefix-check', [installed, 'check', source], cwd=elsewhere)
    hidden = conventional.with_name('stdlib.saved')
    conventional.rename(hidden)
    library_only = consumer / 'library_only.aeth'
    library_only.write_text('use vec::{Vec, vec_new}; fn main() -> i32 { let v: Vec<i64> = vec_new(); return v.len as i32; }')
    run('install/no-development-tree-fallback', [installed, 'check', library_only], expected=1, cwd=elsewhere)
    hidden.rename(conventional)
    explicit = work / 'explicit sysroot'
    (explicit / 'stdlib').mkdir(parents=True, exist_ok=True)
    (explicit / 'stdlib/install_probe.aeth').write_text('pub fn marker() -> i32 { return 23; }')
    probe = consumer / 'probe.aeth'
    probe.write_text('use install_probe::marker; fn main() -> i32 { return marker(); }')
    binary = consumer / 'probe'
    if run('install/explicit-sysroot-build', [installed, 'build', probe, '--backend', args.backends[0],
                                            '--sysroot', explicit, '-o', binary], cwd=elsewhere):
        run('install/explicit-sysroot-run', [binary], expected=23)
    run('install/sysroot-does-not-mix-stdlibs', [installed, 'check', library_only, '--sysroot', explicit], expected=1)
    run('install/invalid-sysroot', [installed, 'check', probe, '--sysroot', work / 'absent'], expected=1,
        validate=lambda result: checked('sysroot' in result.stderr, 'missing sysroot diagnostic'))
    overrides = work / 'module overrides'
    overrides.mkdir(exist_ok=True)
    (overrides / 'install_probe.aeth').write_text('pub fn marker() -> i32 { return 31; }')
    for label, options, environment in [
        ('module-path', ['-I', overrides], env),
        ('environment', [], {**env, 'AETHER_PATH': str(overrides)}),
    ]:
        if run('install/override-' + label, [installed, 'build', probe, '--backend', args.backends[0],
                                            '--sysroot', explicit, *options, '-o', binary], environment=environment):
            run('install/override-run-' + label, [binary], expected=31)

if args.only != 'install':
    directory = work / 'output transactions'
    directory.mkdir(exist_ok=True)
    source = directory / 'main.aeth'
    source.write_text('''fn calculate(count: i32) -> i32 {
    let mut total: i32 = 0;
    let mut index: i32 = 0;
    while index < count { total = total + 2; index = index + 1; }
    return total;
}
fn main() -> i32 { return calculate(21); }
''')
    destination = directory / 'previous output'
    original = b'ORIGINAL ARTIFACT\n' * 40

    def preserved(result):
        checked(destination.read_bytes() == original, 'previous output changed')
        checked(not list(directory.glob('.aethc-output-*')), 'output staging directory leaked')

    def fail_case(label, flags, **options):
        destination.write_bytes(original)
        run(label, [compiler, 'build', source, *flags, '-o', destination], expected=1,
            validate=preserved, **options)

    if os.name == 'posix':
        import resource

        def limit_writes():
            signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
            resource.setrlimit(resource.RLIMIT_FSIZE, (128, 128))

        for mode in ['--emit-c', '--emit-ir']:
            fail_case('output/write-limit-' + mode[2:], [mode, '--backend', args.backends[0]], preexec_fn=limit_writes)
        if 'llvm' in args.backends:
            for mode in ['--emit-obj', '--emit-llvm']:
                fail_case('output/write-limit-' + mode[2:], [mode, '--backend', 'llvm'], preexec_fn=limit_writes)
        stub = directory / 'failing cc'
        stub.write_text('#!' + sys.executable + '\nimport pathlib,sys\npathlib.Path(sys.argv[sys.argv.index("-o")+1]).write_bytes(b"PARTIAL")\nprint("deliberate-link-error",file=sys.stderr)\nsys.exit(23)\n')
        stub.chmod(0o755)
        for backend in args.backends:
            fail_case('output/partial-native-failure-' + backend, ['--backend', backend, '--cc', stub])
        stub.write_text('#!' + sys.executable + '\nimport sys\nsys.exit(0)\n')
        fail_case('output/no-native-artifact', ['--backend', args.backends[0], '--cc', stub])
    for backend in args.backends:
        fail_case('output/actual-link-failure-' + backend, ['--backend', backend, '--link-object', directory / 'missing.o'])
        destination.write_bytes(original)
        if run('output/successful-replacement-' + backend, [compiler, 'build', source, '--backend', backend, '-o', destination],
               validate=lambda result: checked(not list(directory.glob('.aethc-output-*')), 'output staging directory leaked')):
            run('output/replacement-runs-' + backend, [destination], expected=42)
    destination.write_bytes(original)
    if run('output/emit-c-replaces-old-file', [compiler, 'build', source, '--emit-c', '-o', destination],
           validate=lambda result: checked(destination.read_bytes() != original, 'successful output not published')):
        run('output/generated-c-is-valid', ['clang', '-fsyntax-only', '-x', 'c', destination])
    protected = directory / 'protected directory'
    protected.mkdir(exist_ok=True)
    sentinel = protected / 'user file'
    sentinel.write_bytes(original)
    run('output/publish-failure-preserves-directory', [compiler, 'build', source, '--emit-c', '-o', protected], expected=1,
        validate=lambda result: checked(sentinel.read_bytes() == original and not list(directory.glob('.aethc-output-*')),
                                         'failed publication modified destination or leaked temporary'))

print(f'{sum(record["passed"] for record in records)}/{len(records)} checks passed; {results_path}')
raise SystemExit(1 if failed else 0)
