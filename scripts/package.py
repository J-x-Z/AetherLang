#!/usr/bin/env python3
"""Build a local POSIX release and verify both Core bootstrap seeds."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile

from build_selfhost import build as build_selfhost

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--output', type=Path, required=True, help='new local release directory')
p.add_argument('--work-dir', type=Path, required=True, help='persistent bootstrap evidence')
p.add_argument('--bundle-linux-llvm', action='store_true',
               help='include the Linux compiler LLVM shared library and verify its relative RUNPATH')
args = p.parse_args()
if args.bundle_linux_llvm and sys.platform != 'linux':
    p.error('--bundle-linux-llvm requires a native Linux host')
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
    sha = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            sha.update(chunk)
    return sha.hexdigest()

def bundle_linux_llvm(seed):
    # Use the actual SONAME selected by the linker, including distro suffixes.
    deps = output('ldd', seed)
    matches = re.findall(r'^\s*(libLLVM[^/\s]*\.so[^/\s]*) => (/.+?) \(0x[0-9a-f]+\)$', deps, re.M)
    if len(matches) != 1:
        sys.exit(f'expected one resolved LLVM shared library for {seed}:\n{deps}')
    name, source = matches[0]
    library = seed.parent / 'lib' / name
    library.parent.mkdir()
    shutil.copy2(source, library)
    clean_env = {k: v for k, v in os.environ.items() if k != 'LD_LIBRARY_PATH'}
    relocated = subprocess.check_output(['ldd', seed], env=clean_env, text=True)
    if f'{name} => {library} (' not in relocated or 'not found' in relocated:
        sys.exit(f'packaged compiler dependencies could not be resolved locally:\n{relocated}')
    command(seed, '--version', env=clean_env)
    return relocated.replace(str(seed.parent), '$PACKAGE').splitlines()

def source_commit():
    changes = output('git', 'status', '--porcelain', '--', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                     'src', 'stdlib', 'runtime', 'jxz', 'tests', 'scripts', 'install.sh', 'src_aether', 'tools/mcp')
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
    shutil.copy2(root/'tools/mcp/aether-mcp', stage/'aether-mcp')
    for path in ('runtime/llvm/target_init.c', 'runtime/metal/bridge.m', 'runtime/metal/bridge.h',
                 'runtime/metal/provider.m', 'runtime/compute/provider.h', 'runtime/compute/loader.c',
                 'runtime/vulkan/provider.c', 'runtime/vulkan/gemm.comp', 'scripts/build_vulkan.py',
                 'runtime/cudaq/bridge.cpp', 'runtime/cudaq/quake.py', 'scripts/build_cudaq.py',
                 'tools/mcp/aether_mcp.py', 'tools/mcp/queries.py'):
        dest = stage / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root/path, dest)
    checks, generated = {}, []
    relocated_deps = None
    for backend in ('c', 'llvm'):
        build_env = env
        if backend == 'llvm' and args.bundle_linux_llvm:
            build_env = dict(env)
            flag = '-Clink-arg=-Wl,-rpath,$ORIGIN/lib'
            if 'CARGO_ENCODED_RUSTFLAGS' in build_env:
                build_env['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(filter(None, [build_env['CARGO_ENCODED_RUSTFLAGS'], flag]))
            else:
                build_env['RUSTFLAGS'] = ' '.join(filter(None, [build_env.get('RUSTFLAGS'), flag]))
        command('sh', 'scripts/build.sh', backend, env=build_env)
        seed = stage / ('aethc' if backend == 'llvm' else 'aethc-c')
        shutil.copy2(target/'release/aethc', seed)
        if backend == 'llvm' and args.bundle_linux_llvm:
            relocated_deps = bundle_linux_llvm(seed)
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
    native = stage / 'aethc-native'
    regular = build_selfhost(stage / 'aethc-core', work/'regular', native, 'llvm')
    for backend in ('c', 'llvm'):
        jxz_native = stage / ('jxz-native-c' if backend == 'c' else 'jxz-native')
        command('sh', 'jxz/build.sh', jxz_native,
                env={**env, 'AETHC': str(native), 'AETHER_BACKEND': backend})
        command(jxz_native, '--version')
    if source_commit() != commit:
        sys.exit('source identity changed during the regular compiler build')
    (work/'regular-bootstrap.json').write_text(json.dumps(regular, indent=2)+'\n')
    libs = {str(f.relative_to(stage)): digest(f) for f in sorted((stage/'stdlib').iterdir())}
    files = [dict(name=str(f.relative_to(stage)), bytes=f.stat().st_size, sha256=digest(f))
             for f in sorted(stage.rglob('*')) if f.is_file() and f.parent != stage/'stdlib']
    deps = ['otool', '-L'] if sys.platform == 'darwin' else ['ldd']
    deps = relocated_deps or output(*deps, stage/'aethc').splitlines()
    manifest = dict(release_id=out.name, source_commit=commit, platform=dict(system=platform.system(), machine=platform.machine()),
                    compiler_features=['llvm', 'c'], artifacts=files, stdlib=dict(files=libs),
                    bootstrap=checks, regular_compiler={key: value for key, value in regular.items() if key != 'commands'}, core_generated_c_sha256=generated[0], rust=output('rustc', '--version'),
                    clang=output('clang', '--version').splitlines()[0], dynamic_dependencies=deps[1:] if sys.platform == 'darwin' else deps)
    receipt = stage/'manifest.json'
    receipt.write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+'\n')
    checksum = digest(receipt)
    stage.rename(out)
print(f'{checksum}  {out}/manifest.json')
