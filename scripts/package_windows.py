#!/usr/bin/env python3
"""Freeze one Git revision and cross-build a portable Windows acceptance bundle.

No Windows image is executed. Existing native seeds and Zig caches are reused.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

from windows_support import inspect_pe

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', required=True, type=Path, help='existing native Aether-built seed')
p.add_argument('--output', required=True, type=Path, help='new bundle directory; never overwritten')
p.add_argument('--work-dir', required=True, type=Path, help='cross-build logs and intermediates')
p.add_argument('--revision', default='HEAD', help='committed source revision, resolved once')
p.add_argument('--zig', default='zig')
p.add_argument('--zig-cache', type=Path, help='reuse an existing Zig global cache')
a = p.parse_args()
repo = Path(__file__).resolve().parents[1]
out, base, seed = a.output.resolve(), a.work_dir.resolve(), a.compiler.resolve(strict=True)
if out.exists():
    p.error('output already exists; choose a new bundle directory')
out.parent.mkdir(parents=True, exist_ok=True)
base.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix='windows-package-', dir=base))
commit = subprocess.check_output(['git', 'rev-parse', '--verify', a.revision+'^{commit}'], cwd=repo, text=True).strip()
source = work/'source'
source.mkdir()
suite_files = ['scripts/bootstrap_windows.py', 'scripts/bootstrap_jxz_windows.py',
               'scripts/windows_support.py', 'tests/bootstrap_windows_native.py',
               'tests/bootstrap_lsp.py', 'jxz/tests/windows_native.py']
# Rebuilding a historical revision retains that revision's test payload.
csv_files = ['tests/windows_csv.py', 'tests/regressions/platform/windows_stdio_binary.aeth',
             'examples/analysis/regress.aeth']
has_csv_suite = subprocess.check_output(
    ['git', 'ls-tree', '--name-only', commit, '--', csv_files[0]], cwd=repo, text=True
).strip() == csv_files[0]
if has_csv_suite:
    suite_files.extend(csv_files)
snapshot_fixture = 'tests/regressions/large_value_snapshot.aeth'
has_snapshot_fixture = bool(subprocess.check_output(
    ['git', 'ls-tree', '--name-only', commit, '--', snapshot_fixture], cwd=repo))
if has_snapshot_fixture:
    suite_files.append(snapshot_fixture)
paths = ['src_aether/core', 'jxz/src', 'stdlib', *suite_files]
archive = subprocess.check_output(['git', 'archive', '--format=tar', commit, *paths], cwd=repo)
with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
    for item in tar:
        path = Path(item.name)
        if path.is_absolute() or '..' in path.parts or not (item.isdir() or item.isfile()):
            raise RuntimeError('unexpected source archive entry: '+item.name)
        dest = source/path
        if item.isdir():
            dest.mkdir(parents=True, exist_ok=True)
        else:
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(tar.extractfile(item).read())

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

report = dict(source_commit=commit, native_windows_executed=False, status='in-progress',
              seed=dict(path=str(seed), sha256=sha(seed)), jobs=[])
def save():
    (work/'results.json').write_text(json.dumps(report, indent=2)+'\n')
    (base/'latest.json').write_text(json.dumps(dict(run_dir=str(work), results=str(work/'results.json')), indent=2)+'\n')
save()
env = {**os.environ, 'TMPDIR':str(work)}
zig = shutil.which(a.zig)
if not zig:
    p.error('Zig was not found; supply --zig with the existing executable')
report['zig'] = subprocess.check_output([zig, 'version'], text=True).strip()
cache = (a.zig_cache or base/'zig-cache').resolve()
cache.mkdir(parents=True, exist_ok=True)

def run(name, command):
    start = time.monotonic()
    with (work/(name+'.stdout.log')).open('wb') as stdout, (work/(name+'.stderr.log')).open('wb') as stderr:
        result = subprocess.run(list(map(str, command)), env=env, cwd=source, stdout=stdout, stderr=stderr, timeout=1200)
    report['jobs'].append(dict(name=name, command=list(map(str,command)), status=result.returncode,
                               seconds=time.monotonic()-start))
    save()
    if result.returncode:
        report['status']='failed: '+name
        save()
        raise SystemExit(f'{name} failed; see {work}/{name}.stderr.log and .stdout.log')

cross = {}
for name, script in [('compiler','bootstrap_windows.py'),('jxz','bootstrap_jxz_windows.py')]:
    folder = work/name
    folder.mkdir()
    (folder/'zig-cache').symlink_to(cache, target_is_directory=True)
    run(name, [sys.executable, source/'scripts'/script, '--compiler',seed,'--work-dir',folder,'--zig',zig])
    selected = Path(json.loads((folder/'latest.json').read_text())['run_dir'])
    receipt = json.loads((selected/'results.json').read_text())
    if receipt['native_windows_executed'] or any(x['status'] for x in receipt['checks']):
        raise RuntimeError('invalid cross-build receipt')
    cross[name] = (selected,receipt)

# Publish only after both independent cross-link matrices and PE audits pass.
stage = Path(tempfile.mkdtemp(prefix='.'+out.name+'-', dir=out.parent))
try:
    for name, exe in [('compiler','aethc.exe'),('jxz','jxz.exe')]:
        shutil.copy2(cross[name][0]/exe, stage/exe)
    shutil.copytree(source/'stdlib',stage/'stdlib')
    # Install the frozen revision's Windows stdio implementation when present.
    windows_stdio = source/'stdlib/windows/stdio_binary.aeth'
    if windows_stdio.is_file():
        shutil.copy2(windows_stdio, stage/'stdlib/stdio_binary.aeth')
    shutil.copytree(cross['compiler'][0]/'source',stage/'source/core')
    shutil.copytree(cross['jxz'][0]/'source',stage/'source/jxz')
    for path in suite_files:
        dest=stage/path;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source/path,dest)
    # These packaging/acceptance wrappers are hashed independently of the frozen
    # language source, so a later packaging fix never mislabels its source commit.
    for name in ['package_windows.py','validate_windows.py']:
        shutil.copy2(repo/'scripts'/name,stage/'scripts'/name)
    shutil.copy2(repo/'scripts/windows_native.ps1',stage/'Test-Native.ps1')
    (stage/'evidence').mkdir()
    for name,(selected,receipt) in cross.items():
        # Keep absolute build paths and raw user/tool output in the local work directory.
        portable = {key:receipt[key] for key in
                    ('status','native_windows_executed','seed_sha256','source_hashes')}
        portable['target'] = 'x86_64-windows-gnu'
        portable['checks'] = [{'label':step['label'], 'status':step['status']}
                              for step in receipt['checks']]
        portable['products'] = [dict(product, file=Path(product['file']).name)
                                for product in receipt['products']]
        portable['artifact'] = Path(receipt['artifact']).name
        (stage/'evidence'/f'{name}-cross.json').write_text(json.dumps(portable,indent=2)+'\n')
    products=[inspect_pe(stage/'aethc.exe',{'CreateProcessW','GetCommandLineW','_errno'}),
              inspect_pe(stage/'jxz.exe',{'CreateProcessW','FindFirstFileW','CreateHardLinkW','_errno'})]
    for product in products:
        product['file']=Path(product['file']).name
    # Only Windows-provided DLLs are allowed; reject unbundled support runtimes.
    allowed={'kernel32.dll','advapi32.dll','bcrypt.dll','shell32.dll','msvcrt.dll','ucrtbase.dll','ntdll.dll','user32.dll'}
    for product in products:
        unexpected=[x['dll'] for x in product['imports'] if x['dll'].lower() not in allowed and not x['dll'].lower().startswith(('api-ms-win-','ext-ms-win-'))]
        if unexpected:
            raise RuntimeError('unbundled runtime DLL dependencies: '+repr(unexpected))
    (stage/'README-WINDOWS.md').write_text('''# Windows x64 native acceptance bundle

This x86-64 bundle contains a self-hosted Aether compiler and JXZ. Run the
entry below on Windows to verify this copy with your native toolchain.
Copy the whole directory to a writable local Windows x64 path (NTFS recommended).
Do not copy just the EXEs: the adjacent stdlib and frozen test sources are required.

Prerequisites, installed separately (this bundle downloads nothing):
- 64-bit Windows 10/11, PowerShell 5.1 or newer.
- Python 3.10+ (`python.exe`, no third-party Python packages).
- A native x64 clang executable with its linker, C runtime headers/libraries and
  Windows import libraries already configured. A complete llvm-mingw UCRT x86_64
  toolchain is suitable; bare LLVM without its runtime/SDK is insufficient.
  Use its x86_64-w64-mingw32-clang.exe GNU-style driver, not clang-cl.exe.
  Both C and LLVM IR inputs must link successfully; clang 17+ is recommended.
  The chosen executable must work without extra command-line target flags.
- A writable work drive with at least 1 GiB free. Tests use only their new work
  directory; no administrator privileges, registry services or network are needed.

From PowerShell, substitute your actual installed executable paths:

```powershell
.\\Test-Native.ps1 -Clang 'D:\\Tools\\llvm-mingw\\bin\\x86_64-w64-mingw32-clang.exe' -Python 'C:\\Python312\\python.exe' -WorkDir 'D:\\Aether-native-results'
```

If local execution policy blocks scripts, use a process-scoped invocation after
reviewing the script: `powershell -NoProfile -ExecutionPolicy Bypass -File .\\Test-Native.ps1 ...`.
The entry point verifies every SHA-256, probes the actual compiler/runtime by
building and running an x64 C program, then runs the existing compiler and JXZ
Windows suites serially. It stops at the first failure and preserves logs and
`results.json`. Compiler tests include C/LLVM O0/O2, COFF, source diagnostics,
output preservation, two native self-hosted stages, same-source LLVM fixed point
and LSP. JXZ tests include local dependencies, build/run/test/clean, exit codes,
locking, offline cache integrity and project LSP. Bundles with the CSV suite also
verify streaming statistics through files and redirected standard input. `native_windows_executed` is
recorded only by this Windows-native entry point; a cross-build is not acceptance.

`manifest.json` records the source commit, selected-platform source hashes and PE
imports. `SHA256SUMS` also covers the manifest. These detect accidental corruption,
not authenticity against a party able to replace the whole bundle. Read the
per-module documentation before using platform-specific standard-library APIs;
bundling a module does not establish that its POSIX or GPU APIs run on Windows.

To rebuild on a development host with the repository and existing Aether seed:
`python3 scripts/package_windows.py --revision COMMIT --compiler SEED --zig ZIG --zig-cache CACHE --work-dir WORK --output NEW_PACKAGE`.
The output must be new, and the source snapshot comes only from that committed
revision. No Cargo, downloads, Windows execution or Wine are used by packaging.
''',encoding='utf-8')
    files=[dict(name=f.relative_to(stage).as_posix(), bytes=f.stat().st_size, sha256=sha(f))
           for f in sorted(stage.rglob('*')) if f.is_file()]
    manifest=dict(format='aether-windows-native-bundle-1', source_commit=commit,
                  target='x86_64-windows-gnu', native_windows_executed=False,
                  status='cross-linked; Windows-native acceptance pending', seed={'sha256':report['seed']['sha256']}, zig=report['zig'],
                  platform_selection={'source/core/host_platform.aeth':'platform_windows.aeth',
                                      'source/jxz/host.aeth':'host_windows.aeth'},
                  products=products, files=files)
    if windows_stdio.is_file():
        manifest['platform_selection']['stdlib/stdio_binary.aeth'] = 'stdlib/windows/stdio_binary.aeth'
    (stage/'manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
    checksums=[f"{x['sha256']}  {x['name']}" for x in files]
    checksums.append(sha(stage/'manifest.json')+'  manifest.json')
    (stage/'SHA256SUMS').write_text('\n'.join(checksums)+'\n')
    stage.rename(out)
except BaseException:
    report['status']='package publication failed; unpublished staging retained at '+str(stage)
    save()
    raise
report.update(status='cross-built and packaged; native Windows acceptance pending', output=str(out),
              manifest_sha256=sha(out/'manifest.json'), files=len(files), bytes=sum(x['bytes'] for x in files))
save()
print(json.dumps(dict(output=str(out),source_commit=commit,manifest_sha256=report['manifest_sha256'],results=str(work/'results.json')),indent=2))
