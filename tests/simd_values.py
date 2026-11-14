#!/usr/bin/env python3
"""Native SIMD semantics, unaligned memory, lane traps, and explicit foreign ABI scope."""
import argparse,json,os,platform,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--backends',nargs='+',default=['c','llvm']);a=p.parse_args()
root=Path(__file__).resolve().parent/'regressions/simd';work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True);compiler=a.compiler.resolve();env={**os.environ,'TMPDIR':str(work)};records=[];cc=os.environ.get('CC','clang')
def run(label,cmd,fail=False):
 r=subprocess.run(list(map(str,cmd)),text=True,capture_output=True,env=env,timeout=40)
 records.append(dict(label=label,command=list(map(str,cmd)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr));(work/'results.json').write_text(json.dumps(records,indent=2))
 assert (r.returncode!=0 if fail else r.returncode==0),f'{label}: {r.returncode}\n{r.stderr}'
 return r
obj=work/'abi.o';run('C ABI companion',[cc,'-O2','-c',root/'abi.c','-o',obj])
for name in ['values','abi']:
 source=root/(name+'.aeth');run(name+'/check',[compiler,'check',source])
 for backend in a.backends:
  if name=='abi' and backend=='llvm' and not(platform.system()=='Darwin' and platform.machine()=='arm64'):
   r=run('foreign vector ABI unsupported',[compiler,source,'--backend',backend,'--emit-obj','-o',work/'unsupported.o'],True)
   assert 'foreign vector ABI' in r.stderr;continue
  for level in [0,2]:
   binary=work/f'{name}-{backend}-{level}';cmd=[compiler,source,'--backend',backend,f'-O{level}','-o',binary]
   if name=='abi':cmd += ['--link-object',obj]
   run(f'{name}/{backend}/O{level}/build',cmd);run(f'{name}/{backend}/O{level}/run',[binary])
 c=work/f'{name}.c';binary=work/f'{name}-sanitized';run(name+'/emit C',[compiler,source,'--emit-c','-o',c])
 cmd=[cc,'-std=c11','-O1','-g','-fsanitize=address,undefined',c,'-o',binary]
 if name=='abi':cmd += [root/'abi.c']
 run(name+'/sanitizer build',cmd);run(name+'/sanitizer run',[binary])
for label,text in {
 'ambiguous_vector_comparison':'fn main(){let x:bool=__simd_f32x4_splat(1.0)==__simd_f32x4_splat(1.0);}',
 'integer_vector_division':'fn main(){let x:i32x4=__simd_i32x4_splat(1)/__simd_i32x4_splat(1);}',
 'wrong_shape':'fn main(){let x:f32x4=__simd_f32x4_add(__simd_i32x4_splat(1),__simd_f32x4_splat(1.0));}',
 'wrong_lane_type':'fn main(){let x:f32=__simd_f32x4_extract(__simd_f32x4_splat(1.0),true);}',
 'unsafe_pointer':'fn main(){let x:f32x4=__simd_f32x4_load(0 as *f32);}',
 'foreign_redefinition':'extern "C" {fn __simd_f32x4_new(a:f32)->i64;} fn main(){}',
}.items():
 source=work/(label+'.aeth');source.write_text(text);run(label+'/reject',[compiler,'check',source],True)
source=work/'lane_trap.aeth';source.write_text('fn main()->i32{let x:f32=__simd_f32x4_extract(__simd_f32x4_splat(1.0),4);return x as i32;}')
for backend in a.backends:
 for level in [0,2]:
  binary=work/f'lane-{backend}-{level}';run('lane trap/build',[compiler,source,'--backend',backend,f'-O{level}','-o',binary]);r=run('lane trap/run',[binary],True);assert r.returncode<0,'out-of-range lane must trap, not return an arbitrary scalar'
if 'llvm' in a.backends:
 for label,text in {
  'vector_aggregate_abi':'struct S{value:f32x4} extern "C"{fn vector_holder(value:S)->S;} fn main()->i32{return 0;}',
  'wide_vector_abi':'extern "C"{fn wide_vector(value:f32x8)->f32x8;} fn main()->i32{return 0;}',
 }.items():
  source=work/(label+'.aeth');source.write_text(text)
  r=run(label+'/explicit boundary',[compiler,source,'--backend','llvm','--emit-obj','-o',work/(label+'.o')],True)
  assert 'foreign vector ABI' in r.stderr
 for target in ['x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu']:
  run('internal vector cross object/'+target,[compiler,root/'values.aeth','--backend','llvm','--emit-obj','--target',target,'-O2','-o',work/(target+'.o')])
print(f'{len(records)} SIMD checks passed; evidence: {work / "results.json"}')
