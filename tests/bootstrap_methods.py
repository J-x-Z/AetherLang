#!/usr/bin/env python3
"""Inherent/generic methods use the shared self-hosted signatures and checked references."""
import argparse,hashlib,json,os,shutil,subprocess,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);mode=p.add_mutually_exclusive_group(required=True)
mode.add_argument('--seed',type=Path,help='Aether seed: run the independent self-compilation fixed point')
mode.add_argument('--compiler',type=Path,help='Reuse an already validated compiler; run language cases only')
p.add_argument('--sanitized-compiler',type=Path,help='Reuse the outer pipeline sanitizer compiler')
p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--no-sanitize',action='store_true');a=p.parse_args()
r=Path(__file__).resolve().parent.parent;base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True);w=Path(tempfile.mkdtemp(prefix='run-',dir=base));(base/'latest.json').write_text(json.dumps({'run_dir':str(w),'results':str(w/'results.json')},indent=2));source=w/'source'
if a.seed:shutil.copytree(r/'src_aether/core',source)
compiler_input=(a.seed or a.compiler).resolve()
(w/'compiler.json').write_text(json.dumps({'mode':'seed' if a.seed else 'compiler','path':str(compiler_input),'sha256':hashlib.sha256(compiler_input.read_bytes()).hexdigest(),'sanitized_compiler':str(a.sanitized_compiler.resolve()) if a.sanitized_compiler else None},indent=2))
fixtures=w/'fixtures';shutil.copytree(r/'tests/regressions/bootstrap/methods',fixtures);lib=w/'stdlib';shutil.copytree(r/'stdlib',lib)
(w/'sources.json').write_text(json.dumps({str(x.relative_to(w)):hashlib.sha256(x.read_bytes()).hexdigest() for directory in [source,fixtures,lib] for x in sorted(directory.rglob('*')) if x.is_file()},indent=2))
env={**os.environ,'TMPDIR':str(w),'UBSAN_OPTIONS':'halt_on_error=1','ASAN_OPTIONS':'detect_stack_use_after_return=1'};records=[]
def run(label,args,status=0,contains=None):
 q=subprocess.run(list(map(str,args)),capture_output=True,env=env,timeout=180);out=q.stdout.decode(errors='replace');err=q.stderr.decode(errors='replace');records.append(dict(label=label,args=list(map(str,args)),status=q.returncode,stdout=out,stderr=err));(w/'results.json').write_text(json.dumps(records,indent=2));assert q.returncode==status,(label,q.returncode,out,err)
 if contains:assert contains in out+err,(label,out,err)
 return q
main=source/'main.aeth'
if a.seed:
 s1=w/'stage1';s2=w/'stage2';s3=w/'stage3'
 run('Aether seed builds methods',[compiler_input,'build',main,'--backend','c','-O2','-o',s1]);run('methods self LLVM stage2',[s1,'build',main,'--backend','llvm','-O2','-o',s2]);run('methods self LLVM stage3',[s2,'build',main,'--backend','llvm','-O2','-o',s3])
 for stage in [s2,s3]:run(stage.name+'/IR',[stage,main,'--emit-llvm','-o',w/(stage.name+'.ll')])
 assert (w/'stage2.ll').read_bytes()==(w/'stage3.ll').read_bytes(),'LLVM self compiler fixed point mismatch'
else:s3=compiler_input
for name in ['values.aeth','modules.aeth','statistics.aeth','script.ath']:
 path=fixtures/name;run(name+'/check',[s3,'check',path,'-I',lib])
 for b in ['c','llvm']:
  for o in [0,2]:
   exe=w/f'{name}-{b}-{o}';run(f'{name}/{b}/{o}/build',[s3,'build',path,'-I',lib,'--backend',b,f'-O{o}','-o',exe]);q=run(f'{name}/{b}/{o}/run',[exe]);assert not q.stdout and not q.stderr
 if not a.no_sanitize:
  c=w/(name+'.c');exe=w/(name+'-san');run(name+'/san/emit',[s3,path,'-I',lib,'--emit-c','-o',c]);run(name+'/san/build',['clang','-std=c11','-O1','-fsanitize=address,undefined',c,'-o',exe]);run(name+'/san/run',[exe])
base='struct Box<T>{value:T} impl<T> Box<T>{fn new(value:T)->Self{return Self{value:value};} fn get(self:&Self)->T{return self.value;} fn get_ref(self:&Self)->&T{return &self.value;} fn set(self:&mut Self,value:T){self.value=value;}} '
negative={
 'unknown_impl':('impl Missing{fn value()->i32{return 7;}}fn main()->i32{return Missing::value();}','impl target must be a struct'),
 'unknown_empty_impl':('impl Missing{}fn main(){}','impl target must be a struct'),
 'private':('use library::Item;fn main()->i32{let value:Item<i64>=Item::create(7);return value.hidden() as i32;}','private'),
 'missing':(base+'fn main(){let value:Box<i64>=Box::new(7);value.missing();}','unknown inherent'),
 'immutable':(base+'fn main(){let value:Box<i64>=Box::new(7);value.set(8);}','immutable'),
 'raw_reference':(base+'fn main(){let mut value:Box<i64>=Box::new(7);let p:*Box<i64>=&mut value as *Box<i64>;p.get();}','raw pointer'),
 'receiver_type':('struct A{x:i64}struct B{x:i64}impl A{fn get(self:&B)->i64{return self.x;}}fn main(){}','self parameter'),
 'duplicate':('struct A{x:i64}impl A{fn f(){}}impl A{fn f(){}}fn main(){}','duplicate inherent'),
 'generic_shadow':('struct A<T>{x:T}impl<T> A<T>{fn f<T>(x:T){}}fn main(){}','shadows'),
 'static_instance':(base+'fn main(){let value:Box<i64>=Box::new(7);value.new(8);}','associated function requires'),
 'static_value':('struct A{x:i64}impl A{fn create()->i64{return 7;}}fn main(){let x:i64=A::create;}','parentheses'),
 'escape':(base+'fn leak()->&i64{let value:Box<i64>=Box::new(7);return value.get_ref();}fn main(){}','local storage'),
 'temporary_escape':(base+'fn main(){let reference:&i64=Box::new(7).get_ref();let value:i64=*reference;}','outlives its local storage'),
 'live_borrow':(base+'fn main(){let mut value:Box<i64>=Box::new(7);let reference:&i64=value.get_ref();value.set(8);let copied:i64=*reference;}','live reference loan'),
}
san=None
if not a.no_sanitize and a.sanitized_compiler:san=a.sanitized_compiler.resolve()
if not a.no_sanitize and a.seed and san is None:
 c=w/'compiler.c';san=w/'compiler-san';run('compiler/san/emit',[s3,main,'--emit-c','-o',c]);run('compiler/san/build',['clang','-x','c','-std=c11','-O1','-fsanitize=address,undefined',c,'-o',san])
for name,(text,message) in negative.items():
 path=w/(name+'.aeth');path.write_text(text);run(name+'/reject',[s3,'check',path,'-I',fixtures],1,message)
 if san:run(name+'/san-reject',[san,'check',path,'-I',fixtures],1,message)
phase='LLVM self-host fixed point' if a.seed else 'reused compiler; language cases only'
print(f'{len(records)} methods checks passed; {phase}; {w/"results.json"}')
