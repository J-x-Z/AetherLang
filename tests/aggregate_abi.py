#!/usr/bin/env python3
"""First-class aggregates, saturating casts, packed accesses and bidirectional C ABI.

All outputs stay under --work-dir. Requires the existing host C compiler; no
Cargo invocation, downloads, or emulated claims for other architectures.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
args = parser.parse_args()
root = Path(__file__).resolve().parent / 'regressions/aggregates'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work)}
compiler = args.compiler.resolve()
records = []
cc = os.environ.get('CC', 'clang')

def run(label, command, environment=env, unsupported=None):
    result = subprocess.run(list(map(str, command)), env=environment, capture_output=True, text=True, timeout=30)
    records.append(dict(label=label, command=list(map(str, command)), status=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, unsupported=unsupported))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    if unsupported:
        assert result.returncode != 0 and unsupported in result.stderr, f'{label}: expected target ABI diagnostic, got {result.stderr}'
        return result
    if result.returncode:
        raise AssertionError(f'{label}: exit {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result

for name in ['values', 'packed', 'saturating_casts', 'abi']:
    run(f'{name}/check', [compiler, 'check', root / f'{name}.aeth'])

obj = work / 'native-abi.o'
run('native C ABI companion', [cc, '-std=c11', '-O2', '-c', root / 'abi.c', '-o', obj])
for backend in args.backends:
    for opt in [0, 2]:
        for name in ['values', 'packed', 'saturating_casts', 'abi']:
            # LLVM's target ABI classifier is intentionally target-specific.
            binary = work / f'{name}-{backend}-O{opt}'
            extra = ['--link-object', obj] if name == 'abi' else []
            unsupported = 'aggregate C ABI lowering is currently implemented for Apple arm64' if backend == 'llvm' and name == 'abi' and not (platform.system() == 'Darwin' and platform.machine() == 'arm64') else None
            run(f'{name}/{backend}/O{opt}/build', [compiler, 'build', root / f'{name}.aeth', '--backend', backend, f'-O{opt}', '-o', binary, *extra], unsupported=unsupported)
            if unsupported:
                continue
            output = run(f'{name}/{backend}/O{opt}/run', [binary])
            assert output.stdout == '' and output.stderr == '', f'{binary}: unexpected output'

# Inlining used to hide the returning-local-array bug. Keep O0 and noinline;
# enable use-after-return plus alignment UB checks for nested packed storage.
for name in ['values', 'packed', 'saturating_casts', 'abi']:
    source, binary = work / f'{name}-sanitized.c', work / f'{name}-sanitized'
    run(f'{name}/emit C', [compiler, root / f'{name}.aeth', '--emit-c', '-o', source])
    extra = [root / 'abi.c'] if name == 'abi' else []
    run(f'{name}/sanitizer/build', [cc, '-std=c11', '-O0', '-g', '-fno-inline', '-fsanitize=address,undefined', '-fno-omit-frame-pointer', source, *extra, '-o', binary])
    run(f'{name}/sanitizer/run', [binary], {**env, 'ASAN_OPTIONS': 'detect_stack_use_after_return=1', 'UBSAN_OPTIONS': 'halt_on_error=1'})
print(f'{len(records)} checks passed; evidence: {work / "results.json"}')
