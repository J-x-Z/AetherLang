#!/usr/bin/env python3
"""Exercise verified, interruptible side-by-side package installation."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--work-dir', type=Path, required=True)
args = p.parse_args()
root = Path(__file__).resolve().parents[1]
args.work_dir.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(dir=args.work_dir.resolve()))
prefix = work / 'prefix Ω'
records = []

def package(version):
    base = work / version
    (base / 'stdlib').mkdir(parents=True)
    artifacts = []
    for name in ('aethc', 'jxz'):
        path = base / name
        path.write_text(f'#!/bin/sh\necho {version}\n')
        path.chmod(0o755)
        artifacts.append(dict(name=name, sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
    module = base / 'stdlib/vec.aeth'
    module.write_text(version)
    manifest = dict(release_id=version, platform=dict(system=platform.system(), machine=platform.machine()),
                    artifacts=artifacts, stdlib=dict(files={'stdlib/vec.aeth': hashlib.sha256(module.read_bytes()).hexdigest()}))
    (base / 'manifest.json').write_text(json.dumps(manifest))
    return base

def invoke(label, base, expected=0, env=None, dest=prefix, extra=()):
    r = subprocess.run([root / 'install.sh', '--package', base, '--prefix', dest, *extra],
                       capture_output=True, text=True, env=env, timeout=20)
    records.append(dict(label=label, exit=r.returncode, stderr=r.stderr))
    assert r.returncode == expected, (label, r.stderr)

def check(version):
    assert subprocess.check_output([prefix / 'bin/aethc'], text=True).strip() == version
    assert subprocess.check_output([prefix / 'bin/jxz'], text=True).strip() == version
    assert (prefix / 'bin/stdlib/vec.aeth').read_text() == version

one, two = package('one'), package('two')
invoke('install', one, extra=['--manifest-sha256', hashlib.sha256((one/'manifest.json').read_bytes()).hexdigest()])
check('one')
invoke('mismatched-receipt', two, 1, extra=['--manifest-sha256', '0'*64])
check('one')
(two/'stdlib/vec.aeth').write_text('damaged')
invoke('damaged-payload', two, 1)
check('one')
(two/'stdlib/vec.aeth').write_text('two')
hook = work / 'fault'
hook.mkdir()
(hook / 'sitecustomize.py').write_text(
    'import os, signal\noriginal=os.replace\n'
    'def replace(a,b):\n if str(b).endswith("/bin"): os.kill(os.getpid(), signal.SIGKILL)\n return original(a,b)\n'
    'os.replace=replace\n')
invoke('killed-before-activation', two, -signal.SIGKILL, {**os.environ, 'PYTHONPATH': str(hook)})
check('one')
invoke('recover-and-upgrade', two)
check('two')
invoke('idempotent', two)
check('two')
invoke('roll-back-to-verified-version', one)
check('one')
foreign = work / 'unmanaged'
(foreign/'bin').mkdir(parents=True)
(foreign/'bin/user-data').write_bytes(b'keep')
invoke('preserve-existing-bin', two, 1, dest=foreign)
assert (foreign/'bin/user-data').read_bytes() == b'keep'
(args.work_dir / 'results.json').write_text(json.dumps(records, indent=2)+'\n')
print(f'{len(records)} package installation and rollback scenarios passed')
