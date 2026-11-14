#!/usr/bin/env python3
"""Target-libc errno access through canonical typed compiler operations."""
import argparse, json, os, re, shutil, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True); p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',default=['c','llvm']); a=p.parse_args()
w=a.work_dir.resolve(); w.mkdir(parents=True,exist_ok=True); c=a.compiler.resolve(); records=[]
def run(name,cmd,fail=False):
 r=subprocess.run(list(map(str,cmd)),text=True,capture_output=True,timeout=30)
 records.append(dict(name=name,command=list(map(str,cmd)),exit=r.returncode,stdout=r.stdout,stderr=r.stderr))
 (w/'results.json').write_text(json.dumps(records,indent=2))
 assert (r.returncode!=0 if fail else r.returncode==0),str(records[-1])
 return r
source=Path(__file__).resolve().parent/'regressions/platform/errno.aeth'
for backend in a.backends:
 for level in [0,2]:
  binary=w/f'errno-{backend}-{level}'
  run('build errno',[c,source,'--backend',backend,f'-O{level}','-o',binary]);run('run errno',[binary])
for name,source_text in {
 'arity':'fn main(){__errno_get(1);}',
 'pure':'fn value()->i32 pure {__errno_get()} fn main(){}',
 'redefine':'fn __errno_get()->i32{1} fn main(){}',
 'foreign':'extern "C"{fn __errno_get()->i32;} fn main(){}',
 'shadow':'fn main(){let __errno_get:i32=2;}',
 'wrong_type':'fn main(){let error:bool=__errno_get();}',
}.items():
 path=w/(name+'.aeth'); path.write_text(source_text);run('reject '+name,[c,'check',path],True)
if 'llvm' in a.backends:
 for target in ['x86_64-unknown-linux-gnu','aarch64-linux-android','x86_64-pc-windows-msvc']:
  run('cross object '+target,[c,source,'--backend','llvm','--emit-obj','--target',target,'-o',w/(target+'.o')])
 # Inspect the actual text relocations, rather than merely asserting which
 # target-machine option was selected. The same source must retain static
 # address materialization on the freestanding ELF target.
 readobj=os.environ.get('LLVM_READOBJ') or shutil.which('llvm-readobj-21') or shutil.which('llvm-readobj')
 if not readobj:
  prefixes=[os.environ.get('LLVM_SYS_211_PREFIX',''),'/opt/homebrew/opt/llvm@21','/opt/homebrew/opt/llvm']
  for prefix in prefixes:
   tool=Path(prefix)/'bin/llvm-readobj'
   if prefix and tool.is_file():readobj=str(tool);break
 if not readobj:
  config=shutil.which('llvm-config-21') or shutil.which('llvm-config')
  if config:
   directory=subprocess.run([config,'--bindir'],text=True,capture_output=True,check=True).stdout.strip()
   tool=Path(directory)/'llvm-readobj'
   if tool.is_file():readobj=str(tool)
 assert readobj,'LLVM object regression requires llvm-readobj (or LLVM_READOBJ)'
 literal=source.with_name('literal_address.aeth')
 for target,pic in [('x86_64-unknown-linux-gnu',True),('x86_64-unknown-none-elf',False)]:
  for level in [0,2]:
   obj=w/f'literal-{target}-O{level}.o'
   run(f'literal object {target}/O{level}',[c,literal,'--backend','llvm','--emit-obj','--target',target,f'-O{level}','-o',obj])
   relocations=run(f'literal relocations {target}/O{level}',[readobj,'--relocations',obj]).stdout
   absolute=re.findall(r'\bR_X86_64_(?:32S?|64)\b',relocations)
   if pic:
    assert not absolute,f'hosted Linux emitted absolute text addresses: {relocations}'
    assert 'R_X86_64_PC32' in relocations or 'R_X86_64_GOT' in relocations,relocations
   else:
    assert absolute,f'freestanding ELF lost static address materialization: {relocations}'
# Compile the C entrypoint with the stricter pointer diagnostics of newer clang.
main=w/'main_args.aeth';main.write_text('fn main(argc:i32,argv:**u8)->i32{if argc!=2{return 1;} if argv[1][0]!=120{return 2;} return 0;}')
main_c=w/'main_args.c';run('emit argv C',[c,main,'--emit-c','-o',main_c])
run('strict argv C ABI',['clang','-std=c11','-Werror=incompatible-pointer-types',main_c,'-o',w/'main_args'])
run('run argv C ABI',[w/'main_args','x'])
ffi=w/'foreign_api.aeth';ffi.write_text('extern "C" {fn abs(value:i32)->i32;}')
foreign=w/'foreign_import.aeth';foreign.write_text('use foreign_api::abs; fn main()->i32{if abs(-3)!=3{return 1;}return 0;}')
for backend in a.backends:
 binary=w/f'foreign-{backend}'
 run('foreign module import',[c,foreign,'--backend',backend,'-o',binary]);run('foreign module execute',[binary])
print(f'{len(records)} platform checks passed')
