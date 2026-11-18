#!/usr/bin/env python3
import argparse,json,subprocess,tempfile,hashlib,os,sys
from pathlib import Path
ap=argparse.ArgumentParser(description="Native Aether LSP, shared CLI source facts and bounded session regressions")
ap.add_argument('--compiler',required=True);ap.add_argument('--work-dir',required=True)
ap.add_argument('--no-injection',action='store_true',help='Skip transport interposition (useful for an already sanitized compiler)')
args=ap.parse_args();compiler=str(Path(args.compiler).resolve());base=Path(args.work_dir).resolve();base.mkdir(parents=True,exist_ok=True)
w=Path(tempfile.mkdtemp(prefix='run-',dir=base));(base/'latest.json').write_text(json.dumps(dict(run_dir=str(w),results=str(w/'results.json')),indent=2));results=[]
repo=Path(__file__).resolve().parent.parent
metadata=dict(compiler=compiler,sha256=hashlib.sha256(Path(compiler).read_bytes()).hexdigest())
def persist(): (w/'results.json').write_text(json.dumps(dict(**metadata,checks=results),indent=2))
def file(name,text):
 p=w/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text);return p
def req(i,method,params=None):return dict(id=i,method=method,params=params)
def note(method,params=None):return dict(method=method,params=params)
def opened(p,t):return note('textDocument/didOpen',dict(textDocument=dict(uri=p.as_uri(),version=1,text=t)))
def changed(p,t,v):return note('textDocument/didChange',dict(textDocument=dict(uri=p.as_uri(),version=v),contentChanges=[dict(text=t)]))
def query(i,method,p,text,needle,include=False):
 at=text.index(needle);line=text[:at].count('\n');column=len(text[:at].rsplit('\n',1)[-1].encode('utf-16-le'))//2
 return req(i,'textDocument/'+method,dict(textDocument=dict(uri=p.as_uri()),position=dict(line=line,character=column),context=dict(includeDeclaration=include)))
def wire(messages):
 out=b''
 for message in messages:
  b=json.dumps(message,ensure_ascii=False).encode();out+=f'Content-Length: {len(b)}\r\n\r\n'.encode()+b
 return out
def run(name,messages,flags=(),raw=None,expected=0,stderr=b'',env=None):
 data=wire(messages) if raw is None else raw;r=subprocess.run([compiler,'lsp',*flags],input=data,capture_output=True,timeout=90,env={**os.environ,**(env or {})})
 (w/(name+'.stdout')).write_bytes(r.stdout);(w/(name+'.stderr')).write_bytes(r.stderr)
 assert r.returncode==expected,(name,r.returncode,r.stderr)
 if stderr is not None:assert r.stderr==stderr,(name,r.stderr)
 out=[];remaining=r.stdout
 while remaining:
  h,sep,body=remaining.partition(b'\r\n\r\n');assert sep,(name,remaining[:150]);n=int(h.removeprefix(b'Content-Length: '));assert len(body)>=n;out.append(json.loads(body[:n]));remaining=body[n:]
 results.append(dict(name=name,status='pass',returncode=r.returncode,replies=len(out)));persist()
 return out
init=req(1,'initialize',{});end=[req(998,'shutdown'),note('exit')]
def replies(r):return {m['id']:m for m in r if 'id'in m}
# Same-spelled fields retain their own declaration; includes constructor keys.
lt='pub struct Point{x:i32}\n';l=file('left.aeth',lt);rr=file('right.aeth',lt)
t='use left::Point as L; use right::Point as R; fn main()->i32{let a:L=L{x:1};let b:R=R{x:2};return a.x+b.x;}'
p=file('fields.aeth',t);ot='use left::Point; fn f(p:*Point)->i32{return p.x;} fn main()->i32{return 0;}'
o=file('other.aeth',ot)
r=run('fields',[init,opened(p,t),opened(o,ot),opened(l,lt),query(2,'definition',p,t,'x+b'),query(3,'definition',p,t,'x;}'),query(4,'references',l,lt,'x:i32',True),changed(o,'fn main()->i32{return 0;}',2),query(5,'references',l,lt,'x:i32'),*end]);by=replies(r)
assert by[2]['result']['uri']==l.as_uri(),by[2];assert by[3]['result']['uri']==rr.as_uri(),by[3]
assert len(by[4]['result'])==4,by[4];assert len(by[5]['result'])==2,by[5]
# Exact local binding identities and generic method callee source locations.
t='struct Box<T>{value:T}\nimpl<T> Box<T>{pub fn get(self:&Self)->T{return self.value;}}\nfn main()->i32{let b:Box<i32>=Box{value:7};let x:i32=1;{let x:i32=2;x;}return b.get()+x;}'
p=file('methods.aeth',t)
r=run('methods',[init,opened(p,t),query(2,'definition',p,t,'get()+'),query(3,'hover',p,t,'get()+'),query(4,'definition',p,t,'x;}return'),query(5,'definition',p,t,'x;',False),req(6,'textDocument/documentSymbol',dict(textDocument=dict(uri=p.as_uri()))),req(7,'textDocument/completion',dict(textDocument=dict(uri=p.as_uri()))),*end]);by=replies(r)
assert by[2]['result']['range']['start']['line']==1,by
assert 'i32'in by[3]['result']['contents']['value'],by[3]
assert by[4]['result']['range']['start']['character']==t.splitlines()[2].index('x:i32=2'),by[4]
assert any(s['name']=='Box' for s in by[6]['result']);assert any(s['label']=='main' for s in by[7]['result'])
# UTF16 positions before an astral character; incremental edit and stale version.
t='fn answer()->i32{return 7;}\nfn main()->i32{let text:*u8="😀é";return answer();}'
p=file('unicode Ω.aeth',t);pos=query(2,'definition',p,t,'answer();');byte=t.index('7;');line=0;col=byte
change=note('textDocument/didChange',dict(textDocument=dict(uri=p.as_uri(),version=2),contentChanges=[dict(range=dict(start=dict(line=0,character=col),end=dict(line=0,character=col+1)),text='true')]))
r=run('utf16',[init,opened(p,t),pos,change,changed(p,t,1),changed(p,t,3),*end]);by=replies(r);assert by[2]['result']['range']['start']==dict(line=0,character=3)
diags=[m['params']['diagnostics'] for m in r if m.get('method')=='textDocument/publishDiagnostics'];assert any(diags) and diags[-1]==[]
# Diagnostic locations after astral text are UTF16 rather than UTF8 bytes.
bad='fn main()->i32{let s:*u8="😀é";return true;}'
bp=file('utf16-error.aeth',bad);r=run('utf16-diagnostic',[init,opened(bp,bad),*end]);d=next(m['params']['diagnostics'][0] for m in r if m.get('method')=='textDocument/publishDiagnostics' and m['params']['diagnostics']);expected=len(bad[:bad.index('return')].encode('utf-16-le'))//2;assert d['range']['start']['character']==expected,d
# Open-only source modules are never written to disk. Closing restores disk graph.
child=w/'overlay_only.aeth';child.unlink(missing_ok=True);ct='pub fn answer()->i32{return 9;}';t='use overlay_only::answer;fn main()->i32{return answer();}';p=file('vfs.aeth',t)
r=run('vfs',[init,opened(child,ct),opened(p,t),query(2,'definition',p,t,'answer();'),note('textDocument/didClose',dict(textDocument=dict(uri=child.as_uri()))),*end]);assert replies(r)[2]['result']['uri']==child.as_uri();assert not child.exists();assert any(m['params']['diagnostics'] for m in r if m.get('method')=='textDocument/publishDiagnostics' and m['params']['uri']==p.as_uri())
# Entirely new unsaved directories are resolved without filesystem creation.
child=w/'new parent Ω/deep/only.aeth';ct='pub fn answer()->i32{return 12;}'
p=w/'new parent Ω/deep/main.aeth';t='use only::answer;fn main()->i32{return answer();}'
r=run('vfs-new-directories',[req(1,'initialize',dict(rootUri=p.parent.as_uri())),opened(child,ct),opened(p,t),query(2,'definition',p,t,'answer();'),*end]);assert replies(r)[2]['result']['uri']==child.as_uri(),r;assert not p.parent.exists()
# Inline and external mod use the compiler's exact graph, not a second resolver.
q=file('nested/child.aeth','pub fn value()->i32{return 4;}');t='mod nested { pub mod child; } fn main()->i32{return nested::child::value();}'
# Existing mod resolution uses containing-file directory, so explicit flat child.
q=file('child.aeth','pub fn value()->i32{return 4;}');p=file('inline.aeth',t)
r=run('inline',[init,opened(p,t),query(2,'definition',p,t,'value();'),*end]);assert replies(r)[2]['result']['uri']==q.as_uri(),r
# Generated receiver temporaries must not obscure a real method occurrence.
t='struct S{value:i32} impl S{fn get(self:&Self)->i32{return self.value;}} fn make()->S{return S{value:9};} fn main()->i32{return make().get();}'
p=file('temporary.aeth',t);r=run('temporary-method',[init,opened(p,t),query(2,'definition',p,t,'get();'),*end]);assert replies(r)[2]['result']['range']['start']['character']==t.index('get('),r
# Native Script shares token locations and inferred scalar facts.
t='def answer() -> int:\n    return 7\ndef main() -> int:\n    return answer()\n';p=file('script.ath',t)
r=run('script',[init,opened(p,t),query(2,'definition',p,t,'answer()\n'),*end]);assert replies(r)[2]['result']['range']['start']['line']==0,r
# JSON IDs/escaping, protocol recovery, errors and bounded malformed transport.
r=run('json',[req('id\x00😀','initialize',{}),req(7,'unknown'),*end]);assert replies(r)['id\x00😀']['result'];assert replies(r)[7]['error']['code']==-32601
bad=b'{"method": "bad\\q"}';r=run('parse-recovery',[],raw=f'Content-Length: {len(bad)}\r\n\r\n'.encode()+bad+wire([init,*end]));assert r[0]['error']['code']==-32700;assert replies(r)[1]['result']
for name,data in [('truncated',b'Content-Length: 99\r\n\r\n{}'),('duplicate',b'Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}'),('too-large',b'Content-Length: 16777217\r\n\r\n')]:run(name,[],raw=data,expected=1,stderr=None)
for name,bad in [('surrogate',b'{"id":"\\uD800","method":"initialize"}'),('utf8',b'{"method":"\xff"}'),('nesting',b'['*129+b'0'+b']'*129)]:
 r=run(name,[],raw=f'Content-Length: {len(bad)}\r\n\r\n'.encode()+bad+wire([init,*end]));assert r[0]['error']['code']==-32700;assert replies(r)[1]['result']
r=run('shutdown',[init,req(9,'shutdown'),req(10,'textDocument/hover'),note('exit')]);assert replies(r)[10]['error']['code']==-32600
run('early-exit',[note('exit')],expected=1,stderr=None)
# null is a real request ID; notification methods carrying it must not mutate.
p=file('request-id.aeth','fn main()->i32{return 0;}');bad='fn main()->i32{return missing;}'
invalid=opened(p,bad);invalid['id']=None
r=run('null-id-and-notification',[req(None,'initialize',{}),invalid,opened(p,p.read_text()),note('shutdown'),req(9,'textDocument/documentSymbol',dict(textDocument=dict(uri=p.as_uri()))),*end]);assert [x for x in r if x.get('id','absent') is None][1]['error']['code']==-32600,r;assert replies(r)[9]['result'];assert all(not x['params']['diagnostics'] for x in r if x.get('method')=='textDocument/publishDiagnostics')
r=run('duplicate-initialize',[init,req(11,'initialize',{}),*end]);assert replies(r)[11]['error']['code']==-32600
# Hundreds of updates free parse/check arenas and closed document buffers.
p=file('session.aeth','fn main()->i32{return 0;}');msgs=[init,opened(p,p.read_text())]
for i in range(2,202):msgs.append(changed(p,'fn main()->i32{return '+str(i)+';}',i))
msgs += [note('textDocument/didClose',dict(textDocument=dict(uri=p.as_uri()))),*end]
run('long-session',msgs)
# CLI JSON diagnostics and symbols share source locations and declaration IDs.
p=file('symbols.aeth','fn main()->i32{let x:i32=1;{let x:i32=2;x;}return x;}')
r=subprocess.run([compiler,'symbols',p],capture_output=True,timeout=30);assert r.returncode==0,r.stderr;s=json.loads(r.stdout);xs=[x for x in s['symbols'] if x['name']=='x'];assert len(xs)==2,s;assert len({x['id'] for x in xs})==2
for x in xs:assert any(y['symbol']==x['id'] for y in s['references'])
results.append(dict(name='cli-symbols',status='pass'))
p=file('bad.ath','def main() -> int:\n    return missing\n');r=subprocess.run([compiler,'check','--error-format','json',p],capture_output=True,timeout=30);assert r.returncode==1 and not r.stdout,(r.returncode,r.stdout,r.stderr);d=json.loads(r.stderr);assert Path(d['location']['file']).resolve()==p.resolve() and d['location']['line']==2,d;assert '@'not in d['message'];results.append(dict(name='cli-json-script',status='pass'))
if not args.no_injection:
 library=w/('eintr.dylib' if sys.platform=='darwin' else 'eintr.so');fixture=repo/'tests/regressions/bootstrap/lsp_eintr.c'
 command=[os.environ.get('CC','clang'),*(['-dynamiclib'] if sys.platform=='darwin' else ['-shared','-fPIC']),str(fixture),'-o',str(library),*([] if sys.platform=='darwin' else ['-ldl'])]
 built=subprocess.run(command,capture_output=True,timeout=30);assert built.returncode==0,built.stderr
 receipt=w/'eintr-receipt';key='DYLD_INSERT_LIBRARIES' if sys.platform=='darwin' else 'LD_PRELOAD'
 run('native-eintr-header-body-read-write',[init,*end],env={key:str(library),'AETHER_LSP_EINTR_RECEIPT':str(receipt)})
 assert receipt.read_text().strip()=='15','all four transport interruption points must execute'
persist();print(f'{len(results)} protocol/CLI scenarios passed; {w/"results.json"}')
