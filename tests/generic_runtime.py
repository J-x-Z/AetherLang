#!/usr/bin/env python3
"""Concrete generics, payload enums and checked resource/error chains."""
import argparse
import json
import os
from pathlib import Path
import resource
import signal
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
parser.add_argument('--levels', nargs='+', choices=['0', '2'], default=['0', '2'])
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
source = root / 'tests/regressions/generics'
env = {**os.environ, 'TMPDIR': str(work)}
records = []


def run(command, **kwargs):
    return subprocess.run(list(map(str, command)), capture_output=True, text=True,
                          timeout=30, env=kwargs.pop('env', env), **kwargs)


def no_file_bytes():
    signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (0, 0))


library = work / ('realloc_failure.dylib' if sys.platform == 'darwin' else 'realloc_failure.so')
flags = ['-dynamiclib'] if sys.platform == 'darwin' else ['-shared', '-fPIC']
build = run([os.environ.get('CC', 'cc'), *flags, source / 'realloc_failure.c', '-o', library,
             *([] if sys.platform == 'darwin' else ['-ldl'])])
if build.returncode:
    raise SystemExit(build.stderr)
failed = False
for name, diagnostic in [('unknown_error_type', "concrete type for 'E'"),
                         ('conflicting_arguments', 'Type mismatch'),
                         ('wrong_instantiated_body', 'Type mismatch'),
                         ('wrong_payload', 'Type mismatch')]:
    checked = run([compiler, 'check', source / 'invalid' / f'{name}.aeth'])
    valid = checked.returncode != 0 and diagnostic in checked.stderr
    records.append(dict(name=name, phase='check-rejected', exit=checked.returncode,
                        stdout=checked.stdout, stderr=checked.stderr, passed=valid))
    print(f'{"PASS" if valid else "FAIL"} reject {name}')
    failed |= not valid
for backend in args.backends:
    for level in args.levels:
        for name in ['containers', 'results', 'file_result', 'allocation_failure', 'zero_sized']:
            binary = work / f'{name}-{backend}-O{level}'
            build = run([compiler, 'build', source / f'{name}.aeth', '--backend', backend,
                         f'-O{level}', '-o', binary])
            if build.returncode:
                records.append(dict(name=name, backend=backend, level=level, phase='build',
                                    exit=build.returncode, stderr=build.stderr, stdout=build.stdout))
                print(f'FAIL build {name}/{backend}/O{level}: {build.stderr}', file=sys.stderr)
                failed = True
                continue
            with tempfile.TemporaryDirectory(prefix='generic runtime ', dir=work) as temp:
                directory = Path(temp)
                if name == 'file_result':
                    cases = [('success', [directory / 'out', 'success'], {}),
                             ('open-error', [directory / 'absent' / 'out', 'open'], {}),
                             ('flush-error', [directory / 'full', 'flush'], {'preexec_fn': no_file_bytes})]
                elif name == 'allocation_failure':
                    injection = 'DYLD_INSERT_LIBRARIES' if sys.platform == 'darwin' else 'LD_PRELOAD'
                    cases = [('realloc-error', [], {'env': {**env, injection: str(library)}})]
                else:
                    cases = [(name, [], {})]
                for case, command_args, options in cases:
                    result = run([binary, *command_args], **options)
                    valid = result.returncode == 0 and result.stdout == '' and result.stderr == ''
                    if name == 'file_result' and case == 'success':
                        valid = valid and (directory / 'out').read_bytes() == b'payload'
                    if name == 'file_result' and case == 'flush-error':
                        valid = valid and (directory / 'full').stat().st_size == 0
                    records.append(dict(name=case, backend=backend, level=level, phase='run',
                                        exit=result.returncode, stdout=result.stdout, stderr=result.stderr,
                                        passed=valid))
                    print(f'{"PASS" if valid else "FAIL"} {case}/{backend}/O{level}')
                    failed |= not valid
(work / 'results.json').write_text(json.dumps(records, indent=2) + '\n')
raise SystemExit(1 if failed else 0)
