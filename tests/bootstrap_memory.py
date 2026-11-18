#!/usr/bin/env python3
"""Self-hosted memory semantics: real intrinsics, pthreads, orders and no_std."""
import argparse, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--cc',default=os.environ.get('CC','clang'))
p.add_argument('--sanitize',action='store_true')
a=p.parse_args();root=Path(__file__).resolve().parents[1]
base=a.work_dir.resolve();base.mkdir(parents=True,exist_ok=True)
w=Path(tempfile.mkdtemp(prefix='run-',dir=base));compiler=a.compiler.resolve()
(base/'latest.json').write_text(json.dumps(dict(run_dir=str(w),results=str(w/'results.json'))))
cc=shutil.which(a.cc);assert cc,'native C compiler unavailable'
rows=[];env={**os.environ,'TMPDIR':str(w)}
meta=dict(compiler=str(compiler),sha256=hashlib.sha256(compiler.read_bytes()).hexdigest())
def record(name,ok,**facts):
 rows.append(dict(name=name,passed=bool(ok),**facts))
 (w/'results.json').write_text(json.dumps(dict(**meta,checks=rows),indent=2)+'\n')
 print(('PASS ' if ok else 'FAIL ')+name,flush=True)
 if not ok:raise SystemExit(name)
def run(name,command,status=0,diagnostic=None,quiet=False):
 command=list(map(str,command))
 try:r=subprocess.run(command,capture_output=True,timeout=60,env=env)
 except subprocess.TimeoutExpired:record(name,False,command=command,error='timeout')
 text=(r.stdout+r.stderr).decode(errors='replace')
 record(name,r.returncode==status and (diagnostic is None or diagnostic.lower() in text.lower()) and (not quiet or not r.stdout and not r.stderr),command=command,exit=r.returncode,stdout=r.stdout.decode(errors='replace'),stderr=r.stderr.decode(errors='replace'))
 return r
mem=root/'tests/regressions/memory';extra=root/'tests/regressions/bootstrap_memory'
run('pthread-platform',[cc,'-std=c11','-O2','-pthread','-c',mem/'pthreads.c','-o',w/'pthreads.o'])
for name in ['operations','threaded','values','qualified','typed_volatile']:
 source=(root/'tests/regressions/system' if name=='typed_volatile' else extra if name in ('values','qualified') else mem)/(name+'.aeth')
 run(name+'-check',[compiler,'check',source,'-I',root/'stdlib'])
 for backend in ['c','llvm']:
  for level in [0,2]:
   exe=w/f'{name}-{backend}-{level}'
   command=[compiler,'build',source,'-I',root/'stdlib','--backend',backend,f'-O{level}','-o',exe]
   if name=='threaded':command+=['--link-object',w/'pthreads.o','-l','pthread']
   run(exe.name+'-build',command);run(exe.name+'-run',[exe],quiet=True)
# Keep original sources and semantic diagnostic expectations unchanged.
for name,message in [('load_release','order'),('store_acquire','order'),('fence_relaxed','order'),('cas_stronger_failure','order'),('runtime_order','compile-time'),('wrong_pointer_width','type mismatch')]:
 run(name,[compiler,'check',mem/'invalid'/(name+'.aeth')],status=1,diagnostic=message)
invalid={
 'dynamic-qualifier':('interface V{fn read(self:&Self)->u32;}struct R{value:u32}impl V for R{fn read(self:&Self)->u32{self.value}}fn main(){let p:*volatile R=0 as *volatile R;let q:&dyn V=&*p as &dyn V;}','volatile qualifier'),
 'drop-qualifier':('fn main(){let p:*volatile u32=0 as *volatile u32;let q:*u32=p;}','type mismatch'),
 'generic-qualifier':('fn plain<T>(p:*T)->T{*p}fn main(){let p:*volatile u32=0 as *volatile u32;let n:u32=plain(p);}','qualifier'),
 'projected-qualifier':('struct Bank{value:u32}fn main(){let p:*volatile Bank=0 as *volatile Bank;let q:&mut u32=&mut p.value;}','type mismatch'),
 'pure-annotation':('@volatile fn f()->u32 pure{1}fn main(){}','effect'),
 'annotation-eof':('#![no_main] @volatile','declaration'),
 'annotation-static':('@volatile static X:u32=0;fn main(){}','function'),
 'duplicate-annotation':('@volatile @volatile fn f(){}fn main(){}','duplicate'),
 'reserved-function':('fn __atomic_load_u32(p:*u32)->u32{0}fn main(){}','reserved'),
 'reserved-extern':('extern "C"{fn __atomic_store_u8(p:*u8,v:u8);}fn main(){}','reserved'),
 'reserved-binding':('fn main(){let __volatile_read_u8:i32=0;}','reserved'),
 'pure-atomic':('fn f(p:*u8)->u8 pure{__atomic_load_u8(p)}fn main(){}','effect'),
 'pure-volatile':('fn f(p:*u8)->u8 pure{__volatile_read_u8(p)}fn main(){}','effect'),
 'write-only-cas':('fn f(p:*u8)->bool effect[write]{__atomic_compare_exchange_u8(p,1,2)}fn main(){}','effect'),
 'read-only-fence':('fn f() effect[read]{__atomic_thread_fence();}fn main(){}','effect'),
 'wrong-value':('fn main(){let p:*u8=0 as *u8;let v:u32=1;__atomic_store_u8(p,v);}','type mismatch'),
 'arity':('fn main(){__atomic_thread_fence(1);}','too many'),
 'negative-order':('fn main(){__atomic_thread_fence_explicit(-1);}','order'),
 'large-order':('fn main(){__atomic_thread_fence_explicit(5);}','order'),
 'side-effect-order':('fn order()->i32{1}fn main(){__atomic_thread_fence_explicit(order());}','compile-time'),
}
for name,(text,message) in invalid.items():
 f=w/(name+'.aeth');f.write_text(text);run(name,[compiler,'check',f],status=1,diagnostic=message)
# All 25 success/failure combinations: reject release failures and stronger
# failures, rather than treating memory order values as ordered integers.
for success in range(5):
 for failure in range(5):
  allowed=failure in ({0},{0,1},{0},{0,1},{0,1,4})[success]
  f=w/f'cas-{success}-{failure}.aeth'
  f.write_text(f'fn f(p:*u32)->bool{{__atomic_compare_exchange_u32_explicit(p,0,1,{success},{failure})}}fn main(){{}}')
  run(f.stem,[compiler,'check',f],status=0 if allowed else 1,diagnostic=None if allowed else 'order')
for backend in ['c','llvm']:
 obj=w/f'freestanding-{backend}.o'
 run(obj.stem,[compiler,'build',extra/'freestanding.aeth','--backend',backend,'-O2','--emit-obj','-o',obj])
 r=run(obj.stem+'-symbols',['nm','-u',obj]);record(obj.stem+'-no-runtime-dependency',not r.stdout.strip())
 ir=w/('operations.'+('ll' if backend=='llvm' else 'c'))
 run(backend+'-emit',[compiler,'build',mem/'operations.aeth','-I',root/'stdlib','--backend',backend,'--emit-llvm' if backend=='llvm' else '--emit-c','-o',ir])
 text=ir.read_text()
 patterns=([r'load volatile i8\b',r'store volatile i64\b',r'atomicrmw add\b',r'atomicrmw sub\b',r'cmpxchg[^\n]*acq_rel acquire',r'load atomic[^\n]*acquire',r'store atomic[^\n]*release',r'fence seq_cst',r'fence acquire',r'fence release'] if backend=='llvm' else [r'__atomic_load_n\(',r'__atomic_store_n\(',r'__atomic_fetch_add\(',r'__atomic_fetch_sub\(',r'__atomic_compare_exchange_n\(',r'volatile struct __attribute__\(\(packed\)\)'])
 missing=[pattern for pattern in patterns if not re.search(pattern,text)]
 record(backend+'-real-memory-instructions',not missing,missing=missing)
typed_ir=w/'qualified.ll'
run('qualified-emit',[compiler,'build',extra/'qualified.aeth','--emit-llvm','-o',typed_ir])
text=typed_ir.read_text()
def body(name):
 match=re.search(r'(?ms)^define[^\n]*@'+name+r'\(.*?^}',text)
 record(name+'-present',match is not None)
 return match[0]
record('qualified-two-preserved',len(re.findall(r'load volatile i32\b',body('qualified_two')))==2)
local=body('local_storage')
record('annotated-storage',all(re.search(pattern,local) for pattern in [r'store volatile ptr',r'load volatile ptr',r'store volatile i32',r'load volatile i32']))
# The zero-sized pointee has no memory access even for an arbitrary sentinel.
unit=body('qualified_unit')
record('unit-no-volatile-access',not re.search(r'(load|store) volatile',unit))
if a.sanitize:
 emitted=w/'values-sanitized.c'
 run('sanitize-emit',[compiler,'build',extra/'values.aeth','--emit-c','-o',emitted])
 run('sanitize-build',[cc,'-O1','-fsanitize=address,undefined','-fno-sanitize-recover=all',emitted,'-o',w/'values-sanitized'])
 run('sanitize-run',[w/'values-sanitized'],quiet=True)
 for name in ['typed_volatile','qualified']:
  source=(root/'tests/regressions/system' if name=='typed_volatile' else extra)/(name+'.aeth');emitted=w/(name+'-san.c');exe=w/(name+'-san')
  run(name+'-san-emit',[compiler,'build',source,'--emit-c','-o',emitted]);run(name+'-san-build',[cc,'-O1','-fsanitize=address,undefined','-fno-sanitize-recover=all',emitted,'-o',exe]);run(name+'-san-run',[exe],quiet=True)
print(f'{len(rows)} memory checks passed')
