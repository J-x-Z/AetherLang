#!/usr/bin/env python3
"""Owned-resource semantic agreement and exactly-once native scope cleanup."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--seed',type=Path,required=True)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--fixtures',type=Path,default=Path(__file__).resolve().parent/'regressions/bootstrap/owned')
a=p.parse_args();work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
core=a.compiler.resolve();seed=a.seed.resolve();records=[]
def run(command,status=0):
    command=list(map(str,command))
    r=subprocess.run(command,capture_output=True,text=True,timeout=60,env={**os.environ,'TMPDIR':str(work)})
    records.append(dict(command=command,status=r.returncode,expected=status,stdout=r.stdout,stderr=r.stderr))
    (work/'results.json').write_text(json.dumps(dict(core_sha256=hashlib.sha256(core.read_bytes()).hexdigest(),seed_sha256=hashlib.sha256(seed.read_bytes()).hexdigest(),checks=records),indent=2))
    assert r.returncode==status,(command,r.returncode,r.stdout,r.stderr)
    return r
for name in ['cleanup','descriptors','error_paths','heap','packed','temporary_arguments']:
    source=a.fixtures/f'{name}.aeth'
    run([seed,'check',source]);run([core,source,'--check'])
    for backend in ['c','llvm']:
        generated=work/f'{name}.{ "c" if backend=="c" else "ll"}'
        run([core,source,'-o',generated]+(['--emit-llvm'] if backend=='llvm' else []))
        for level in [0,2]:
            binary=work/f'{name}-{backend}-{level}'
            run(['clang',f'-O{level}',generated,'-lm','-o',binary])
            r=run([binary]);assert r.stdout=='' and r.stderr==''
        sanitized=work/f'{name}-{backend}-sanitized'
        run(['clang','-O1','-g','-fsanitize=address,undefined','-fno-sanitize-recover=all',generated,'-lm','-o',sanitized])
        r=run([sanitized]);assert r.stdout=='' and r.stderr==''
prefix='struct R{fd:i32}fn make()->own R{unsafe{return R{fd:7} as own R;}}'
negative={
 'duplicate_owner':prefix+'fn bad(){let a:own R=make();let b:own R=a;let fd:i32=a.fd;}',
 'write_moved_field':prefix+'fn bad(){let mut a:own R=make();let b:own R=a;a.fd=2;}',
 'borrow_moved_owner':prefix+'fn bad(){let a:own R=make();let b:own R=a;let view:&own R=&a;let fd:i32=view.fd;}',
 'branch_move':prefix+'fn use_it(x:own R){}fn bad(flag:bool){let a:own R=make();if flag{use_it(a);}let fd:i32=a.fd;}',
 'loop_move':prefix+'fn use_it(x:own R){}fn bad(flag:bool){let a:own R=make();while flag{use_it(a);}}',
 'move_during_borrow':prefix+'fn use_it(x:own R){}fn bad(){let a:own R=make();let view:&own R=&a;use_it(a);let fd:i32=view.fd;}',
 'move_out_of_borrow':prefix+'fn bad(view:&own R)->own R{return *view;}',
 'return_local_borrow':prefix+'fn bad()->&own R{let a:own R=make();return &a;}',
 'adopt_requires_unsafe':'struct R{fd:i32}fn bad()->own R{return R{fd:7} as own R;}',
 'release_requires_unsafe':prefix+'fn bad(value:own R)->R{return value as R;}',
 'live_replacement':prefix+'fn bad(){let mut a:own R=make();a=make();}',
 'copy_into_plain':prefix+'fn bad(){let a:own R=make();let plain:R=a;}',
 'primitive_owner':'fn bad(value:own i64){}',
 'destructor_arity':prefix+'@drop fn bad(a:own R,b:own R){}',
 'destructor_result':prefix+'@drop fn bad(a:own R)->i64{return 1;}',
 'duplicate_destructor':prefix+'@drop fn first(a:own R){}type Alias=R;@drop fn second(a:own Alias){}',
 'foreign_owner':prefix+'extern "C"{fn bad(a:own R);}',
 'borrowed_raw_field':'struct R{data:*i32}fn bad(value:&own R)->*i32{return value.data;}',
 'raw_field_requires_unsafe':'struct R{data:*i32}fn bad(value:own R)->*i32{return value.data;}',
 'nested_raw_field':'struct Inner{data:*i32}struct R{inner:Inner}fn bad(value:own R)->Inner{return value.inner;}',
}
for name,source in negative.items():
    path=work/f'{name}.aeth';path.write_text(source+'fn main()->i32{return 0;}')
    run([seed,'check',path],1);run([core,path,'--check'],1);run([core,path,'-o',work/'rejected.c'],1)
# Deliberately bounded surfaces; rejecting these must remain an ordinary error.
boundaries={
 'owner_array':prefix+'fn bad(){let a:own R=make();let values:[own R;1]=[a];}',
 'owner_record':prefix+'struct Box{value:own R}fn bad(){let a:own R=make();let b:Box=Box{value:a};}',
 'ignored_owner_result':prefix+'fn bad(){make();}',
 'generic_destructor':'struct Box<T>{value:T}@drop fn bad<T>(value:own Box<T>){}',
 'drop_on_record':'@drop struct Bad{x:i32}',
 'repr_on_drop':'@repr(C) @drop fn bad(){}',
 'drop_before_inner_attribute':'@drop #![no_main] fn bad(){}',
 'annotation_on_extern':'@drop extern "C"{fn bad();}',
}
for name,source in boundaries.items():
    path=work/f'{name}.aeth';path.write_text(source+'fn main()->i32{return 0;}');run([core,path,'--check'],1)
(work/'resource.aeth').write_text('pub struct Resource{fd:i32}')
for module in ['a','b']:
    (work/f'{module}.aeth').write_text('use resource::Resource;@drop fn destroy(value:own Resource){}pub fn marker(){}')
source=work/'duplicate_modules.aeth';source.write_text('use a::marker;use b::marker as other;fn main()->i32{return 0;}')
run([seed,'check',source],1);run([core,source,'--check'],1)
print(f'{len(records)} Core owned-resource commands passed')
