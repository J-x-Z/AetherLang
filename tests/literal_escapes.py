#!/usr/bin/env python3
"""Literal decoding, byte-preserving code generation and located rejection."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--levels', nargs='+', choices=['0', '2', '3'], default=['0', '2', '3'])
parser.add_argument('--core', type=Path, help='Optional standalone Core compiler; validates shared string syntax')
args = parser.parse_args()
compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
fixtures = Path(__file__).resolve().parent / 'regressions/literals'
env = {**os.environ, 'TMPDIR': str(work)}
records = []
failed = False
metadata = {'compiler': str(compiler),
            'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest()}

# Oracles follow the language semantics, independently of generated C/LLVM.
nul_digits = b''.join(bytes([0, digit]) for digit in range(48, 56))
expected = {
    'strings.aeth': (bytes([10, 13, 9, 92, 34, 39, 0]) + b'\x1b'
                     + b'\x1b[31mred\x1b[0m\n' + nul_digits * 3
                     + b'AA' + b'AF' + b'A1' + bytes([127, 0, 90])
                     + 'Aé中😀\U0010ffff'.encode('utf-8')
                     + b'\0' + 'é中😀'.encode('utf-8') + b'\0'),
    'characters.aeth': (b'10,13,9,92,34,39,0,27,65,127,233,20013,128512,1114111,233,128512\n'
                        b'128512,128512,255,-1,-1\n'),
    'script.ath': (b'common:\n\r\t\\"\'\n' + b'\x1b\n' + b'\x1b[31mred\x1b[0m\n'
                   + b'AAAFA1\n' + 'Aé中😀\U0010ffff\nraw:é中😀\n'.encode('utf-8') + b'nul:\n'),
}


def record(label, command, validator, stdout_expected=None):
    global failed
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, timeout=30)
    except subprocess.TimeoutExpired as error:
        result = subprocess.CompletedProcess(command, 124, error.stdout or b'', error.stderr or b'timed out')
    try:
        passed = bool(validator(result))
        explanation = ''
    except (AssertionError, ValueError, KeyError, TypeError, UnicodeDecodeError) as error:
        passed = False
        explanation = str(error)
    item = dict(label=label, command=command, exit=result.returncode,
                stdout_hex=result.stdout.hex(), stderr_hex=result.stderr.hex(),
                stderr=result.stderr.decode('utf-8', errors='replace'), passed=passed)
    if stdout_expected is not None:
        item['expected_stdout_hex'] = stdout_expected.hex()
    if explanation:
        item['validation_error'] = explanation
    records.append(item)
    failed |= not passed
    (work / 'results.json').write_text(json.dumps({**metadata, 'records': records},
                                                 ensure_ascii=False, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {label}', flush=True)
    if not passed:
        print(item['stderr'] or explanation or f'stdout bytes: {item["stdout_hex"]}', flush=True)
    return passed


def successful(result):
    return result.returncode == 0


def output_is(value):
    return lambda result: result.returncode == 0 and result.stdout == value and result.stderr == b''


for filename, stdout in expected.items():
    source = fixtures / filename
    for backend in args.backends:
        for level in args.levels:
            label = f'{source.stem}/{backend}/O{level}'
            binary = work / f'{source.stem}-{backend}-O{level}'
            built = record(label + '/build', [compiler, 'build', source, '--backend', backend,
                                              f'-O{level}', '-o', binary], successful)
            if built:
                record(label + '/run', [binary], output_is(stdout), stdout)


invalid = {
    'unknown-q': r'\q', 'unknown-e': r'\e', 'unknown-a': r'\a',
    'hex-empty': r'\x', 'hex-short': r'\x1', 'hex-nonhex': r'\xGG',
    'hex-nonascii-80': r'\x80', 'hex-nonascii-ff': r'\xFF',
    'unicode-no-brace': r'\u1234', 'unicode-empty': r'\u{}',
    'unicode-nonhex': r'\u{GG}', 'unicode-unclosed': r'\u{123',
    'unicode-long': r'\u{0000041}', 'unicode-surrogate-first': r'\u{d800}',
    'unicode-surrogate-last': r'\u{dfff}', 'unicode-too-large': r'\u{110000}',
    'eof-backslash': '\\', 'eof-string': 'unterminated',
}


def invalid_source(mode, name, literal):
    # Valid multibyte text precedes the bad token on the same line, so byte
    # offsets cannot accidentally serve as human character columns.
    if mode == 'script':
        prefix = 'def main():\n    print("é中😀" + '
        quote, suffix = '"', ')\n    return 0\n'
    else:
        prefix = 'fn main() -> i32 {\n    let prefix: *u8 = "é中😀" as *u8; '
        prefix += 'let value: u32 = ' if mode == 'char' else 'let value: *u8 = '
        quote = "'" if mode == 'char' else '"'
        suffix = ' as u32;\n    0\n}\n' if mode == 'char' else ' as *u8;\n    0\n}\n'
    text = prefix + quote + literal
    if not name.startswith('eof-'):
        text += quote + suffix
    return text, len(prefix.encode('utf-8')), len((prefix + quote + literal).encode('utf-8'))


def located_rejection(path, text, token_start, token_end):
    raw = text.encode('utf-8')
    def validate(result):
        assert result.returncode == 1, f'expected diagnostic exit 1, got {result.returncode}'
        diagnostic = json.loads(result.stderr)
        assert diagnostic.get('code'), 'missing diagnostic code'
        location = diagnostic['location']
        assert location, 'missing source location'
        assert Path(location['file']).resolve() == path
        start, end = location['start'], location['end']
        assert 0 <= start <= end <= len(raw), f'invalid byte span: {location}'
        before = raw[:start].decode('utf-8')
        before_end = raw[:end].decode('utf-8')
        assert location['line'] == before.count('\n') + 1
        assert location['column'] == len(before.rsplit('\n', 1)[-1]) + 1
        assert location['end_line'] == before_end.count('\n') + 1
        assert location['end_column'] == len(before_end.rsplit('\n', 1)[-1]) + 1
        # Either the whole invalid literal or the offending escape may be
        # highlighted. A point at EOF is valid for unterminated input.
        assert token_start <= start <= token_end, f'location misses bad literal: {location}'
        return True
    return validate


negative_strings = []
for mode in ['string', 'char', 'script']:
    for name, literal in invalid.items():
        # An unterminated alphabetic char token can denote a lifetime. The
        # backslash-at-EOF case still tests the character decoder's EOF path.
        if mode == 'char' and name == 'eof-string':
            continue
        text, start, end = invalid_source(mode, name, literal)
        source = work / f'invalid-{mode}-{name}.{ "ath" if mode == "script" else "aeth" }'
        source.write_text(text)
        validate = located_rejection(source, text, start, end)
        record(f'{mode}/{name}/check', [compiler, 'check', source, '--error-format', 'json'], validate)
        for backend in args.backends:
            record(f'{mode}/{name}/{backend}/build', [compiler, 'build', source, '--backend', backend,
                                                    '--error-format', 'json', '-o', work / 'invalid-output'], validate)
        if mode == 'string':
            negative_strings.append(source)

if args.core:
    core = args.core.resolve()
    source = fixtures / 'strings.aeth'
    generated = work / 'core-strings.c'
    if record('core/strings/emit', [core, source, '-o', generated], successful):
        for level in args.levels:
            binary = work / f'core-strings-O{level}'
            if record(f'core/strings/O{level}/build', [env.get('CC', 'cc'), generated,
                                                    f'-O{level}', '-o', binary], successful):
                record(f'core/strings/O{level}/run', [binary], output_is(expected['strings.aeth']), expected['strings.aeth'])
    for source in negative_strings:
        record('core/' + source.stem + '/check', [core, source, '--check'],
               lambda result: result.returncode == 1 and str(source).encode('utf-8') in result.stdout + result.stderr)

print(f'{sum(item["passed"] for item in records)}/{len(records)} checks passed; {work / "results.json"}')
raise SystemExit(1 if failed else 0)
