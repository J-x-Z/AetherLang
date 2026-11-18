#!/usr/bin/env python3
"""Hosted fallback names and no_std in the 64-bit self-hosted compiler."""
import argparse, hashlib, json, os, signal, shutil, subprocess, time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True);p.add_argument('--seed',type=Path)
p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--sanitize',action='store_true')
p.add_argument('--formats',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--cc',default=os.environ.get('CC','clang'));a=p.parse_args()
r=Path(__file__).resolve().parents[1];w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True)
c=a.compiler.resolve();cc=shutil.which(a.cc);env={**os.environ,'TMPDIR':str(w)};rows=[]
if not cc:raise SystemExit('native C compiler unavailable')
(w/'compiler.json').write_text(json.dumps(dict(path=str(c),sha256=hashlib.sha256(c.read_bytes()).hexdigest()),indent=2))
def record(name,passed,**facts):
 rows.append(dict(name=name,passed=bool(passed),**facts));(w/'results.json').write_text(json.dumps(rows,indent=2)+'\n');print(('PASS ' if passed else 'FAIL ')+name,flush=True)
 if not passed:raise SystemExit(name)
def run(name,cmd,status=0,stdout=None,diagnostic=None):
 cmd=list(map(str,cmd));start=time.monotonic()
 try:q=subprocess.run(cmd,cwd=w,env=env,capture_output=True,timeout=50)
 except subprocess.TimeoutExpired:record(name,False,command=cmd,error='timeout')
 good=q.returncode in (-signal.SIGILL,-signal.SIGTRAP) if status=='trap' else q.returncode==status
 text=(q.stdout+q.stderr).decode(errors='replace');record(name,good and (stdout is None or q.stdout==stdout) and (diagnostic is None or diagnostic in text),command=cmd,status=q.returncode,stdout=q.stdout.decode(errors='replace'),stderr=q.stderr.decode(errors='replace'),seconds=time.monotonic()-start);return q
def source(name,text):
 f=w/(name+'.aeth');f.write_text(text);return f
positive={
 'hosted':(r/'tests/regressions/host_builtins/hosted.aeth',0,b'hello world\n-92233720368547758089223372036854775807\n'),
 'explicit':(r/'tests/regressions/host_builtins/explicit_names.aeth',0,b''),
 'hello':(r/'examples/hello.aeth',0,b'Hello, AetherLang!\n'),
 'parse-end':(r/'tests/regressions/host_builtins/parse_end.aeth',0,b''),
 'exit':(source('exit','fn main()->i32{println("before");exit(7);println("after");return 0;}'),7,b'before\n'),
 'assert':(source('assert','fn main()->i32{assert(false);return 0;}'),'trap',b''),
 'never-context':(source('never-context','fn result(flag:bool)->i32{if flag{exit(8)}else{42}}fn main()->i32{if result(false)!=42{return 1;}let value:i32=exit(9);println_i64(value as i64);0}'),9,b''),
 'no-std-explicit':(source('no-std-explicit','#![no_std]\n'+(r/'tests/regressions/host_builtins/explicit_names.aeth').read_text()),0,b''),
 'format-effects':(source('format-effects','static mut CALLS:i32=0;fn text()->*u8{CALLS+=1;"%n 100%"}fn output() effect[io,write,read]{println(text());}fn math()->bool effect[read]{strcmp("a","a")==0&&atof("1.5")==1.5}fn memory() effect[alloc]{let p:*u8=malloc(1);free(p);}fn main()->i32{output();memory();if CALLS!=1||!math(){return 1;}let u:()=print("");if puts("done")<0{return 2;}return 0;}'),0,b'%n 100%\ndone\n'),
 'never-let':(source('never-let','fn stop()->i32{let value:i32=exit(13);}fn main()->i32{stop()}'),13,b''),
 'never-short-circuit':(source('never-short-circuit','fn main()->i32{let skip:bool=false&&exit(14);if skip{return 1;}let take:bool=true&&exit(15);return 2;}'),15,b''),
 'local-names':(source('local-names','fn print(n:i32)->i32 pure{n+1}fn malloc(n:i32)->i32 pure{n+2}fn main()->i32{if print(1)!=2||malloc(1)!=3{return 1;}0}'),0,b''),
}
module=source('overrides','pub fn println(n:i32)->i32{n+1}')
positive['import-name']=(source('import-name','use overrides::println;fn main()->i32{if println(8)!=9{return 1;}0}'),0,b'')
for name,(f,status,out) in positive.items():
 run(name+'-check',[c,f,'--check'])
 if a.seed:run(name+'-reference',[a.seed.resolve(),'check',f])
 for fmt in a.formats:
  emitted=w/(name+('.ll' if fmt=='llvm' else '.c'));run(name+'-'+fmt+'-emit',[c,f,'--emit-llvm' if fmt=='llvm' else '--emit-c','-o',emitted])
  for opt in [0,2,3]:
   binary=w/(name+'-'+fmt+str(opt));flags=['-fsanitize=address,undefined','-fno-sanitize-recover=all'] if a.sanitize else []
   run(name+'-'+fmt+str(opt)+'-build',[cc,'-O'+str(opt),*flags,emitted,'-o',binary]);run(name+'-'+fmt+str(opt)+'-run',[binary],status=status,stdout=out)
response=run('hello-symbols',[c,'symbols',r/'examples/hello.aeth'])
symbols=json.loads(response.stdout)
record('hello-no-synthetic-declarations',[item['name'] for item in symbols['symbols']]==['main'])
invalid={
 'print-type':('fn main(){print(1);}','type mismatch'),
 'print-arity':('fn main(){println("x","y");}','too many'),
 'alloc-arity':('fn main(){alloc();}','too few'),
 'exit-type':('fn main(){let n:i64=0;exit(n);}','type mismatch'),
 'assert-type':('fn main(){assert(1);}','type mismatch'),
 'unit-return':('fn main()->i32{print("x")}','type mismatch'),
 'puts-return':('fn main(){let n:()=puts("x");}','type mismatch'),
 'shadow-local':('fn main(){let print:i32=0;print("x");}','not a closure'),
 'pure-output':('fn f() pure{println("x");}fn main(){}','effect'),
 'pure-read':('fn f()->bool pure{strcmp("a","a")==0}fn main(){}','effect'),
 'pure-parse':('fn f()->f64 pure{atof("1")}fn main(){}','effect'),
 'pure-alloc':('fn f()->*u8 pure{alloc(1)}fn main(){}','effect'),
 'pure-assert':('fn f() pure{assert(true);}fn main(){}','effect'),
 'pure-exit':('fn f() pure{exit(0);}fn main(){}','effect'),
 'bad-malloc':('extern "C"{fn malloc(n:i32)->i32;}fn main(){}','hosted symbol'),
 'bad-printf':('extern "C"{fn printf(p:*u8)->i32;}fn main(){}','hosted symbol'),
 'bad-static':('extern "C"{static strlen:i32;}fn main(){}','hosted symbol'),
 'bad-strtod':('extern "C"{fn strtod(p:*u8,end:i32)->f64;}fn main(){}','hosted symbol'),
 'bad-export':('#[no_mangle]fn malloc(n:i32)->i32{0}fn main(){}','hosted symbol'),
}
for name,call in [('print','print("x")'),('println','println("x")'),('print_i64','print_i64(1)'),('println_i64','println_i64(1)'),('puts','puts("x")'),('alloc','alloc(1)'),('malloc','malloc(1)'),('free','free(0 as *u8)'),('atof','atof("1")'),('strcmp','strcmp("a","b")'),('exit','exit(0)'),('assert','assert(true)')]:invalid['no-std-'+name]=('#![no_std]\nfn main(){'+call+';}','no_std requires')
for name,(text,diagnostic) in invalid.items():
 f=source(name,text);run(name+'-check',[c,f,'--check'],status=1,diagnostic=diagnostic)
 if a.seed:run(name+'-reference',[a.seed.resolve(),'check',f],status=1)
 for fmt in a.formats:
  output=w/(name+'.'+fmt);output.write_bytes(b'user artifact\n')
  run(name+'-'+fmt+'-reject',[c,f,'--emit-llvm' if fmt=='llvm' else '--emit-c','-o',output],status=1,diagnostic=diagnostic);record(name+'-'+fmt+'-preserved',output.read_bytes()==b'user artifact\n')
# Root no_std covers imported source too. Explicit platform declarations remain
# permitted, including a custom freestanding ABI that is never linked to libc here.
source('host_dependency','pub fn output(){println("implicit");}')
f=source('no-std-import','#![no_std]\nuse host_dependency::output;fn main(){output();}')
run('no-std-import-check',[c,f,'--check'],status=1,diagnostic='no_std requires')
f=source('no-std-custom-abi','#![no_std]#![no_main]extern "C"{fn malloc(n:i32)->i32;}fn custom()->i32{malloc(2)}');run('no-std-custom-abi-check',[c,f,'--check'])
print(f'{len(rows)} hosted checks passed')
