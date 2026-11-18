#!/usr/bin/env python3
"""Self-hosted borrowed dynamic dispatch, shared aggregate loans and Script imports."""
import argparse,hashlib,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--seed',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);a=p.parse_args();a.compiler=a.compiler.resolve();a.seed=a.seed.resolve()
root=Path(__file__).resolve().parents[1];work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True);records=[]
def run(command,status=0,stdout=None):
 command=list(map(str,command));r=subprocess.run(command,capture_output=True,text=True,timeout=60,env={**os.environ,'TMPDIR':str(work)})
 records.append(dict(command=command,status=r.returncode,expected=status,stdout=r.stdout,stderr=r.stderr));(work/'results.json').write_text(json.dumps(dict(compiler_sha256=hashlib.sha256(a.compiler.read_bytes()).hexdigest(),seed_sha256=hashlib.sha256(a.seed.read_bytes()).hexdigest(),checks=records),indent=2))
 assert r.returncode==status,records[-1]
 if stdout is not None:assert r.stdout==stdout and not r.stderr,records[-1]
 return r
(work/'private.aeth').write_text('pub interface V{fn read(self:&Self)->i64;}struct Secret{value:i64}static VALUE:Secret=Secret{value:37};impl V for Secret{fn read(self:&Self)->i64{self.value}}pub fn hidden()->&dyn V{&VALUE as &dyn V}')
(work/'private.ath').write_text('from private import V, hidden\ndef main():\n    value = hidden()\n    if value.read() != 37:\n        return 1\n    return 0\n')
(work/'public.aeth').write_text('pub interface V{fn read(self:&Self)->i64;}pub struct A{x:i64}impl V for A{fn read(self:&Self)->i64{self.x}}pub fn make()->A{A{x:5}}pub fn view(a:&A)->DynAlias{a as &dyn V}pub type DynAlias=&dyn V;')
sources=[(root/'tests/regressions/dynamic/borrowed.aeth',''),(root/'tests/regressions/dynamic/imports.ath','typed imports and dyn ok\n'),(root/'tests/regressions/bootstrap/dynamic/shared_refs.aeth',''),(work/'private.ath',''),(root/'tests/regressions/semantics/patterns_arrays.aeth','')]
for i,(source,stdout) in enumerate(sources):
 run([a.seed,'check',source]);run([a.compiler,'check',source])
 for backend in ['c','llvm']:
  generated=work/f'{i}.{ "c" if backend=="c" else "ll"}';run([a.compiler,source,'--emit-'+backend,'-o',generated])
  for level in ['0','2','san']:
   binary=work/f'{i}-{backend}-{level}';flags=['-O'+level] if level!='san' else ['-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all']
   run(['clang',*flags,generated,'-lm','-o',binary]);run([binary],stdout=stdout)
prefix='interface V{fn get(self:&Self)->i64;fn put(self:&mut Self);}struct A{x:i64}impl V for A{fn get(self:&Self)->i64{self.x}fn put(self:&mut Self){self.x=2;}}'
negative={
 'missing_impl':prefix+'struct B{x:i64}fn bad(){let b:B=B{x:1};let v:&dyn V=&b as &dyn V;}',
 'raw_pointer':prefix+'fn bad(p:*A){let v:&dyn V=p as &dyn V;}',
 'shared_mutation':prefix+'fn bad(){let a:A=A{x:1};let v:&dyn V=&a as &dyn V;v.put();}',
 'exclusive_copy':prefix+'fn bad(){let mut a:A=A{x:1};let mut v:&mut dyn V=&mut a as &mut dyn V;let mut copy:&mut dyn V=v;v.put();copy.put();}',
 'escape':prefix+'fn leak()->&dyn V{let a:A=A{x:1};&a as &dyn V}',
 'union_abi':prefix+'type Ref=&dyn V;union Wrapped{v:Ref,x:i64}extern "C"{fn bad(v:*Wrapped);}',
 'export_abi':prefix+'#[no_mangle]fn exported(v:&dyn V){}',
 'effects':prefix+'fn bad(v:&dyn V)->i64 pure{v.get()}',
 'contract_effect':'interface V{fn read(self:&Self)->i64 pure;}struct A{x:i64}impl V for A{fn read(self:&Self)->i64{self.x}}',
 'nested_alias':'fn bad(){let mut x:i64=1;let r:&i64=&x;let rr:& &i64=&r;x=2;let v:i64=**rr;}',
 'record_alias':'struct R{p:&i64}fn bad(){let mut x:i64=1;let r:R=R{p:&x};let outer:&R=&r;x=2;let v:i64=*outer.p;}',
 'capture_alias':'fn bad(){let mut x:i64=1;let r:&i64=&x;let f:_=||*r;x=2;f();}',
 'escape_record':'struct R{p:&i64}fn bad()->R{let x:i64=1;R{p:&x}}',
 'escape_closure':'fn bad()->&i64{let x:i64=1;let p:&i64=&x;let callback:_=||p;callback()}',
 'scope_array':'fn bad(x:&i64){let mut a:[&i64;1]=[x];{let y:i64=2;a=[&y];}let v:i64=*a[0];}',
 'private_direct':'use public::make;fn bad(){let a:public::A=make();a.read();}',
}
for name,text in negative.items():
 source=work/(name+'.aeth');source.write_text(text+'fn main()->i32{return 0;}');run([a.seed,'check',source],1);run([a.compiler,'check',source],1);run([a.compiler,'build',source,'--backend','c','-o',work/'rejected'],1)
for name,text in {
 'private_import':'from private import Secret\ndef main():\n    return 0\n',
 'escape':'from public import V, make, view\ndef leak() -> Dyn[V]:\n    local = make()\n    return view(ref(local))\ndef main():\n    return 0\n',
 'wrong_argument':'from public import view\ndef main():\n    value = 3\n    view(ref(value))\n    return 0\n',
 'unassigned_reference':'from public import make, view\ndef main():\n    value = make()\n    if False:\n        borrowed = view(ref(value))\n    return borrowed.read()\n',
}.items():
 source=work/('script_'+name+'.ath');source.write_text(text);run([a.seed,'check',source],1);run([a.compiler,'check',source],1);run([a.compiler,'build',source,'--backend','c','-o',work/'rejected'],1)
print(f'{len(records)} dynamic/shared-reference checks passed')
