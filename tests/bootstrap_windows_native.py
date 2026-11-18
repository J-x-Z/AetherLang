#!/usr/bin/env python3
"""Run only on Windows: verify the prepared native compiler and self-hosted stages."""
import argparse,hashlib,json,os,subprocess,sys,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',required=True,type=Path);p.add_argument('--core-source',required=True,type=Path);p.add_argument('--cc',required=True,type=Path,help='Native clang.exe with a configured Windows C runtime/SDK');p.add_argument('--work-dir',required=True,type=Path);p.add_argument('--lsp-runner',type=Path)
a=p.parse_args();base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='windows native Ω ',dir=base));records=[];report=dict(native_windows_executed=False,status='not-run',checks=records)
def save(): (w/'results.json').write_text(json.dumps(report,indent=2))
if os.name!='nt':
 report['status']='skipped: requires actual Windows; cross compilation is not native acceptance';save();print(w/'results.json');sys.exit(77)
tool=a.compiler.resolve();cc=a.cc.resolve();source=a.core_source.resolve();report.update(native_windows_executed=True,status='in-progress',compiler=str(tool),sha256=hashlib.sha256(tool.read_bytes()).hexdigest(),cc=str(cc),source_hashes={str(f.relative_to(source)):hashlib.sha256(f.read_bytes()).hexdigest() for f in source.glob('*.aeth')});save()
def run(label,command,expected=0,output=None):
 r=subprocess.run(list(map(str,command)),capture_output=True,timeout=240,cwd=w);records.append(dict(label=label,command=list(map(str,command)),status=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace')));save();assert r.returncode==expected,(label,r.returncode,r.stdout,r.stderr)
 if output is not None:assert r.stdout==output,(label,r.stdout,output)
 return r
run('version',[tool,'--version'])
program=w/'source Ω ; &.aeth';program.write_text('''extern "C"{fn printf(format:*u8,...)->i32;fn _close(fd:i32)->i32;fn _setmode(fd:i32,mode:i32)->i32;}
struct Pair{x:i64,y:i64} fn pair(x:i64)->Pair{return Pair{x:x,y:x+1};}
fn main()->i32{_setmode(1,32768);let p:Pair=pair(41);if p.x+p.y!=83{return 1;}if _close(-1)!=-1||__errno_get()!=9{return 2;}
let max:u64=18446744073709551615;printf("%s %lld %llu %.1f\\n","Ω中文😀",-9223372036854775808,max,1.5);return 0;}
''',encoding='utf-8')
expected='Ω中文😀 -9223372036854775808 18446744073709551615 1.5\n'.encode()
snapshot=Path(__file__).resolve().parent/'regressions/large_value_snapshot.aeth'
for backend in ('c','llvm'):
 for opt in (0,2):
  exe=w/f'program Ω {backend} {opt}.exe';run(f'native {backend} O{opt} build',[tool,'build',program,'--cc',cc,'--backend',backend,f'-O{opt}','-o',exe]);run(f'native {backend} O{opt} run',[exe],output=expected)
  obj=w/f'object {backend} {opt}.o';run('COFF object '+backend+str(opt),[tool,program,'--cc',cc,'--backend',backend,f'-O{opt}','--emit-obj','-o',obj]);assert obj.read_bytes()[:2]==b'\x64\x86'
  large=w/f'large-{backend}-O{opt}.exe'
  run(f'large aggregate {backend} O{opt} build',[tool,'build',snapshot,'--cc',cc,'--backend',backend,f'-O{opt}','-o',large])
  run(f'large aggregate {backend} O{opt} run',[large],output=b'')
# Failed parsing and child startup preserve an existing requested output.
old=w/'preserved Ω.exe';old.write_bytes(b'PRESERVE');bad=w/'bad.aeth';bad.write_text('fn main()->i32{return unknown;}')
run('parse failure preserves output',[tool,bad,'--cc',cc,'-o',old],1);assert old.read_bytes()==b'PRESERVE'
run('missing child preserves output',[tool,program,'--cc',w/'missing compiler.exe','-o',old],1);assert old.read_bytes()==b'PRESERVE'
assert not list(w.glob('*.tmp.*'))
r=run('JSON diagnostics',[tool,'check','--error-format','json',bad],1);assert not r.stdout;assert Path(json.loads(r.stderr)['location']['file']).resolve()==bad.resolve()
run('workspace symbols',[tool,'symbols',program]);
# Both code generators compile the same selected Windows source; the native
# driver must spawn the toolchain, validate PE, and publish the resulting image.
stages=[]
for backend in ('c','llvm'):
 stage=w/f'stage2-{backend}.exe';run('native self compiler '+backend,[tool,'build',source/'aethc.aeth','--cc',cc,'--backend',backend,'-O2','-l','bcrypt','-l','shell32','-o',stage]);run('self compiler version '+backend,[stage,'--version']);stages.append(stage)
llvm=[]
for compiler in [tool,*stages]:
 out=w/(compiler.stem+'.self.ll');run('fixed point emit '+compiler.stem,[compiler,source/'aethc.aeth','--emit-llvm','-o',out]);llvm.append(out.read_bytes())
assert llvm[0]==llvm[1]==llvm[2],'same-source native compiler LLVM fixed point differs'
abi = Path(__file__).resolve().parent/'regressions/aggregates'
oracle = w/'win64-oracle.obj'
run('compile independent Win64 C caller and callee', [cc, '-std=c11', '-O2', '-c', abi/'win64.c', '-o', oracle])
for backend in ('c', 'llvm'):
 for level in (0, 2):
  exe = w/f'win64-abi-{backend}-O{level}.exe'
  run(f'Win64 ABI {backend} O{level} build', [stages[1], 'build', abi/'win64.aeth', '--cc', cc,
      '--backend', backend, f'-O{level}', '--link-object', oracle, '-o', exe])
  run(f'Win64 bidirectional ABI {backend} O{level} run', [exe])
scalars=w/'win64-scalar.aeth'
scalars.write_text('extern "C"{fn narrow(a:i8,b:u16,c:bool)->u8;} fn main()->i32{return narrow(-1,2,true) as i32;}')
scalar_ir=w/'win64-scalar.ll'
run('Win64 narrow scalar signature', [stages[1], scalars, '--emit-llvm', '-o', scalar_ir])
decl=next(line for line in scalar_ir.read_text().splitlines() if line.startswith('declare ') and '@narrow(' in line)
assert 'declare i8 @narrow(' in decl and decl.count('zeroext')==1 and 'i1 zeroext' in decl and 'signext' not in decl, decl
for name, definition in [('empty', 'struct S{}'),
                         ('nested-empty','struct Empty{} struct S{x:u8,empty:Empty}')]:
 for direction, signature in [('in','(value:S)->i32'),('out','()->S')]:
  invalid=w/f'win64-{name}-{direction}.aeth'
  invalid.write_text(definition+' extern "C"{fn boundary'+signature+';} fn main()->i32{return 0;}')
  result=run(f'reject Win64 {name} record {direction}', [stages[1], 'check', invalid], 1)
  assert b'zero-sized' in result.stderr
if a.lsp_runner:run('native LSP suite',[sys.executable,a.lsp_runner.resolve(),'--compiler',stages[1],'--work-dir',w/'lsp','--no-injection'])
report['status']='native Windows acceptance passed';save();print(w/'results.json')
