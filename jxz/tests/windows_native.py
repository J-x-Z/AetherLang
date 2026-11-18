#!/usr/bin/env python3
"""Windows-only acceptance of native JXZ: local graphs, offline cache, locks, LSP."""
import argparse,hashlib,json,os,subprocess,sys,tempfile,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--jxz',required=True,type=Path);p.add_argument('--compiler',required=True,type=Path);p.add_argument('--cc',required=True,type=Path);p.add_argument('--work-dir',required=True,type=Path);p.add_argument('--stdlib',type=Path);p.add_argument('--host-source',type=Path,help='Frozen selected Windows JXZ source; defaults to jxz.exe sibling source/')
a=p.parse_args();base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='JXZ Windows Ω ',dir=base));records=[];report={'native_windows_executed':False,'status':'not-run','checks':records}
def save():(w/'results.json').write_text(json.dumps(report,indent=2))
if os.name!='nt':report['status']='skipped: requires actual Windows; no native acceptance claimed';save();print(w/'results.json');sys.exit(77)
jxz=a.jxz.resolve();compiler=a.compiler.resolve();cc=a.cc.resolve();report.update(native_windows_executed=True,status='in-progress',jxz_sha256=hashlib.sha256(jxz.read_bytes()).hexdigest(),compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest())
env={**os.environ,'AETHC':str(compiler),'CC':str(cc),'JXZ_OFFLINE':'1'}
if a.stdlib:env['AETHER_PATH']=str(a.stdlib.resolve())
def run(label,args,cwd,expected=0,extra=None):
 r=subprocess.run(list(map(str,args)),cwd=cwd,env={**env,**(extra or {})},capture_output=True,timeout=120);records.append({'label':label,'command':list(map(str,args)),'cwd':str(cwd),'exit':r.returncode,'stdout':r.stdout.decode(errors='replace'),'stderr':r.stderr.decode(errors='replace')});save();assert r.returncode==expected,(label,r.returncode,r.stdout,r.stderr);return r
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
for backend in ['c','llvm']:
 run('local graph build '+backend,[jxz,'build','--release'],root,extra={'AETHER_BACKEND':backend});run('local graph run '+backend,[jxz,'run','--','space value','; & |'],root,extra={'AETHER_BACKEND':backend});run('native enumeration '+backend,[jxz,'test'],root,extra={'AETHER_BACKEND':backend})
 assert (root/'target/program.exe').is_file()
record=(root/'target/.jxz-dependencies').read_bytes();run('deterministic graph record',[jxz,'build'],root);assert (root/'target/.jxz-dependencies').read_bytes()==record
(leaf/'src/lib.aeth').write_text('pub fn value()->i32{return 44;}');run('dependency edit rebuilds',[jxz,'run'],root,3);(leaf/'src/lib.aeth').write_text('pub fn value()->i32{return 41;}')
# The compiler child retains the directory HANDLE after JXZ is killed.
helper=w/'hold.c';helper.write_text('#include <windows.h>\nint main(void){HANDLE f=CreateFileW(L"compiler-started",GENERIC_WRITE,7,0,CREATE_ALWAYS,0,0);if(f==INVALID_HANDLE_VALUE)return 2;CloseHandle(f);for(int i=0;i<3000;i++){if(GetFileAttributesW(L"compiler-release")!=INVALID_FILE_ATTRIBUTES)return 1;Sleep(10);}return 9;}')
wrapper=w/'holding compiler.exe';run('build native lock helper',[cc,helper,'-o',wrapper],w)
child=subprocess.Popen([str(jxz),'build'],cwd=root,env={**env,'AETHC':str(wrapper)},stdout=subprocess.PIPE,stderr=subprocess.PIPE)
try:
 deadline=time.monotonic()+20
 while not (root/'compiler-started').exists() and child.poll() is None and time.monotonic()<deadline:time.sleep(.01)
 assert (root/'compiler-started').exists();child.kill();child.wait(timeout=5)
 r=run('inherited lock survives parent death',[jxz,'clean'],root,1);assert b'busy or cannot be locked' in r.stderr
finally:
 (root/'compiler-release').touch();child.communicate(timeout=35)
run('recover interrupted publication',[jxz,'build'],root)
# Unknown ownership records are never silently replaced/deleted.
protected=w/'protected';(protected/'src').mkdir(parents=True);(protected/'Jxz.toml').write_text("[package]\nname='safe'\n");(protected/'src/main.aeth').write_text('fn main()->i32{return 0;}');(protected/'target').mkdir();unknown=protected/'target/.jxz-dependencies';unknown.write_bytes(b'USER DATA')
for command in ['build','clean']:
 run('preserve unowned dependency record '+command,[jxz,command],protected,1);assert unknown.read_bytes()==b'USER DATA'
# Fully verified offline cache: no curl/find/tar process is needed.
cache=w/'cache';cache.mkdir();(cache/'.jxz-cache').write_bytes(b'JXZ-CACHE-1\n');files={'Jxz.toml':b"[package]\nname='cached'\nversion='1.2.3'\n[build]\nsrc='src'\n",'src/lib.aeth':b'pub fn value()->i32{return 42;}'}
listing=b'JXZ-PACKAGE-1\n'+b''.join(hashlib.sha256(data).hexdigest().encode()+b'\t'+name.encode()+b'\n' for name,data in sorted(files.items()));digest=hashlib.sha256(listing).hexdigest();package=cache/digest;package.mkdir()
for name,data in files.items():f=package/name;f.parent.mkdir(parents=True,exist_ok=True);f.write_bytes(data)
(package/'.jxz-files').write_bytes(listing);offline=w/'offline';(offline/'src').mkdir(parents=True);(offline/'Jxz.toml').write_text("[package]\nname='offline'\n[dependencies]\ncached='1.2.3'\n");(offline/'Jxz.lock').write_text(f'JXZ-REGISTRY-LOCK-1\ncached\t1.2.3\t{digest}\thttps://invalid.example\n');(offline/'src/main.aeth').write_text('use cached::value;fn main()->i32{if value()!=42{return 1;}return 0;}')
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
run('clean owned outputs',[jxz,'clean'],root);assert (root/'Jxz.toml').is_file() and (root/'src/main.aeth').is_file();assert not (root/'target/program.exe').exists()
report['status']='native Windows JXZ acceptance passed';save();print(w/'results.json')
