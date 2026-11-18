#!/usr/bin/env python3
"""Runtime closed-world interface dispatch and typed Script/Core imports."""
import argparse,hashlib,json,subprocess,re,shutil
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm']);a=p.parse_args()
compiler=a.compiler.resolve();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True);root=Path(__file__).resolve().parents[1];records=[]
def run(command,expected=0,stdout=None):
 command=list(map(str,command));r=subprocess.run(command,capture_output=True,text=True,timeout=30)
 records.append(dict(command=command,status=r.returncode,expected=expected,stdout=r.stdout,stderr=r.stderr))
 (work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),checks=records),indent=2))
 assert r.returncode==expected,(command,r.stdout,r.stderr)
 if stdout is not None:assert r.stdout==stdout and not r.stderr,(command,r.stdout,r.stderr)
 return r
(work/'private.aeth').write_text('pub interface HiddenValue{fn read(self:&Self)->i64;}struct Secret{value:i64}static VALUE:Secret=Secret{value:37};impl HiddenValue for Secret{fn read(self:&Self)->i64{self.value}}pub fn hidden()->&dyn HiddenValue{&VALUE as &dyn HiddenValue}')
private=work/'private.ath';private.write_text('from private import HiddenValue, hidden\ndef main():\n    value = hidden()\n    if value.read() != 37:\n        return 1\n    return 0\n')
(work/'aliases.aeth').write_text('pub const N:usize=2;pub fn next(value:i64)->i64{value+1}pub fn buffer()->[i64;N+1]{[4,5,6]}')
alias=work/'aliases.ath';alias.write_text('from aliases import next as print, buffer\ndef main():\n    data = buffer()\n    return print(data[0]) - 5\n')
sources=[(alias,''),(root/'tests/regressions/dynamic/borrowed.aeth',''),(root/'tests/regressions/dynamic/imports.ath','typed imports and dyn ok\n'),(private,'')]
for i,(source,stdout) in enumerate(sources):
 for backend in a.backends:
  for level in [0,2,3]:
   binary=work/f'{i}-{backend}-{level}';run([compiler,source,'--backend',backend,'-O',level,'-o',binary]);run([binary],stdout=stdout)
prefix='interface V{fn get(self:&Self)->i64;fn put(self:&mut Self);}struct A{x:i64}impl V for A{fn get(self:&Self)->i64{self.x}fn put(self:&mut Self){self.x=2;}}'
negative={
 'missing_impl':prefix+'struct B{x:i64}fn main(){let b:B=B{x:1};let v:&dyn V=&b as &dyn V;}',
 'raw_pointer':prefix+'fn main(){let p:*A=0 as *A;let v:&dyn V=p as &dyn V;}',
 'shared_mutation':prefix+'fn main(){let a:A=A{x:1};let v:&dyn V=&a as &dyn V;v.put();}',
 'immutable_object':prefix+'fn main(){let mut a:A=A{x:1};let v:&mut dyn V=&mut a as &mut dyn V;v.put();}',
 'exclusive_copy':prefix+'fn main(){let mut a:A=A{x:1};let mut v:&mut dyn V=&mut a as &mut dyn V;let mut copy:&mut dyn V=v;v.put();copy.put();}',
 'escape':prefix+'fn leak()->&dyn V{let a:A=A{x:1};&a as &dyn V}fn main(){}',
 'opaque_pattern':prefix+'fn main(){let a:A=A{x:1};let v:&dyn V=&a as &dyn V;match v{Impl0(value)=>{value.x;}}}',
 'object_safety':'interface V{fn clone(self:&Self)->Self;}struct A{x:i64}impl V for A{fn clone(self:&Self)->Self{A{x:self.x}}}fn main(){let a:A=A{x:1};let v:&dyn V=&a as &dyn V;}',
 'unsized':prefix+'fn wrong(value:dyn V){}fn main(){}',
 'foreign':prefix+'extern "C"{fn foreign(v:&dyn V);}fn main(){}',
 'export':prefix+'#[no_mangle] fn exported(v:&dyn V){}fn main(){}',
 'nested_foreign':prefix+'struct Wrapped{v:&dyn V}extern "C"{fn foreign(v:*Wrapped);}fn main(){}',
 'union_foreign':prefix+'type Ref=&dyn V;union Wrapped{v:Ref,x:i64}extern "C"{fn foreign(v:*Wrapped);}fn main(){}',
 'union_export':prefix+'union Wrapped{v:&dyn V,x:i64}#[no_mangle] fn exposed(v:*Wrapped){}fn main(){}',
 'effects':prefix+'fn call(v:&dyn V)->i64 pure{v.get()}fn main(){}',
 'script_private_import':'from private import Secret\ndef main():\n    return 0\n',
 'script_unknown_method':'from private import hidden\ndef main():\n    value = hidden()\n    return value.missing()\n',
}
# Explicit public record imports preserve a separate direct-method privacy boundary.
(work/'public.aeth').write_text('pub interface V{fn read(self:&Self)->i64;}pub struct A{x:i64}impl V for A{fn read(self:&Self)->i64{self.x}}pub fn make()->A{A{x:5}}pub fn view(a:&A)->&dyn V{a as &dyn V}')
negative['private_direct']='use public::{make};fn main(){let a:public::A=make();a.read();}'
negative['script_escape']='from public import V, make, view\ndef leak() -> Dyn[V]:\n    local = make()\n    return view(ref(local))\ndef main():\n    return 0\n'
negative['script_wrong_argument']='from public import view\ndef main():\n    value = 3\n    view(ref(value))\n    return 0\n'
for name,source in negative.items():
 path=work/f'{name}.{ "ath" if name.startswith("script_") else "aeth"}';path.write_text(source)
 checked=run([compiler,'check','--error-format','json',path],1)
 built=run([compiler,'build','--error-format','json',path,'--backend','c','-o',work/'rejected'],1)
 assert json.loads(checked.stderr)==json.loads(built.stderr),(name,checked.stderr,built.stderr)
# Source references to an implementation helper remain undefined, even when a
# previous compilation revealed the private helper's physical emitted spelling.
probe=work/'hygiene.aeth';probe.write_text('use public::{make,view};fn main(){let a:public::A=make();let v:&dyn public::V=view(&a);v.read();}')
ir=work/'hygiene.ir';run([compiler,probe,'--backend','c','--emit-ir','-O0','-o',ir])
symbol=re.search(r'__aeth_dyn_implementation_\d+',ir.read_text()).group()
probe.write_text('use public::{make,view};fn main(){let a:public::A=make();let v:&dyn public::V=view(&a);v.read();'+symbol+'(&a);}')
r=run([compiler,'check','--error-format','json',probe],1);assert json.loads(r.stderr)['code']=='E2001',r.stderr
if all(shutil.which(tool) for tool in ['node','clang','wasm-ld']):
 wasm_source=work/'borrowed32.aeth'
 wasm_source.write_text('#![no_std] #![no_main]\n'+(root/'tests/regressions/dynamic/borrowed.aeth').read_text().replace('fn main()->i32','#[no_mangle] fn verify_dyn()->i32'))
 for backend in a.backends:
  obj=work/f'dyn-{backend}.o';wasm=obj.with_suffix('.wasm')
  run([compiler,wasm_source,'--target','wasm32-unknown-unknown','--backend',backend,'--cc','clang','--emit-obj','-O2','-o',obj])
  run(['wasm-ld','--no-entry','--export=verify_dyn',obj,'-o',wasm])
  run(['node','-e',"const fs=require('fs');process.exit(new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1]))).exports.verify_dyn());",wasm])
print(f'{len(records)} dynamic/import commands passed')
