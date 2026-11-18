#!/usr/bin/env python3
"""Windows-only acceptance of native JXZ: local graphs, offline cache, locks, LSP."""
import argparse,hashlib,json,os,subprocess,sys,tempfile,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--jxz',required=True,type=Path);p.add_argument('--compiler',required=True,type=Path);p.add_argument('--cc',required=True,type=Path);p.add_argument('--work-dir',required=True,type=Path);p.add_argument('--stdlib',type=Path);p.add_argument('--host-source',type=Path,help='Frozen selected Windows JXZ source; defaults to jxz.exe sibling source/')
p.add_argument('--publication-only',action='store_true',help='Run only the Windows output sharing regression')
a=p.parse_args();base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='JXZ Windows Ω ',dir=base));records=[];report={'native_windows_executed':False,'status':'not-run','checks':records}
def save():(w/'results.json').write_text(json.dumps(report,indent=2))
if os.name!='nt':report['status']='skipped: requires actual Windows; no native acceptance claimed';save();print(w/'results.json');sys.exit(77)
jxz=a.jxz.resolve();compiler=a.compiler.resolve();cc=a.cc.resolve();report.update(native_windows_executed=True,status='in-progress',jxz_sha256=hashlib.sha256(jxz.read_bytes()).hexdigest(),compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest())
env={**os.environ,'AETHC':str(compiler),'CC':str(cc),'JXZ_OFFLINE':'1'}
if a.stdlib:env['AETHER_PATH']=str(a.stdlib.resolve())
def run(label,args,cwd,expected=0,extra=None):
 r=subprocess.run(list(map(str,args)),cwd=cwd,env={**env,**(extra or {})},capture_output=True,timeout=120);records.append({'label':label,'command':list(map(str,args)),'cwd':str(cwd),'exit':r.returncode,'stdout':r.stdout.decode(errors='replace'),'stderr':r.stderr.decode(errors='replace')});save();assert r.returncode==expected,(label,r.returncode,r.stdout,r.stderr);return r
def test_publication_sharing():
    import ctypes
    from ctypes import wintypes

    project = w / 'publication sharing'
    project.mkdir()
    run('initialize sharing fixture', [jxz, 'init', 'publication'], project)
    source = project / 'src/main.aeth'
    source.write_bytes(b'fn main()->i32{return 0;}')
    run('publish original executable', [jxz, 'build'], project)
    target = project / 'target'
    output, record = target / 'publication.exe', target / '.jxz-outputs'
    old_image, old_record = output.read_bytes(), record.read_bytes()
    source.write_bytes(b'fn main()->i32{return 23;}')
    new_image = project / 'replacement.exe'
    run('build replacement executable', [compiler, 'build', source, '--cc', cc, '-o', new_image], project)
    assert new_image.read_bytes() != old_image

    # JXZ invokes AETHC with "build" as argv[1]. Using Python as the compiler
    # makes this file a gate between readying a real transaction and publishing it.
    (project / 'build').write_text(
        'import os, shutil, sys, time\n'
        'from pathlib import Path\n'
        'shutil.copyfile("replacement.exe", sys.argv[sys.argv.index("-o") + 1])\n'
        'Path("pid.tmp").write_text(str(os.getpid()))\n'
        'os.replace("pid.tmp", "compiler.pid")\n'
        'deadline = time.monotonic() + 20\n'
        'while not Path("release").exists():\n'
        '    if time.monotonic() >= deadline: sys.exit(31)\n'
        '    time.sleep(0.01)\n', encoding='ascii')

    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    create = kernel.CreateFileW
    create.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
                       wintypes.LPVOID, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
    create.restype = wintypes.HANDLE
    open_process = kernel.OpenProcess
    open_process.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    open_process.restype = wintypes.HANDLE
    wait = kernel.WaitForSingleObject
    wait.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    wait.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL

    ready, release = project / 'compiler.pid', project / 'release'
    for persistent in (True, False):
        ready.unlink(missing_ok=True)
        release.unlink(missing_ok=True)
        child = subprocess.Popen([jxz, 'build'], cwd=project,
                                 env={**env, 'AETHC':sys.executable},
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        held = worker = None
        try:
            deadline = time.monotonic() + 20
            while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                time.sleep(.01)
            assert ready.is_file(), 'JXZ did not reach its compiler with a prepared transaction'
            assert (target / '.jxz-build/ready').is_file()
            worker = open_process(0x00100000, False, int(ready.read_text()))  # SYNCHRONIZE
            if not worker:
                raise ctypes.WinError(ctypes.get_last_error())
            # Allow readers and writers, explicitly omit FILE_SHARE_DELETE.
            held = create(str(output), 0x80000000, 3, None, 3, 0, None)
            if held == ctypes.c_void_p(-1).value:
                held = None
                raise ctypes.WinError(ctypes.get_last_error())
            release.touch()
            assert wait(worker, 10000) == 0, 'compiler worker did not exit'
            if persistent:
                stdout, stderr = child.communicate(timeout=15)
                assert child.returncode == 1, (stdout, stderr)
                assert output.read_bytes() == old_image and record.read_bytes() == old_record
            else:
                # Start the hold interval after compiler exit, not during compilation.
                try:
                    stdout, stderr = child.communicate(timeout=.20)
                except subprocess.TimeoutExpired:
                    pass
                else:
                    raise AssertionError(('build exited while output was briefly held', child.returncode, stdout, stderr))
                assert kernel.CloseHandle(held)
                held = None
                stdout, stderr = child.communicate(timeout=15)
                assert child.returncode == 0, (stdout, stderr)
                assert output.read_bytes() == new_image.read_bytes() and record.read_bytes() == old_record
                assert not (target / '.jxz-build').exists()
                run('execute replacement after sharing clears', [output], project, 23)
            records.append(dict(label='persistent sharing preserves published output' if persistent else
                                'transient sharing publishes replacement', exit=child.returncode))
            save()
        finally:
            release.touch(exist_ok=True)
            if held is not None:
                kernel.CloseHandle(held)
            if worker:
                kernel.CloseHandle(worker)
            if child.poll() is None:
                child.kill()
            child.communicate(timeout=10)


test_publication_sharing()
if a.publication_only:
    report['status']='native Windows publication sharing passed'; save(); print(w/'results.json'); sys.exit(0)
root=w/'project Ω';root.mkdir();run('version',[jxz,'--version'],root);run('init',[jxz,'init','program'],root);before=(root/'Jxz.toml').read_bytes();run('init preservation',[jxz,'init'],root,1);assert (root/'Jxz.toml').read_bytes()==before
for name,expected in [('missing compiler.exe',127),('invalid compiler.exe',126)]:
 path=w/name
 if expected==126:path.write_bytes(b'not a PE executable')
 r=run('language-server startup status '+name,[jxz,'lsp'],root,expected,{'AETHC':str(path)})
 assert not r.stdout and b'cannot execute language server' in r.stderr
# Real local transitive graph and Unicode paths, without any shell command.
dep=w/'dependency 中文';leaf=w/'leaf';(dep/'src').mkdir(parents=True);(leaf/'src').mkdir(parents=True)
(leaf/'Jxz.toml').write_text("[package]\nname='leaf'\n[build]\nsrc='src'\n",encoding='utf8');(leaf/'src/lib.aeth').write_text('pub fn value()->i32{return 41;}')
(dep/'Jxz.toml').write_text("[package]\nname='dep'\n[build]\nsrc='src'\n[dependencies]\nleaf={path='../leaf'}\n",encoding='utf8')
dep_entry=dep/'src/lib.aeth'
dep_entry.write_text('use leaf::value;\npub fn answer()->i32{return value()+1;}',encoding='utf-8')
manifest="[package]\nname='program'\n[build]\nsrc='src'\ntarget='target'\n[dependencies]\ndep={path='../dependency 中文'}\n"
(root/'Jxz.toml').write_text(manifest,encoding='utf8');text='use dep::answer;\nfn main()->i32{if answer()!=42{return 3;}return 0;}\n';(root/'src/main.aeth').write_text(text)
(root/'tests/nested').mkdir(parents=True);(root/'tests/nested/test_.aeth').write_text('fn main()->i32{return 0;}')
target=root/'target'
expected_record=b'JXZ-PATH-DEPS-1\n'+b''.join(
 f'{name}\t{directory.as_posix()}\t{(directory/"src/lib.aeth").as_posix()}\n'.encode('utf-8')
 for name,directory in [('dep',dep),('leaf',leaf)])
for backend in ['c','llvm']:
 run('local graph build '+backend,[jxz,'build','--release'],root,extra={'AETHER_BACKEND':backend})
 assert (target/'.jxz-outputs').read_bytes()==b'program.exe\n'
 assert (target/'.jxz-dependencies-owner').read_bytes()==b'JXZ-WIN-MARKER-1\nJXZ-PATH-DEPS-RECORD-1'
 assert (target/'.jxz-dependencies').read_bytes()==expected_record
 assert (target/'program.exe').is_file()
 run('local graph run '+backend,[jxz,'run','--','space value','; & |'],root,extra={'AETHER_BACKEND':backend});run('native enumeration '+backend,[jxz,'test'],root,extra={'AETHER_BACKEND':backend})
record=(root/'target/.jxz-dependencies').read_bytes();run('deterministic graph record',[jxz,'build'],root);assert (root/'target/.jxz-dependencies').read_bytes()==record
(leaf/'src/lib.aeth').write_text('pub fn value()->i32{return 44;}');run('dependency edit rebuilds',[jxz,'run'],root,3);(leaf/'src/lib.aeth').write_text('pub fn value()->i32{return 41;}')
# The compiler child retains the delete-on-close lock HANDLE after JXZ is killed.
lock_name='.jxz-directory-lock'
helper=w/'hold.c';helper.write_text('#include <windows.h>\nint main(void){HANDLE f=CreateFileW(L"compiler-started",GENERIC_WRITE,7,0,CREATE_ALWAYS,0,0);if(f==INVALID_HANDLE_VALUE)return 2;CloseHandle(f);for(int i=0;i<3000;i++){if(GetFileAttributesW(L"compiler-release")!=INVALID_FILE_ATTRIBUTES)return 1;Sleep(10);}return 9;}')
wrapper=w/'holding compiler.exe';run('build native lock helper',[cc,helper,'-o',wrapper],w)
child=subprocess.Popen([str(jxz),'build'],cwd=root,env={**env,'AETHC':str(wrapper)},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
try:
 deadline=time.monotonic()+20
 while not (root/'compiler-started').exists() and child.poll() is None and time.monotonic()<deadline:time.sleep(.01)
 assert (root/'compiler-started').exists();child.kill();child.wait(timeout=5)
 assert lock_name in os.listdir(target)
 r=run('inherited lock survives parent death',[jxz,'clean'],root,1);assert b'busy or cannot be locked' in r.stderr
finally:
 (root/'compiler-release').touch();child.communicate(timeout=35)
deadline=time.monotonic()+5
while lock_name in os.listdir(target) and time.monotonic()<deadline:time.sleep(.01)
assert lock_name not in os.listdir(target)
run('recover interrupted publication',[jxz,'build'],root)
# Unknown ownership records are never silently replaced/deleted.
protected=w/'protected';(protected/'src').mkdir(parents=True);(protected/'Jxz.toml').write_text("[package]\nname='safe'\n");(protected/'src/main.aeth').write_text('fn main()->i32{return 0;}');(protected/'target').mkdir();unknown=protected/'target/.jxz-dependencies';unknown.write_bytes(b'USER DATA')
for command in ['build','clean']:
 run('preserve unowned dependency record '+command,[jxz,command],protected,1);assert unknown.read_bytes()==b'USER DATA'
# A preexisting file at the lock name is never opened for deletion.
blocked=w/'preexisting-lock';(blocked/'src').mkdir(parents=True);(blocked/'target').mkdir()
(blocked/'Jxz.toml').write_text("[package]\nname='blocked'\n",encoding='utf-8')
(blocked/'src/main.aeth').write_text('fn main()->i32{return 0;}',encoding='utf-8')
user_lock=blocked/'target'/lock_name;user_lock.write_bytes(b'owner data\x00\r\n')
for command in ['build','clean']:
 r=run('preserve preexisting lock file '+command,[jxz,command],blocked,1)
 assert b'busy or cannot be locked' in r.stderr and user_lock.read_bytes()==b'owner data\x00\r\n'
# Fully verified offline cache: no curl/find/tar process is needed.
cache=w/'cache';cache.mkdir();(cache/'.jxz-cache').write_bytes(b'JXZ-CACHE-1\n');files={'Jxz.toml':b"[package]\nname='cached'\nversion='1.2.3'\n[build]\nsrc='src'\n",'src/lib.aeth':b'pub fn value()->i32{return 42;}'}
listing=b'JXZ-PACKAGE-1\n'+b''.join(hashlib.sha256(data).hexdigest().encode()+b'\t'+name.encode()+b'\n' for name,data in sorted(files.items()));digest=hashlib.sha256(listing).hexdigest();package=cache/digest;package.mkdir()
for name,data in files.items():f=package/name;f.parent.mkdir(parents=True,exist_ok=True);f.write_bytes(data)
(package/'.jxz-files').write_bytes(listing);offline=w/'offline';(offline/'src').mkdir(parents=True);(offline/'Jxz.toml').write_text("[package]\nname='offline'\n[dependencies]\ncached='1.2.3'\n");(offline/'src/main.aeth').write_text('use cached::value;fn main()->i32{if value()!=42{return 1;}return 0;}')
lock=offline/'Jxz.lock'
pin=f'JXZ-REGISTRY-LOCK-1\ncached\t1.2.3\t{digest}\thttps://invalid.example\n'.encode('utf-8')
crlf_pin=pin.replace(b'\n',b'\r\n')
lock.write_bytes(crlf_pin)
r=run('preserve malformed CRLF lock',[jxz,'fetch'],offline,1,{'JXZ_CACHE':str(cache)})
assert b'unrecognized Jxz.lock' in r.stderr and lock.read_bytes()==crlf_pin
lock.write_bytes(pin)
fresh_cache=w/'empty-cache';fresh_cache.mkdir()
r=run('initialize empty offline cache',[jxz,'fetch'],offline,1,{'JXZ_CACHE':str(fresh_cache)})
assert b'offline registry cache is missing or corrupt' in r.stderr
assert os.listdir(fresh_cache)==['.jxz-cache'] and lock_name not in os.listdir(offline)
assert (fresh_cache/'.jxz-cache').read_bytes()==b'JXZ-WIN-MARKER-1\nJXZ-CACHE-1'
assert lock.read_bytes()==pin
run('verified offline cache',[jxz,'run'],offline,extra={'JXZ_CACHE':str(cache)});(package/'src/lib.aeth').write_bytes(b'corrupt');run('corrupt offline cache rejected',[jxz,'build'],offline,1,{'JXZ_CACHE':str(cache)});assert (package/'src/lib.aeth').read_bytes()==b'corrupt'
# Project LSP over standard pipes, including a typed cross-package definition
# and an unsaved edit. Protocol stdout is parsed byte-for-byte.
def frame(value):
 data=json.dumps(value,ensure_ascii=False,separators=(',',':')).encode();return b'Content-Length: '+str(len(data)).encode()+b'\r\n\r\n'+data
uri=(root/'src/main.aeth').as_uri();offset=text.splitlines()[1].index('answer')+1
messages=[{'jsonrpc':'2.0','id':1,'method':'initialize','params':{'rootUri':root.as_uri()}},{'jsonrpc':'2.0','method':'initialized','params':{}},{'jsonrpc':'2.0','method':'textDocument/didOpen','params':{'textDocument':{'uri':uri,'languageId':'aether','version':1,'text':text}}},{'jsonrpc':'2.0','id':2,'method':'textDocument/definition','params':{'textDocument':{'uri':uri},'position':{'line':1,'character':offset}}},{'jsonrpc':'2.0','method':'textDocument/didChange','params':{'textDocument':{'uri':uri,'version':2},'contentChanges':[{'text':text.replace('answer()!=42','missing()!=42')}]}},{'jsonrpc':'2.0','id':3,'method':'shutdown','params':None},{'jsonrpc':'2.0','method':'exit','params':None}]
r=subprocess.run([str(jxz),'lsp'],input=b''.join(map(frame,messages)),cwd=root,env=env,capture_output=True,timeout=60);assert r.returncode==0,(r.returncode,r.stdout,r.stderr);data=r.stdout;responses=[]
while data:
 header,data=data.split(b'\r\n\r\n',1);assert header.startswith(b'Content-Length: '),header;n=int(header.split(b':',1)[1]);responses.append(json.loads(data[:n]));data=data[n:]
definition=[x.get('result') for x in responses if x.get('id')==2]
expected_definition={'uri':dep_entry.as_uri(),'range':{
 'start':{'line':1,'character':7},'end':{'line':1,'character':13}}}
assert definition==[expected_definition],(definition,expected_definition)
assert any(x.get('method')=='textDocument/publishDiagnostics' and x.get('params',{}).get('diagnostics') for x in responses),responses
assert (root/'src/main.aeth').read_text()==text;records.append({'label':'native project LSP definition/unsaved/shutdown','exit':r.returncode,'responses':responses});save()
# Force stdio failure with errno == 0: it must not become Ok(partial bytes).
host_source=(a.host_source or jxz.parent/'source').resolve();assert (host_source/'host.aeth').is_file(),'provide --host-source with the cross runner selected Windows source'
shim=w/'stdio_fault.c';shim.write_text('#include <stdio.h>\n#include <errno.h>\nint ferror(FILE *f){(void)f;errno=0;return 1;}\n')
obj=w/'stdio_fault.obj';run('build stdio error shim',[cc,'-c',shim,'-o',obj],w)
probe=w/'stdio_fault.aeth';probe.write_text('use host::read_path_bytes;use result::Result;fn main()->i32{match read_path_bytes("stdio-input",32){Result::Err(e)=>{if e==5{return 0;}return 2;},Result::Ok(_)=>{return 3;}}}')
(w/'stdio-input').write_bytes(b'partial')
for backend in ['c','llvm']:
 exe=w/('stdio-'+backend+'.exe');run('zero errno read failure build '+backend,[compiler,probe,'--cc',cc,'--backend',backend,'-O2','--module','host='+str(host_source/'host.aeth'),'--link-object',obj,'-l','bcrypt','-l','shell32','-o',exe],w);run('zero errno read failure '+backend,[exe],w)
# Closing a redirected stdout HANDLE must not replace the child's spawn error.
spawn_probe=w/'spawn_status.aeth';spawn_probe.write_text('use host::process_run;fn main()->i32{let args:[*u8;2]=["./missing-child.exe",0 as *u8];if process_run(&args[0] as **u8,"spawn-output")!=127{return 1;}return 0;}')
for backend in ['c','llvm']:
 exe=w/('spawn-status-'+backend+'.exe');run('redirected spawn status build '+backend,[compiler,spawn_probe,'--cc',cc,'--backend',backend,'-O2','--module','host='+str(host_source/'host.aeth'),'-l','bcrypt','-l','shell32','-o',exe],w);run('redirected spawn status '+backend,[exe],w)
run('clean owned outputs',[jxz,'clean'],root);assert (root/'Jxz.toml').is_file() and (root/'src/main.aeth').is_file();assert not target.exists()
report['status']='native Windows JXZ acceptance passed';save();print(w/'results.json')
