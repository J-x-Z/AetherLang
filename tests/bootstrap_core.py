#!/usr/bin/env python3
"""Build the Aether-written Core compiler through four native generations.

The harness copies source files verbatim and invokes native C compilation. Every
stage after the seed loads modules, lexes, parses, checks and emits C in Aether.
All intermediate source, executables and evidence stay below --work-dir.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess
import sys
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True, help='Rust seed aethc')
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backend', choices=['llvm', 'c'], default='llvm')
parser.add_argument('--cc', default=os.environ.get('CC', 'clang'))
parser.add_argument('--skip-sanitizers', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
logs = work / 'logs'
logs.mkdir(exist_ok=True)
compiler = args.compiler.resolve()
cc = shutil.which(args.cc)
if not cc:
    raise SystemExit(f'C compiler not found: {args.cc}')
cc = str(Path(cc).resolve())
records = []
metadata = {}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save():
    (work / 'results.json').write_text(json.dumps({'metadata': metadata, 'checks': records}, indent=2) + '\n')


def check(name, passed, **facts):
    records.append(dict(name=name, passed=bool(passed), **facts))
    save()
    print(f'{"PASS" if passed else "FAIL"} {name}', flush=True)
    if not passed:
        raise SystemExit(f'Bootstrap stopped at {name}; evidence: {work / "results.json"}')


trap = work / 'trap-bin'
trap.mkdir(exist_ok=True)
trap_record = work / 'external-aethc-called'
if trap_record.exists():
    trap_record.unlink()
for name in ['aethc', 'aethc-c']:
    file = trap / name
    file.write_text('#!/bin/sh\nprintf "unexpected compiler call\\n" >> "$BOOTSTRAP_TRAP_RECORD"\nexit 109\n')
    file.chmod(0o755)
env = {**os.environ, 'TMPDIR': str(work), 'PATH': str(trap) + os.pathsep + os.environ.get('PATH', ''),
       'AETHC': str(trap / 'aethc'), 'BOOTSTRAP_TRAP_RECORD': str(trap_record)}


def run(name, command, expected=0, timeout=120, **options):
    started = time.monotonic()
    try:
        result = subprocess.run(list(map(str, command)), capture_output=True, text=True, timeout=timeout,
                                env=options.pop('env', env), cwd=options.pop('cwd', work), **options)
    except subprocess.TimeoutExpired as error:
        stdout = error.stdout or b''
        stderr = error.stderr or b''
        result = subprocess.CompletedProcess(command, 124,
                  stdout.decode(errors='replace') if isinstance(stdout, bytes) else stdout,
                  (stderr.decode(errors='replace') if isinstance(stderr, bytes) else stderr) + '\nTIMEOUT')
    elapsed = time.monotonic() - started
    (logs / f'{name}.stdout').write_text(result.stdout)
    (logs / f'{name}.stderr').write_text(result.stderr)
    accepted = result.returncode in expected if isinstance(expected, list) else result.returncode == expected
    check(name, accepted and not trap_record.exists(), command=list(map(str, command)),
          exit=result.returncode, expected_exit=expected, elapsed_seconds=elapsed,
          stdout_log=str(logs / f'{name}.stdout'), stderr_log=str(logs / f'{name}.stderr'))
    return result


paths = sorted((root / 'src_aether' / 'core').glob('*.aeth'))
source_tree = work / 'core-source'
source_tree.mkdir(exist_ok=True)
# File copies only: the compiler itself finds, reads and resolves every import.
for path in paths:
    shutil.copyfile(path, source_tree / path.name)
source = source_tree / 'main.aeth'
metadata.update(seed_compiler=str(compiler), seed_sha256=digest(compiler),
                source_sha256=digest(source), source_bytes=sum(p.stat().st_size for p in paths),
                source_entry=str(source), source_loading="compiler-native-module-graph",
                source_modules={str(p.relative_to(root)): digest(p) for p in paths},
                cc=cc, platform=sys.platform, backend=args.backend,
                core_commit=subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=root,
                                           capture_output=True, text=True).stdout.strip())
save()
stages = {n: work / f'stage{n}' for n in range(1, 5)}
run('seed-build-stage1', [compiler, 'build', source, '--backend', args.backend, '-O2', '-o', stages[1]], timeout=300)
for number in range(1, 4):
    generated = work / f'stage{number + 1}.c'
    run(f'stage{number}-check-self', [stages[number], source, '--check'])
    run(f'stage{number}-emit-self', [stages[number], source, '-o', generated])
    run(f'cc-stage{number + 1}', [cc, '-std=c11', '-O2', '-Wno-incompatible-library-redeclaration',
                                generated, '-lm', '-o', stages[number + 1]], timeout=300)
run('stage4-check-self', [stages[4], source, '--check'])
check('three-generations-byte-identical', (work / 'stage2.c').read_bytes() == (work / 'stage3.c').read_bytes()
      == (work / 'stage4.c').read_bytes(), c_sha256=digest(work / 'stage2.c'))
metadata['artifacts'] = {path.name: dict(bytes=path.stat().st_size, sha256=digest(path))
                         for path in [*stages.values(), work / 'stage2.c', work / 'stage3.c', work / 'stage4.c']}
save()

fixtures = {'recursive_heap': 'heap=49995000 factorial=3628800 fib=6765\n',
            'file_io': 'file-bytes=5\n',
            'effects_numbers': 'effects=123 wrap=4 decimal=12\n',
            'literal_control_flow': '', 'typed_values': 'typed-values=ok\n', 'foreign_names': '', 'llvm_abi': 'llvm-abi=ok\n'}
fixture_root = root / 'tests' / 'regressions' / 'bootstrap'
def companion_objects(name):
    companion = fixture_root / f'{name}.c'
    return [companion] if companion.is_file() else []

for name, expected in fixtures.items():
    fixture = fixture_root / f'{name}.aeth'
    generated = work / f'{name}.c'
    run(f'fixture-{name}-check', [stages[2], fixture, '--check'])
    run(f'fixture-{name}-emit', [stages[2], fixture, '-o', generated])
    for level in ['0', '2']:
        binary = work / f'{name}-O{level}'
        run(f'fixture-{name}-cc-O{level}', [cc, '-std=c11', '-Werror=return-type', f'-O{level}', generated, *companion_objects(name), '-lm', '-o', binary])
        command = [binary]
        if name == 'file_io':
            command += [work / 'io-original', work / 'io-renamed']
        result = run(f'fixture-{name}-run-O{level}', command)
        check(f'fixture-{name}-output-O{level}', result.stdout == expected and result.stderr == '',
              stdout=result.stdout, stderr=result.stderr)
        if name == 'file_io':
            check(f'fixture-file-cleanup-O{level}', not (work / 'io-original').exists() and not (work / 'io-renamed').exists())

# Division/remainder by zero and signed MIN/-1 are defined traps, including
# under optimization. A diagnostic or arbitrary nonzero exit is not a trap.
def no_core_dump():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


trap_source = fixture_root / 'arithmetic_trap.aeth'
trap_c = work / 'arithmetic_trap.c'
run('arithmetic-trap-emit', [stages[2], trap_source, '-o', trap_c])
for level in ['0', '2']:
    binary = work / f'arithmetic-trap-O{level}'
    run(f'arithmetic-trap-cc-O{level}', [cc, '-std=c11', f'-O{level}', trap_c, '-o', binary])
    for mode in ['division-zero', 'division-overflow', 'remainder-zero', 'remainder-overflow']:
        selector = {'division-zero': '0', 'division-overflow': '1',
                    'remainder-zero': '2', 'remainder-overflow': '3'}[mode]
        run(f'{mode}-O{level}', [binary, selector], expected=[-signal.SIGILL, -signal.SIGTRAP], preexec_fn=no_core_dump)

run('semantic-corpus-stage2', [sys.executable, root / 'tests' / 'core_semantics.py',
                              '--compiler', stages[2], '--work-dir', work / 'semantics-stage2', '--cc', cc], timeout=180)

run('module-projects-stage2', [sys.executable, root / 'tests' / 'bootstrap_modules.py',
                              '--compiler', stages[2], '--compare', stages[4],
                              '--work-dir', work / 'modules-stage2', '--cc', cc], timeout=180)

bad_lexemes = [b'0x', b'1e+', b'1__2', b'1_', b'123abc', b'"\\q"', b'"\\x0"',
               b'"unterminated', b'/* unfinished', b'/* /* */', b'@', b'\0']
for number, data in enumerate(bad_lexemes):
    file = work / f'bad-lexeme-{number}.aeth'
    file.write_bytes(data)
    run(f'reject-lexeme-{number}', [stages[2], file, '--check'], expected=1)

oversized = work / 'oversized.aeth'
with oversized.open('wb') as stream:
    stream.truncate(33554433)
run('reject-oversized-source', [stages[2], oversized, '--check'], expected=1)
oversized.unlink()

# Errors cannot destroy an earlier successful output or the compiler's input.
original = work / 'preserved.c'
original.write_text('USER ORIGINAL\n')
invalid = work / 'invalid.aeth'
invalid.write_text('fn broken( {')
run('invalid-input-preserves-output', [stages[2], invalid, '-o', original], expected=1)
check('invalid-output-unchanged', original.read_text() == 'USER ORIGINAL\n')
alias = work / 'input-alias.aeth'
if alias.is_symlink() or alias.exists():
    alias.unlink()
alias.symlink_to(source)
run('reject-input-output-alias', [stages[2], source, '-o', alias], expected=1)
check('input-unchanged', digest(source) == metadata['source_sha256'])


def no_file_growth():
    signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (0, 0))


run('write-failure-preserves-output', [stages[2], fixture_root / 'effects_numbers.aeth', '-o', original],
    expected=1, preexec_fn=no_file_growth)
check('failed-output-cleaned', original.read_text() == 'USER ORIGINAL\n' and not list(work.glob('preserved.c.tmp.*')))

if not args.skip_sanitizers:
    sanitized = work / 'stage2-sanitized'
    flags = ['-O1', '-g', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-fno-sanitize-recover=all']
    sanitizer_env = {**env, 'ASAN_OPTIONS': 'detect_leaks=0:abort_on_error=1',
                     'UBSAN_OPTIONS': 'halt_on_error=1:print_stacktrace=1'}
    run('cc-stage2-sanitized', [cc, '-std=c11', *flags, '-Wno-incompatible-library-redeclaration',
                               work / 'stage2.c', '-lm', '-o', sanitized], timeout=300)
    run('sanitized-check-self', [sanitized, source, '--check'], env=sanitizer_env)
    run('sanitized-emit-self', [sanitized, source, '-o', work / 'sanitized-self.c'], env=sanitizer_env)
    check('sanitized-self-identical', (work / 'sanitized-self.c').read_bytes() == (work / 'stage2.c').read_bytes())
    run('semantic-corpus-sanitized', [sys.executable, root / 'tests' / 'core_semantics.py',
                                    '--compiler', sanitized, '--work-dir', work / 'semantics-sanitized', '--cc', cc],
        env=sanitizer_env, timeout=180)
    run('module-projects-sanitized', [sys.executable, root / 'tests' / 'bootstrap_modules.py',
                                    '--compiler', sanitized, '--work-dir', work / 'modules-sanitized',
                                    '--sanitize', '--cc', cc], env=sanitizer_env, timeout=180)
    for name, expected in fixtures.items():
        generated = work / f'{name}-sanitized.c'
        run(f'sanitized-{name}-emit', [sanitized, fixture_root / f'{name}.aeth', '-o', generated], env=sanitizer_env)
        check(f'sanitized-{name}-identical', generated.read_bytes() == (work / f'{name}.c').read_bytes())
        binary = work / f'{name}-sanitized'
        run(f'sanitized-{name}-cc', [cc, '-std=c11', *flags, generated, *companion_objects(name), '-lm', '-o', binary])
        command = [binary]
        if name == 'file_io':
            command += [work / 'io-sanitized-original', work / 'io-sanitized-renamed']
        result = run(f'sanitized-{name}-run', command, env=sanitizer_env)
        check(f'sanitized-{name}-output', result.stdout == expected and result.stderr == '')

check('no-external-aethc-called', not trap_record.exists())
metadata['complete'] = True
save()
print(f'Core bootstrap complete: {len(records)} checks; evidence {work / "results.json"}')
