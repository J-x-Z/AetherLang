#!/usr/bin/env python3
"""Self-hosted structural tuple values, layout, inference and ABI boundaries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--reference-compiler',type=Path)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--formats',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--sanitize',action='store_true')
p.add_argument('--cc',default=os.environ.get('CC','clang'))
a=p.parse_args();root=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
compiler=a.compiler.resolve();cc=shutil.which(a.cc);records=[];env={**os.environ,'TMPDIR':str(w)}
if not cc:raise SystemExit('native C compiler unavailable')

def record(name,passed,**facts):
    records.append(dict(name=name,passed=bool(passed),**facts));(w/'results.json').write_text(json.dumps(records,indent=2)+'\n')
    print(('PASS ' if passed else 'FAIL ')+name,flush=True)
    if not passed:raise SystemExit(name)

def run(name,cmd,status=0,stdout=None,diagnostic=None):
    cmd=list(map(str,cmd));started=time.monotonic()
    try:r=subprocess.run(cmd,cwd=w,env=env,capture_output=True,timeout=45)
    except subprocess.TimeoutExpired:record(name,False,command=cmd,error='timeout')
    text=(r.stdout+r.stderr).decode(errors='replace')
    record(name,r.returncode==status and (stdout is None or r.stdout==stdout) and (diagnostic is None or diagnostic in text),
           command=cmd,exit=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'),elapsed_seconds=time.monotonic()-started)
    return r

(w/'compiler.json').write_text(json.dumps(dict(path=str(compiler),sha256=hashlib.sha256(compiler.read_bytes()).hexdigest()),indent=2))
module=w/'modules';module.mkdir(exist_ok=True)
(module/'left.aeth').write_text('pub type Pair=(i32,i64);pub fn make()->Pair{(20,22)}')
(module/'right.aeth').write_text('pub type Pair=(i32,i64);pub fn sum(pair:Pair)->i64{pair.0 as i64+pair.1}')
(module/'main.aeth').write_text('use left::make;use right::{Pair,sum};extern "C"{fn opaque(p:*(i32,i64));}fn main()->i32{let pair:Pair=make();if sum(pair)!=42{return 1;}0}')
for name,source,expected in [('values',root/'tests/regressions/bootstrap/tuple_values.aeth',b'tuple-values=ok\n'),('modules',module/'main.aeth',b'')]:
    run(name+'-check',[compiler,source,'-I',root/'stdlib','--check'])
    if a.reference_compiler:run(name+'-reference-check',[a.reference_compiler.resolve(),'check',source,'-I',root/'stdlib'])
    for fmt in a.formats:
        emitted=w/(name+('.ll' if fmt=='llvm' else '.c'))
        run(name+'-'+fmt+'-emit',[compiler,source,'-I',root/'stdlib',*(['--emit-llvm'] if fmt=='llvm' else []),'-o',emitted])
        for opt in ['0','2']:
            binary=w/(name+'-'+fmt+opt)
            run(name+'-'+fmt+opt+'-build',[cc,'-O'+opt,*(['-fsanitize=address,undefined','-fno-sanitize-recover=all'] if a.sanitize else []),emitted,'-o',binary])
            run(name+'-'+fmt+opt+'-run',[binary],stdout=expected)

symbols_source=w/'symbols.aeth';symbols_source.write_text('fn make(x:i32)->(i32,){(x,)}fn main()->i32{let pair:(i32,)=make(0);return pair.0;}')
symbols=json.loads(run('tuple-symbols',[compiler,'symbols',symbols_source]).stdout)
record('tuple-symbol-types',any(item['name']=='pair' and item['type_name']=='pair: (i32,)' for item in symbols['symbols']),actual=symbols)
ids={item['id'] for item in symbols['symbols']}
record('tuple-symbol-references',all(item['symbol'] in ids for item in symbols['references']),actual=symbols)

invalid={
 'arity':('fn bad(){let t:(i32,i32)=(1,);}','arity'),
 'element-type':('fn bad(){let t:(i32,bool)=(1,2);}','type mismatch'),
 'parentheses':('fn bad(){let t:(i32,)=1;}','type mismatch'),
 'index-range':('fn bad(){let t:(i32,)=(1,);let x:i32=t.1;}','unknown struct field'),
 'named-field':('fn bad(){let t:(i32,)=(1,);let x:i32=t.value;}','unknown struct field'),
 'immutable':('fn bad(){let t:(i32,)=(1,);t.0=2;}','immutable'),
 'shared-write':('fn bad(t:&(i32,)){t.0=2;}','shared reference'),
 'temporary-address':('fn bad()->&(i32,){&(1,)}','address requires a place'),
 'temporary-field-address':('fn bad()->&i32{&(1,).0}','address requires a place'),
 'foreign-param':('extern "C"{fn bad(t:(i32,i64));}','tuple values have no stable C ABI'),
 'foreign-return':('extern "C"{fn bad()->(i32,i64);}','tuple values have no stable C ABI'),
 'foreign-nested':('struct R{data:[(i32,i64);1]}extern "C"{fn bad(t:R);}','tuple values have no stable C ABI'),
 'foreign-static':('extern "C"{static BAD:(i32,i64);}','tuple values have no stable C ABI'),
 'foreign-export':('#[no_mangle]fn bad(t:(i32,i64)){}','tuple values have no stable C ABI'),
 'variadic':('extern "C"{fn variadic(first:i32,...);}fn bad(){variadic(1,(2,3));}','variadic arguments'),
 'nominal-distinction':('struct Pair{x:i32,y:i64}fn bad(){let t:(i32,i64)=Pair{x:1,y:2};}','type mismatch'),
 'generic-arity':('fn same<T>(a:(T,T)){}fn bad(){same((1,));}','arity'),
 'generic-types':('fn same<T>(a:(T,T)){}fn bad(){same((1,true));}','conflicting generic'),
}
for name,(text,diagnostic) in invalid.items():
    source=w/(name+'.aeth');source.write_text(text)
    run(name+'-check',[compiler,source,'--check'],status=1,diagnostic=diagnostic)
    if a.reference_compiler:run(name+'-reference-check',[a.reference_compiler.resolve(),'check',source],status=1)
    for fmt in a.formats:
        out=w/(name+'.'+fmt);out.write_bytes(b'existing user artifact\n')
        run(name+'-'+fmt+'-reject',[compiler,source,*(['--emit-llvm'] if fmt=='llvm' else []),'-o',out],status=1,diagnostic=diagnostic)
        record(name+'-'+fmt+'-preserved',out.read_bytes()==b'existing user artifact\n')
# Copyable tuple storage does not silently bypass the current reference/owner
# projection restrictions. These are recorded as migration boundaries.
for name,text in {
 'reference-payload':'fn bad(x:&i32)->(&i32,){(x,)}',
 'owned-payload':'struct R{fd:i32}fn bad(x:own R){let t:(own R,)=(x,);}',
 'temporary-owner-field':'struct R{fd:i32}fn make()->own R{unsafe{return R{fd:3} as own R;}}@drop fn release(x:own R){}fn bad(){let fd:i32=make().fd;}',
}.items():
    source=w/(name+'.aeth');source.write_text(text)
    run(name+'-boundary',[compiler,source,'--check'],status=1,diagnostic=('addressable place' if name=='temporary-owner-field' else 'aggregate'))
print(f'{len(records)} tuple checks passed')
