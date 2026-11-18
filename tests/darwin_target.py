#!/usr/bin/env python3
"""Darwin deployment metadata, execution, driver precedence and diagnostics."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', default=['c', 'llvm'])
args = parser.parse_args()
if platform.system() != 'Darwin':
    print('Darwin target regression: skipped on non-Darwin host')
    raise SystemExit(0)
args.compiler = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
records = []
environment = os.environ.copy()
for key in ['SDKROOT', 'DEVELOPER_DIR', 'MACOSX_DEPLOYMENT_TARGET', 'CC']:
    environment.pop(key, None)
environment['TMPDIR'] = str(work)

def run(command, env=None, status=0):
    command = [str(arg) for arg in command]
    completed = subprocess.run(command, env=env or environment, capture_output=True,
                               text=True, timeout=30)
    record = dict(command=command, status=completed.returncode,
                  stdout=completed.stdout, stderr=completed.stderr)
    records.append(record)
    assert completed.returncode == status, record
    return completed

def version(value):
    return tuple((list(map(int, value.split('.'))) + [0, 0])[:3])

def minos(path):
    output = run(['/usr/bin/otool', '-l', path]).stdout
    found = re.search(r'cmd LC_BUILD_VERSION\n(?:.*\n)*?\s+minos ([0-9.]+)', output)
    if found is None:
        found = re.search(r'cmd LC_VERSION_MIN_MACOSX\n(?:.*\n)*?\s+version ([0-9.]+)', output)
    assert found is not None, output
    return version(found.group(1))

source = work / 'probe.aeth'
source.write_text('fn twice(n:i32)->i32{n*2}fn main()->i32{if twice(21)!=42{return 1;}return 0;}\n')
reference = work / 'reference.c'
reference.write_text('int main(void) { return 0; }\n')
arch = 'aarch64' if platform.machine() == 'arm64' else platform.machine()
# This is a test oracle, not a compiler subprocess: selected SDK discovery must
# produce the same default deployment as a direct native C compilation.
selected_sdk = Path(run(['/usr/bin/xcrun', '--sdk', 'macosx', '--show-sdk-path']).stdout.strip())
reference_obj = work / 'reference.o'
run(['/usr/bin/clang', '-c', reference, '-o', reference_obj])
default_minos = minos(reference_obj)

cases = [
    ('native', [], {}, default_minos),
    ('environment', [], {'MACOSX_DEPLOYMENT_TARGET': '13.0'}, version('13.0')),
    ('explicit', ['--target', f'{arch}-apple-macosx14.0'], {'MACOSX_DEPLOYMENT_TARGET': '13.0'}, version('14.0')),
    ('explicit_ignores_invalid_env', ['--target', f'{arch}-apple-macosx13.0'], {'MACOSX_DEPLOYMENT_TARGET': 'invalid'}, version('13.0')),
    ('unversioned_darwin', ['--target', f'{arch}-apple-darwin'], {'MACOSX_DEPLOYMENT_TARGET': '13.0'}, version('13.0')),
]
# Explicit SDKROOT selects the SDK, even when xcode-select points elsewhere.
other_sdk = Path('/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk')
if other_sdk.is_dir() and other_sdk.resolve() != selected_sdk.resolve():
    env = {**environment, 'SDKROOT': str(other_sdk)}
    reference_other = work / 'reference-other-sdk.o'
    run(['/usr/bin/clang', '-isysroot', other_sdk, '-c', reference, '-o', reference_other], env)
    cases.append(('sdkroot', [], {'SDKROOT': str(other_sdk)}, minos(reference_other)))

try:
    for backend in args.backends:
        for label, options, overrides, expected in cases:
            env = {**environment, **overrides}
            artifact = work / f'{backend}-{label}'
            obj = artifact.with_suffix('.o')
            common = [args.compiler, source, '--backend', backend, *options]
            run([*common, '--emit-obj', '-o', obj], env)
            linked = run([*common, '-o', artifact], env)
            assert minos(obj) == expected, (backend, label, minos(obj), expected)
            assert minos(artifact) == expected, (backend, label, minos(artifact), expected)
            assert 'built for newer' not in linked.stderr, linked.stderr
            run([artifact])

        # A real successful linker warning must not vanish, even in quiet mode.
        newer_c = work / 'newer.c'
        newer_c.write_text('int external_probe(void) { return 17; }\n')
        newer_obj = work / 'newer.o'
        run(['/usr/bin/clang', f'--target={arch}-apple-macosx14.0', '-c', newer_c, '-o', newer_obj])
        executable = work / f'{backend}-linker-warning'
        output = run([args.compiler, source, '--backend', backend, '--quiet', '--target',
                      f'{arch}-apple-macosx13.0', '--link-object', newer_obj, '-o', executable])
        assert 'built for newer' in output.stderr and '14.0' in output.stderr and '13.0' in output.stderr, output.stderr
        run([executable])

        # A selected C driver is invoked once; SDK discovery never probes it. Preserve a normal native CC invocation (no --target).
        cc_log = work / f'{backend}-cc-args.txt'
        cc_log.write_text('')
        wrapper = work / f'{backend}-custom-cc.sh'
        wrapper.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> ' + shlex.quote(str(cc_log)) + '\nexec /usr/bin/clang "$@"\n')
        wrapper.chmod(0o755)
        executable = work / f'{backend}-custom-cc'
        run([args.compiler, source, '--backend', backend, '--cc', wrapper, '-o', executable],
            {**environment, 'MACOSX_DEPLOYMENT_TARGET': '13.0'})
        argv = cc_log.read_text().splitlines()
        assert argv.count('-mmacosx-version-min=13.0') == 1, argv
        assert argv.count('-isysroot') == 1 and not any(arg.startswith('--target=') for arg in argv), argv
        actual_sdk = Path(argv[argv.index('-isysroot') + 1])
        assert actual_sdk.resolve() == selected_sdk.resolve(), (actual_sdk, selected_sdk)
        run([executable])

    # Rejection paths are compiler diagnostics, not linker failures or crashes.
    fake_ios = work / 'iPhoneOS.sdk'
    fake_ios.mkdir(exist_ok=True)
    (fake_ios / 'SDKSettings.json').write_text(json.dumps({'SupportedTargets': {'iphoneos': {'DefaultDeploymentTarget': '18.0'}}}))
    for label, options, overrides, expected in [
        ('invalid_env', [], {'MACOSX_DEPLOYMENT_TARGET': '13.invalid'}, 'invalid macOS deployment'),
        ('ios_sdk', [], {'SDKROOT': str(fake_ios)}, 'does not support a macOS target'),
        ('kernel_version', ['--target', f'{arch}-apple-darwin27.0'], {}, 'Darwin kernel triple'),
    ]:
        result = run([args.compiler, source, '--backend', args.backends[0], '--emit-obj', *options, '-o', work / f'{label}.o'],
                     {**environment, **overrides}, status=1)
        assert expected in result.stderr, result.stderr

    if 'llvm' in args.backends:
        linux_object = work / 'linux.o'
        run([args.compiler, source, '--backend', 'llvm', '--target', 'x86_64-unknown-linux-gnu', '--emit-obj', '-o', linux_object],
            {**environment, 'SDKROOT': str(fake_ios), 'MACOSX_DEPLOYMENT_TARGET': 'invalid'})
        assert linux_object.read_bytes()[:4] == b'\x7fELF'
finally:
    (work / 'results.json').write_text(json.dumps({
        'compiler': str(args.compiler), 'compiler_sha256': hashlib.sha256(args.compiler.read_bytes()).hexdigest(),
        'selected_sdk': str(selected_sdk), 'default_minos': default_minos,
        'cases': len(cases), 'records': records,
    }, indent=2))
print(f'Darwin deployment regression: {len(cases)} cases per backend, execution, warning propagation, custom CC and negative checks passed')
