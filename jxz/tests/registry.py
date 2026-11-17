#!/usr/bin/env python3
"""Actual loopback HTTP registry, exact locks, integrity and offline JXZ projects."""
import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time
import unittest
from urllib.parse import unquote, urlsplit

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--jxz', type=Path, required=True)
parser.add_argument('--aethc', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backend', choices=['llvm', 'c'], default='llvm')
args, rest = parser.parse_known_args()
jxz = args.jxz.resolve(); compiler = args.aethc.resolve(); work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
records = []
meta = {'jxz_sha256': hashlib.sha256(jxz.read_bytes()).hexdigest(),
        'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(), 'backend': args.backend}

def record(label, command, result, expected):
    item = {'label': label, 'command': list(map(str, command)), 'exit': result.returncode,
            'expected_exit': expected, 'stdout': result.stdout, 'stderr': result.stderr,
            'passed': result.returncode == expected}
    records.append(item)
    (work / 'results.json').write_text(json.dumps({**meta, 'records': records}, indent=2) + '\n')
    return item

class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.folder = Path(tempfile.mkdtemp(prefix=self._testMethodName+'-',dir=work))
        meta.setdefault('projects',{})[self._testMethodName] = str(self.folder)
        self.project = self.folder / 'project Ω'
        self.project.mkdir()
        self.cache = self.folder / 'cache Ω'
        self.routes = {}; self.index = {}; self.requests = []
        self.slow_path = None; self.started = threading.Event(); self.release = threading.Event()
        owner = self
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def do_GET(self):
                path = unquote(urlsplit(self.path).path)
                owner.requests.append(path)
                body = owner.routes.get(path)
                if body is None:
                    self.send_error(404); return
                self.send_response(200); self.send_header('Content-Length', str(len(body))); self.end_headers()
                try:
                    if path == owner.slow_path:
                        split = max(1, len(body)//2)
                        self.wfile.write(body[:split]); self.wfile.flush(); owner.started.set()
                        owner.release.wait(8)
                        self.wfile.write(body[split:])
                    else:
                        self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True); self.thread.start()
        self.origin = f'http://127.0.0.1:{self.server.server_port}'
        self.env = {**os.environ, 'AETHC': str(compiler), 'AETHER_BACKEND': args.backend,
                    'JXZ_REGISTRY': self.origin, 'JXZ_CACHE': str(self.cache),
                    'JXZ_OFFLINE': '0', 'TMPDIR': str(self.folder)}
        self.addCleanup(self.stop_server)

    def stop_server(self):
        self.release.set(); self.server.shutdown(); self.server.server_close(); self.thread.join(2)

    def source(self, path, content):
        dest = self.project / path; dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(content)

    def project_manifest(self, dependencies, expression='common::value()'):
        self.source('Jxz.toml', "[package]\nname='registry_app'\nversion='0.1.0'\n[dependencies]\n" + dependencies + '\n')
        self.source('src/main.aeth', 'use common;extern "C"{fn printf(format:*u8,...)->i32;}fn main()->i32{printf("registry=%d\\n",'+expression+');0}')

    def package(self, name, version='1.0.0', body='pub fn value()->i32{7}', dependencies='', files=None, build=''):
        entries = {'Jxz.toml': f"[package]\nname='{name}'\nversion='{version}'\n[build]\nsrc='src'\n{build}\n[dependencies]\n{dependencies}\n".encode(),
                   'src/lib.aeth': body.encode()}
        entries.update(files or {})
        manifest = b'JXZ-PACKAGE-1\n' + b''.join(hashlib.sha256(content).hexdigest().encode()+b'\t'+path.encode()+b'\n' for path,content in sorted(entries.items()))
        base = f'/{name}/{version}/'
        for path, content in entries.items(): self.routes[base+path] = content
        self.routes[base+'files.jxz'] = manifest
        digest = hashlib.sha256(manifest).hexdigest(); self.index[(name,version)] = digest; self.write_index()
        return digest

    def write_index(self):
        self.routes['/index.jxz'] = ('JXZ-REGISTRY-1\n' + ''.join(f'{name}\t{version}\t{digest}\n' for (name,version),digest in sorted(self.index.items()))).encode()

    def invoke(self, *command, expected=0, env=None):
        cmd = [jxz, *command]
        result = subprocess.run(list(map(str, cmd)), cwd=self.project, env={**self.env, **(env or {})},
                                capture_output=True, text=True, timeout=45)
        record(self._testMethodName+'/'+':'.join(command), cmd, result, expected)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def execute(self, expected):
        cmd = [self.project/'target/registry_app']
        result = subprocess.run(list(map(str, cmd)), env=self.env, capture_output=True, text=True, timeout=10)
        record(self._testMethodName+'/program', cmd, result, 0)
        self.assertEqual((result.returncode,result.stdout,result.stderr), (0,f'registry={expected}\n',''))

    def lock(self): return (self.project/'Jxz.lock').read_bytes()

    def test_diamond_fetch_build_run_test_clean_and_offline(self):
        common = self.package(
            'common', body='use support;pub fn value()->i32{support::number()}',
            build="module_paths=['src/工具 空格']", files={'assets/binary Ω.bin': b'\0\xff\n','src/工具 空格/support.aeth':b'pub fn number()->i32{7}'})
        self.package('left', body='use common;pub fn value()->i32{common::value()+1}', dependencies="common='1.0.0'")
        self.package('right', body='use common;pub fn value()->i32{common::value()+2}', dependencies="common={version='1.0.0'}")
        self.project_manifest("left='1.0.0'\nright={version='1.0.0'}")
        self.source('src/main.aeth','use left;use right;extern "C"{fn printf(format:*u8,...)->i32;}fn main()->i32{printf("registry=%d\\n",left::value()+right::value());0}')
        self.source('tests/test_registry.aeth','use left;use right;fn main()->i32{if left::value()+right::value()==17{0}else{1}}')
        self.invoke('fetch'); lock = self.lock()
        self.assertEqual((self.cache/common/'assets/binary Ω.bin').read_bytes(), b'\0\xff\n')
        self.assertEqual([line.split(b'\t')[0] for line in lock.splitlines()[1:]], [b'common',b'left',b'right'])
        self.invoke('build'); self.execute(17)
        self.assertIn('registry=17', self.invoke('run').stdout)
        self.assertIn('tests/test_registry.aeth', self.invoke('test').stdout)
        self.invoke('clean'); self.assertEqual(self.lock(), lock)
        requests = len(self.requests)
        self.invoke('build', env={'JXZ_OFFLINE':'1'}); self.execute(17)
        self.assertEqual(len(self.requests), requests); self.assertEqual(self.lock(), lock)

    def test_lock_stays_pinned_and_update_is_explicit(self):
        original = self.package('common'); self.project_manifest("common='1.0.0'")
        self.invoke('build'); locked = self.lock(); self.execute(7)
        replacement = self.package('common',body='pub fn value()->i32{19}')
        before = len(self.requests); self.invoke('build'); self.execute(7)
        self.assertEqual(len(self.requests),before); self.assertEqual(self.lock(),locked)
        self.env.pop('JXZ_REGISTRY')  # Updates reuse the pinned origin by default.
        self.invoke('update'); self.assertNotEqual(self.lock(),locked)
        self.assertIn(replacement.encode(),self.lock()); self.assertTrue((self.cache/original).is_dir())
        self.invoke('build',env={'JXZ_OFFLINE':'1'}); self.execute(19)
        self.package('common','2.0.0','pub fn value()->i32{23}')
        self.project_manifest("common='2.0.0'"); saved=self.lock()
        self.assertIn('jxz update',self.invoke('fetch',expected=1).stderr); self.assertEqual(self.lock(),saved)
        self.invoke('update'); self.invoke('build',env={'JXZ_OFFLINE':'1'}); self.execute(23)

    def test_cache_corruption_extra_files_and_symlinks(self):
        digest=self.package('common');self.project_manifest("common='1.0.0'");self.invoke('build')
        locked=self.lock(); artifact=hashlib.sha256((self.project/'target/registry_app').read_bytes()).digest()
        cached=self.cache/digest/'src/lib.aeth';cached.write_text('pub fn value()->i32{99}')
        requests=len(self.requests)
        self.invoke('build',expected=1,env={'JXZ_OFFLINE':'1'})
        self.assertEqual(len(self.requests),requests);self.assertEqual(self.lock(),locked)
        self.assertEqual(hashlib.sha256((self.project/'target/registry_app').read_bytes()).digest(),artifact)
        self.invoke('build');self.execute(7);self.assertTrue(list(self.cache.glob('.jxz-corrupt-*')))
        (self.cache/digest/'src/extra.aeth').write_text('unlisted input')
        self.invoke('fetch',expected=1,env={'JXZ_OFFLINE':'1'});self.invoke('fetch')
        outside=self.folder/'outside.aeth';outside.write_text('must not touch')
        cached.unlink();cached.symlink_to(outside)
        self.invoke('fetch',expected=1,env={'JXZ_OFFLINE':'1'});self.invoke('fetch')
        self.assertEqual(outside.read_text(),'must not touch');self.assertFalse(cached.is_symlink())
        (self.cache/digest/'.jxz-files').write_bytes(b'x' * 1048577)
        self.invoke('fetch',expected=1,env={'JXZ_OFFLINE':'1'});self.invoke('fetch')

    def test_wrong_hash_missing_files_and_retry(self):
        digest=self.package('common');self.project_manifest("common='1.0.0'")
        original=self.routes['/common/1.0.0/src/lib.aeth'];self.routes['/common/1.0.0/src/lib.aeth']=b'corrupted transfer'
        self.invoke('fetch',expected=1);self.assertFalse((self.project/'Jxz.lock').exists());self.assertFalse((self.cache/digest).exists())
        del self.routes['/common/1.0.0/src/lib.aeth'];self.invoke('fetch',expected=1)
        self.routes['/common/1.0.0/src/lib.aeth']=original;self.invoke('fetch')
        self.assertTrue((self.cache/digest).is_dir())
        locked=self.lock();self.index[('common','1.0.0')]='0'*64;self.write_index()
        self.invoke('update',expected=1);self.assertEqual(self.lock(),locked)
        self.assertFalse((self.cache/('0'*64)).exists())

    def test_bad_metadata_and_path_traversal_never_publish(self):
        self.project_manifest("common='1.0.0'")
        outside=self.folder/'escape.aeth';outside.write_text('owner data')
        cases=['../escape.aeth','/absolute.aeth','src/../../escape.aeth','src//double.aeth','src/./dot.aeth','src\\bad.aeth','.jxz-files','Jxz.toml']
        for path in cases:
            with self.subTest(path=path):
                self.package('common')
                body=b'JXZ-PACKAGE-1\n'+b'0'*64+b'\tJxz.toml\n'+b'0'*64+b'\t'+path.encode()+b'\n'
                self.routes['/common/1.0.0/files.jxz']=body
                self.index[('common','1.0.0')]=hashlib.sha256(body).hexdigest();self.write_index()
                before=len(self.requests);self.invoke('fetch',expected=1)
                self.assertFalse((self.project/'Jxz.lock').exists())
                self.assertFalse(any('escape' in value or 'absolute' in value for value in self.requests[before:]))
                self.assertEqual(outside.read_text(),'owner data')
        self.package('common');self.routes['/index.jxz']+=self.routes['/index.jxz'].splitlines(keepends=True)[1]
        self.invoke('fetch',expected=1);self.assertFalse((self.project/'Jxz.lock').exists())

    def test_conflicting_versions_preserve_previous_lock(self):
        self.package('common');self.package('common','2.0.0')
        self.project_manifest("common='1.0.0'");self.invoke('fetch');locked=self.lock()
        self.package('left',dependencies="common='1.0.0'")
        self.package('right',dependencies="common='2.0.0'")
        self.project_manifest("left='1.0.0'\nright='1.0.0'")
        error=self.invoke('fetch',expected=1).stderr;self.assertIn('conflicting exact versions',error)
        self.assertEqual(self.lock(),locked)
        self.invoke('update',expected=1);self.assertEqual(self.lock(),locked)

    def test_old_curl_fails_before_download(self):
        self.package('common');self.project_manifest("common='1.0.0'")
        tools=self.folder/'tools';tools.mkdir()
        curl=tools/'curl';curl.write_text('#!/bin/sh\nprintf "curl 7.88.1 old transport\\n"\n');curl.chmod(0o755)
        failure=self.invoke('fetch',expected=1,env={'PATH':str(tools)+os.pathsep+self.env['PATH']})
        self.assertIn('curl 8.4',failure.stderr);self.assertEqual(self.requests,[])
        self.assertFalse((self.project/'Jxz.lock').exists())

    def test_exact_version_syntax_and_identity(self):
        self.package('common')
        for version in ['1.0','^1.0.0','01.0.0','1.0.0-alpha','4294967296.0.0','1.0.0+build']:
            self.project_manifest(f"common='{version}'");before=len(self.requests)
            self.invoke('fetch',expected=1);self.assertEqual(len(self.requests),before)
        self.project_manifest("common='1.0.0'")
        self.package('common',files={'Jxz.toml':b"[package]\nname='different'\nversion='1.0.0'\n"})
        self.invoke('fetch',expected=1);self.assertFalse((self.project/'Jxz.lock').exists())

    def test_registry_packages_cannot_escape_into_local_dependencies(self):
        self.project_manifest("common='1.0.0'")
        self.package('common',dependencies="outside={path='../../outside'}")
        self.invoke('fetch',expected=1);self.assertFalse((self.project/'Jxz.lock').exists())
        self.package('common',build="module_paths=['../outside']")
        self.invoke('fetch',expected=1);self.assertFalse((self.project/'Jxz.lock').exists())

    def test_unknown_lock_and_cache_are_preserved(self):
        self.package('common');self.project_manifest("common='1.0.0'")
        path=self.project/'Jxz.lock';path.write_bytes(b'owner notes\n')
        self.invoke('fetch',expected=1);self.invoke('update',expected=1)
        self.assertEqual(path.read_bytes(),b'owner notes\n');self.assertEqual(self.requests,[])
        path.unlink();outside=self.folder/'owner-lock';outside.write_bytes(b'keep');path.symlink_to(outside)
        self.invoke('fetch',expected=1);self.assertEqual(outside.read_bytes(),b'keep');path.unlink()
        self.cache.mkdir();(self.cache/'user-data').write_bytes(b'keep')
        self.invoke('fetch',expected=1);self.assertEqual((self.cache/'user-data').read_bytes(),b'keep')
        self.assertFalse((self.cache/'.jxz-cache').exists())
        os.mkfifo(self.cache/'.jxz-cache')
        self.invoke('fetch',expected=1)
        self.assertEqual((self.cache/'user-data').read_bytes(),b'keep')
        os.mkfifo(self.project/'Jxz.lock')
        self.assertIn('regular file',self.invoke('fetch',expected=1).stderr)
        self.assertTrue((self.project/'Jxz.lock').is_fifo())

    def test_offline_cold_and_lsp_do_not_download_or_mutate(self):
        self.package('common');self.project_manifest("common='1.0.0'")
        self.invoke('lsp',expected=1);self.assertFalse(self.cache.exists());self.assertEqual(self.requests,[])
        self.invoke('fetch',expected=1,env={'JXZ_OFFLINE':'1'});self.assertEqual(self.requests,[])
        self.invoke('fetch');locked=self.lock();before=len(self.requests)
        stub=self.folder/'lsp-stub';stub.write_text('#!/bin/sh\nexit 23\n');stub.chmod(0o755)
        snapshot={str(p.relative_to(self.cache)):p.read_bytes() for p in self.cache.rglob('*') if p.is_file() and not p.is_symlink()}
        self.invoke('lsp',expected=23,env={'AETHC':str(stub)})
        self.assertEqual(self.lock(),locked);self.assertEqual(len(self.requests),before)
        self.assertEqual(snapshot,{str(p.relative_to(self.cache)):p.read_bytes() for p in self.cache.rglob('*') if p.is_file() and not p.is_symlink()})
        self.assertFalse((self.project/'target').exists())

    def slow_fetch(self):
        self.slow_path='/common/1.0.0/src/lib.aeth'
        proc=subprocess.Popen([str(jxz),'fetch'],cwd=self.project,env=self.env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        self.addCleanup(lambda: proc.kill() if proc.poll() is None else None)
        self.assertTrue(self.started.wait(8),'curl did not reach controlled HTTP response')
        return proc

    def test_interrupted_fetch_recovers_without_a_partial_lock(self):
        digest=self.package('common');self.project_manifest("common='1.0.0'");proc=self.slow_fetch()
        proc.kill();self.release.set();out,err=proc.communicate(timeout=10)
        record(self._testMethodName+'/killed-fetch',[jxz,'fetch'],subprocess.CompletedProcess([],proc.returncode,out,err),-signal.SIGKILL)
        self.assertFalse((self.project/'Jxz.lock').exists());self.assertFalse((self.cache/digest).exists())
        self.slow_path=None
        # A still-finishing curl may briefly retain the cache lock. It cannot
        # publish a package, and the bounded retry does not erase its evidence.
        deadline=time.monotonic()+5
        while True:
            result=subprocess.run([str(jxz),'fetch'],cwd=self.project,env=self.env,capture_output=True,text=True,timeout=35)
            if result.returncode==0: break
            self.assertIn('busy or cannot be locked',result.stderr)
            self.assertLess(time.monotonic(),deadline);time.sleep(.05)
        record(self._testMethodName+'/recovered-fetch',[jxz,'fetch'],result,0)
        self.assertTrue((self.cache/digest).is_dir());self.invoke('build',env={'JXZ_OFFLINE':'1'});self.execute(7)

    def test_lock_changed_during_download_is_preserved(self):
        self.package('common');self.project_manifest("common='1.0.0'");proc=self.slow_fetch()
        (self.project/'Jxz.lock').write_bytes(b'new owner data\n');self.release.set()
        out,err=proc.communicate(timeout=15)
        record(self._testMethodName+'/changed-lock',[jxz,'fetch'],subprocess.CompletedProcess([],proc.returncode,out,err),1)
        self.assertEqual(proc.returncode,1,out+err);self.assertIn('changed during resolution',err)
        self.assertEqual(self.lock(),b'new owner data\n')

if __name__=='__main__':
    suite=unittest.defaultTestLoader.loadTestsFromTestCase(RegistryTests)
    if rest:
        suite=unittest.defaultTestLoader.loadTestsFromNames(rest,module=__import__(__name__))
    result=unittest.TextTestRunner(verbosity=2).run(suite)
    meta['tests_run']=result.testsRun;meta['failures']=len(result.failures);meta['errors']=len(result.errors)
    (work/'results.json').write_text(json.dumps({**meta,'records':records},indent=2)+'\n')
    raise SystemExit(0 if result.wasSuccessful() else 1)
