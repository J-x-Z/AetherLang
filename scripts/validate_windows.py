#!/usr/bin/env python3
"""Windows-only bundle integrity, toolchain and existing native acceptance suites."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import time

from windows_support import inspect_pe

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--package',required=True,type=Path)
p.add_argument('--cc',required=True,type=Path)
p.add_argument('--work-dir',required=True,type=Path)
a=p.parse_args()
if os.name!='nt':
    p.exit(77,'Requires actual Windows; no Windows image was executed.\n')
if sys.maxsize<=2**32:
    p.error('use 64-bit Python on Windows x64')
package=a.package.resolve(strict=True)
cc=a.cc.resolve(strict=True)
if not cc.is_file():
    p.error('--cc must name the actual native clang executable')
base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True)
w=Path(tempfile.mkdtemp(prefix='aether-windows-',dir=base))
report=dict(native_windows_executed=False,status='verifying package',checks=[])
def save():
    (w/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    (base/'latest.json').write_text(json.dumps(dict(run_dir=str(w),results=str(w/'results.json')),indent=2)+'\n')
save()
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
env={**os.environ,'TMP':str(w),'TEMP':str(w),'TMPDIR':str(w),'CC':str(cc),
     'AETHC':str(package/'aethc.exe'),'AETHER_PATH':str(package/'stdlib'),
     'JXZ_OFFLINE':'1','PYTHONUTF8':'1','PATH':str(cc.parent)+os.pathsep+os.environ.get('PATH','')}
def run(label,command,timeout=240,expected_stdout=None):
    start=time.monotonic()
    try:
        r=subprocess.run(list(map(str,command)),env=env,cwd=w,capture_output=True,timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        (w/(label+'.stdout.log')).write_bytes(exc.stdout or b'')
        (w/(label+'.stderr.log')).write_bytes(exc.stderr or b'')
        report['checks'].append(dict(label=label,status='timeout',seconds=time.monotonic()-start));save();raise
    (w/(label+'.stdout.log')).write_bytes(r.stdout);(w/(label+'.stderr.log')).write_bytes(r.stderr)
    report['checks'].append(dict(label=label,command=list(map(str,command)),status=r.returncode,seconds=time.monotonic()-start));save()
    if r.returncode or (expected_stdout is not None and r.stdout!=expected_stdout):
        raise RuntimeError(label+' failed; see '+str(w/(label+'.stderr.log')))
    return r
try:
    manifest_path=package/'manifest.json'
    sums={}
    for line in (package/'SHA256SUMS').read_text().splitlines():
        checksum,name=line.split('  ',1)
        if name in sums:raise RuntimeError('duplicate checksum path')
        sums[name]=checksum
    if sums.get('manifest.json')!=sha(manifest_path):raise RuntimeError('manifest checksum mismatch')
    manifest=json.loads(manifest_path.read_text(encoding='utf-8'))
    if manifest.get('format')!='aether-windows-native-bundle-1':raise RuntimeError('unknown bundle format')
    names=set()
    for item in manifest['files']:
        path=PurePosixPath(item['name'])
        if path.is_absolute() or '..' in path.parts or '\\' in item['name'] or ':' in item['name'] or item['name'] in names:
            raise RuntimeError('invalid or duplicate package path')
        names.add(item['name']);f=package/item['name']
        if not f.is_file() or package not in f.resolve().parents or f.stat().st_size!=item['bytes'] or sha(f)!=item['sha256'] or sums.get(item['name'])!=item['sha256']:
            raise RuntimeError('package checksum mismatch: '+item['name'])
    if set(sums)!=names|{'manifest.json'}:raise RuntimeError('checksum inventory differs from manifest')
    report.update(status='native toolchain preflight',source_commit=manifest['source_commit'],manifest_sha256=sha(manifest_path),cc=str(cc),cc_sha256=sha(cc),verified_files=len(names))
    save()
    version=run('clang-version',[cc,'--version'])
    report['clang_version']=version.stdout.decode(errors='replace')
    smoke=w/'toolchain.c'
    smoke.write_text('#include <windows.h>\n#include <stdio.h>\n_Static_assert(sizeof(void*)==8,"x64");\n_Static_assert(sizeof(long)==4,"LLP64");\n_Static_assert(sizeof(WCHAR)==2,"UTF16");\nint main(void){return puts("windows-x64-toolchain-ok")<0;}\n')
    exe=w/'toolchain.exe';run('clang-build',[cc,'-std=c11',smoke,'-o',exe])
    report['toolchain_pe']=inspect_pe(exe)
    run('clang-run',[exe],expected_stdout=b'windows-x64-toolchain-ok\r\n')
    report.update(native_windows_executed=True,status='native suites in progress');save()
    run('compiler-native',[sys.executable,package/'tests/bootstrap_windows_native.py','--compiler',package/'aethc.exe',
        '--core-source',package/'source/core','--cc',cc,'--work-dir',w/'compiler','--lsp-runner',package/'tests/bootstrap_lsp.py'],timeout=1800)
    run('jxz-native',[sys.executable,package/'jxz/tests/windows_native.py','--jxz',package/'jxz.exe',
        '--compiler',package/'aethc.exe','--cc',cc,'--work-dir',w/'jxz','--stdlib',package/'stdlib','--host-source',package/'source/jxz'],timeout=1200)
    report['status']='native Windows acceptance passed';save();print(w/'results.json')
except BaseException as error:
    report.update(status='failed',error=str(error));save();print(w/'results.json',file=sys.stderr);raise
