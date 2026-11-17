#!/usr/bin/env python3
"""Build a local POSIX release and verify both Core bootstrap seeds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--output', type=Path, required=True, help='new local release directory')
p.add_argument('--work-dir', type=Path, required=True, help='persistent bootstrap evidence')
args = p.parse_args()
root = Path(__file__).resolve().parents[1]
work, out = args.work_dir.resolve(), args.output.resolve()
out.parent.mkdir(parents=True, exist_ok=True)
work.mkdir(parents=True, exist_ok=True)
if out.exists():
    p.error('output already exists; choose a new release directory')

def command(*args, env=None):
    subprocess.run(list(map(str, args)), cwd=root, env=env, check=True)

def output(*args):
    return subprocess.check_output(list(map(str, args)), cwd=root, text=True).strip()

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def source_commit():
    changes = output('git', 'status', '--porcelain', '--', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                     'src', 'stdlib', 'runtime', 'jxz', 'tests', 'scripts', 'install.sh', 'src_aether')
    if changes:
        sys.exit('Commit release inputs before packaging:\n' + changes)
    return output('git', 'rev-parse', 'HEAD')

commit = source_commit()
target = Path(os.environ.get('CARGO_TARGET_DIR', root/'target')).resolve()
env = dict(os.environ, CARGO_TARGET_DIR=str(target))
with tempfile.TemporaryDirectory(prefix='.aether-package-', dir=out.parent) as scratch:
    stage = Path(scratch)
    (stage/'stdlib').mkdir()
    for path in sorted((root/'stdlib').glob('*.aeth')):
        shutil.copy2(path, stage/'stdlib'/path.name)
    shutil.copy2(root/'install.sh', stage/'install.sh')
    for path in ('runtime/llvm/target_init.c', 'runtime/metal/bridge.m', 'runtime/cudaq/bridge.cpp'):
        dest = stage / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root/path, dest)
    checks, generated = {}, []
    for backend in ('c', 'llvm'):
        command('sh', 'scripts/build.sh', backend, env=env)
        seed = stage / ('aethc' if backend == 'llvm' else 'aethc-c')
        shutil.copy2(target/'release/aethc', seed)
        jxz = stage / ('jxz' if backend == 'llvm' else 'jxz-c')
        command('sh', 'jxz/build.sh', jxz, env={**env, 'AETHC': str(seed), 'AETHER_BACKEND': backend})
        command(jxz, '--version')
        boot = work / ('bootstrap-' + backend)
        command(sys.executable, 'tests/bootstrap_core.py', '--backend', backend, '--compiler', seed, '--work-dir', boot, env=env)
        meta = json.loads((boot/'results.json').read_text())
        if not meta['metadata']['complete'] or meta['metadata']['seed_sha256'] != digest(seed):
            sys.exit('bootstrap receipt does not match release compiler')
        checks[backend] = dict(seed=digest(seed), checks=len(meta['checks']), stage4=digest(boot/'stage4'), results=str(boot/'results.json'))
        generated.append(digest(boot/'stage4.c'))
    if generated[0] != generated[1] or source_commit() != commit:
        sys.exit('bootstrap fixed point or source identity changed')
    shutil.copy2(work/'bootstrap-llvm/stage4', stage/'aethc-core')
    libs = {str(f.relative_to(stage)): digest(f) for f in sorted((stage/'stdlib').iterdir())}
    files = [dict(name=str(f.relative_to(stage)), bytes=f.stat().st_size, sha256=digest(f))
             for f in sorted(stage.rglob('*')) if f.is_file() and f.parent != stage/'stdlib']
    deps = ['otool', '-L'] if sys.platform == 'darwin' else ['ldd']
    deps = output(*deps, stage/'aethc').splitlines()
    manifest = dict(release_id=out.name, source_commit=commit, platform=dict(system=platform.system(), machine=platform.machine()),
                    compiler_features=['llvm', 'c'], artifacts=files, stdlib=dict(files=libs),
                    bootstrap=checks, core_generated_c_sha256=generated[0], rust=output('rustc', '--version'),
                    clang=output('clang', '--version').splitlines()[0], dynamic_dependencies=deps[1:] if sys.platform == 'darwin' else deps)
    receipt = stage/'manifest.json'
    receipt.write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+'\n')
    checksum = digest(receipt)
    stage.rename(out)
print(f'{checksum}  {out}/manifest.json')
