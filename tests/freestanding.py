#!/usr/bin/env python3
"""Hosted execution of freestanding objects; machine entries get object-only evidence."""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import re
import shlex
import shutil
import subprocess

parser=argparse.ArgumentParser()
parser.add_argument('--compiler',type=Path,required=True)
parser.add_argument('--work-dir',type=Path,required=True)
parser.add_argument('--backends',nargs='+',default=['c','llvm'])
args=parser.parse_args()
compiler=args.compiler.resolve();work=args.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
root=Path(__file__).resolve().parents[1];fixtures=root/'tests/regressions/freestanding'
env=os.environ.copy();env['TMPDIR']=str(work)
records=[]
def run(command,status=0,environment=None):
    command=[str(value) for value in command]
    result=subprocess.run(command,capture_output=True,text=True,env=environment or env,timeout=30)
    records.append(dict(command=command,status=result.returncode,stdout=result.stdout,stderr=result.stderr))
    assert result.returncode==status,records[-1]
    return result

def interrupt_stack_delta(disassembly, name):
    # At IRET the frame pointer must be restored; an exception additionally
    # discards exactly the machine-pushed 8-byte error code. Compilers may add
    # padding for alignment, so checking one literal add instruction is wrong.
    body=re.search(r'<[^>]*'+re.escape(name)+r'>:\n(.*?)(?=\n[0-9a-f]+ <|\Z)',disassembly,re.S).group(1)
    delta=0;frame=None
    for line in body.splitlines():
        if re.search(r'\bpushq?\b',line):delta-=8
        elif re.search(r'\bpopq?\b',line):delta+=8
        match=re.search(r'\b(add|sub)q?\s+\$(0x[0-9a-f]+|[0-9]+),\s*%rsp',line)
        if match:delta+=(1 if match.group(1)=='add' else -1)*int(match.group(2),0)
        if re.search(r'\bmovq?\s+%rsp,\s*%rbp',line):frame=delta
        if re.search(r'\bmovq?\s+%rbp,\s*%rsp',line):delta=frame
        if re.search(r'\bleaveq?\b',line):delta=frame+8
        if re.search(r'\biretq?\b',line):return delta
    raise AssertionError('missing interrupt return')

def tool(name):
    prefix=os.environ.get('LLVM_SYS_211_PREFIX')
    candidates=([Path(prefix)/'bin'/name] if prefix else [])+[Path('/opt/homebrew/opt/llvm@21/bin')/name]
    for candidate in candidates:
        if candidate.is_file():return str(candidate)
    found=shutil.which(name)
    assert found,f'{name} is required for object-code verification'
    return found
clang=tool('clang');objdump=tool('llvm-objdump');nm=tool('llvm-nm')
host_clang='/usr/bin/clang' if platform.system()=='Darwin' else clang
resource=Path(run([clang,'-print-resource-dir']).stdout.strip())/'include'
cc=work/'freestanding-cc.sh';cc_log=work/'cc-arguments.txt';cc_log.write_text('')
cc.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> '+shlex.quote(str(cc_log))+'\nexec '+shlex.quote(clang)+' -nostdinc -isystem '+shlex.quote(str(resource))+' "$@"\n');cc.chmod(0o755)
harness=work/'harness.c'
harness.write_text('''#include <stdint.h>
struct Pair {int64_t a,b;};
extern uint64_t sum(uint64_t);
extern int64_t copied_sum(struct Pair*);
extern int external_abs(void);
extern int64_t convert(double);
extern int64_t nan_result(void);
int64_t custom_add(int64_t a,int64_t b){return a+b;}
int main(void){struct Pair p={20,1};return sum(10)!=55 || copied_sum(&p)!=22 || p.a!=20 || external_abs()!=7 || convert(3.9)!=3 || convert(1e100)!=INT64_MAX || nan_result()!=0;}
''')
try:
    # Compile generated C with only the compiler's freestanding headers available.
    c_source=work/'compute.c'
    run([compiler,fixtures/'compute.aeth','--emit-c','-o',c_source])
    text=c_source.read_text()
    assert set(re.findall(r'#include <([^>]+)>',text))=={'stdint.h','stddef.h','stdbool.h'},text[:1000]
    assert '__builtin_memcpy' in text and '__builtin_isnan' in text
    assert re.search(r'extern .*\babs\(',text) and re.search(r'extern .*\bcustom_add\(',text)
    for backend in args.backends:
        for level in [0,2]:
            obj=work/f'compute-{backend}-{level}.o'
            command=[compiler,fixtures/'compute.aeth','--backend',backend,'-O',level,'--emit-obj','-o',obj]
            if backend=='c':command+=['--cc',cc]
            run(command)
            executable=work/f'compute-{backend}-{level}'
            run([host_clang,harness,obj,'-o',executable]);run([executable])
        for target in ['x86_64-unknown-none','aarch64-unknown-none']:
            obj=work/f'freestanding-{backend}-{target}.o'
            command=[compiler,fixtures/'compute.aeth','--backend',backend,'--target',target,'--emit-obj','-o',obj]
            if backend=='c':command+=['--cc',cc]
            run(command)
            # Explicit foreign calls remain link dependencies, never disappear.
            undefined=run([nm,'--undefined-only',obj]).stdout
            assert 'custom_add' in undefined and re.search(r'\babs\b',undefined),undefined
        for level in [0,2]:
            obj=work/f'interrupt-{backend}-{level}.o'
            command=[compiler,fixtures/'interrupt.aeth','--backend',backend,'--target','x86_64-unknown-none','-O',level,'--emit-obj','-o',obj]
            if backend=='c':command+=['--cc',cc]
            run(command)
            disassembly=run([objdump,'-d',obj]).stdout
            (work/f'interrupt-{backend}-{level}.asm').write_text(disassembly)
            assert len(re.findall(r'\biretq?\b',disassembly))==2,disassembly
            assert not re.search(r'\bretq?\b|\bcallq?\b|\b(?:xmm|ymm|zmm)\d+\b',disassembly),disassembly
            # The two-parameter exception ABI discards its machine-pushed error code.
            assert interrupt_stack_delta(disassembly,'tick')==0,disassembly
            assert interrupt_stack_delta(disassembly,'exception')==8,disassembly
            assert not run([nm,'--undefined-only',obj]).stdout.strip()
        if backend=='llvm':
            ir=work/'interrupt.ll'
            run([compiler,fixtures/'interrupt.aeth','--backend','llvm','--target','x86_64-unknown-none','--emit-llvm','-o',ir])
            assert 'x86_intrcc' in ir.read_text() and 'byval(' in ir.read_text()
        for target in ['x86_64-unknown-none','aarch64-unknown-none']:
            for level in [0,2]:
                obj=work/f'naked-{backend}-{target}-{level}.o'
                command=[compiler,fixtures/'naked.aeth','--backend',backend,'--target',target,'-O',level,'--emit-obj','-o',obj]
                if backend=='c':command+=['--cc',cc]
                run(command)
                disassembly=run([objdump,'-d',obj]).stdout
                (work/f'naked-{backend}-{target}-{level}.asm').write_text(disassembly)
                instructions=[line for line in disassembly.splitlines() if re.match(r'^\s*[0-9a-f]+:',line)]
                assert len(instructions)==1 and re.search(r'\bretq?\b',instructions[0]),disassembly
                assert not run([nm,'--undefined-only',obj]).stdout.strip()
    if 'c' in args.backends:
        assert '-ffreestanding' in cc_log.read_text() and '-mno-red-zone' in cc_log.read_text()
    errno_source=work/'errno.aeth';errno_source.write_text('#![no_std]#![no_main]fn get()->i32{unsafe{__errno_get()}}')
    errno_c=work/'errno.c';run([compiler,errno_source,'--emit-c','-o',errno_c]);assert '#include <errno.h>' in errno_c.read_text()
    frame='#![no_std]#![no_main]struct Frame{ip:u64,cs:u64,flags:u64,sp:u64,ss:u64}'
    negatives=[
        ('wrong_target','@interrupt fn handler(frame:*Frame){}','aarch64-unknown-none'),
        ('wrong_frame','@interrupt fn handler(frame:*u64){}','x86_64-unknown-none'),
        ('wrong_error','@interrupt fn handler(frame:*Frame,error:i64){}','x86_64-unknown-none'),
        ('wrong_return','@interrupt fn handler(frame:*Frame)->i32{0}','x86_64-unknown-none'),
        ('call_handler','@interrupt fn handler(frame:*Frame){}fn caller(frame:*Frame){handler(frame);}','x86_64-unknown-none'),
        ('helper_call','fn helper(){}@interrupt fn handler(frame:*Frame){helper();}','x86_64-unknown-none'),
        ('floating','@interrupt fn handler(frame:*Frame){let value:f64=1.0;}','x86_64-unknown-none'),
        ('naked_local','@naked fn entry(){let value:i32=1;}','x86_64-unknown-none'),
        ('naked_return','@naked fn entry(){return;}','x86_64-unknown-none'),
        ('naked_operand','@naked fn entry(){unsafe{asm!("nop",in("r") 1);}}','x86_64-unknown-none'),
        ('naked_target','@naked fn entry(){unsafe{asm!("ret");}}','riscv64-unknown-none-elf'),
        ('naked_nul',r'@naked fn entry(){unsafe{asm!("ret\0");}}','x86_64-unknown-none'),
        ('implicit_hosted','fn use_hosted(){puts("not declared");}','x86_64-unknown-none'),
    ]
    for name,body,target in negatives:
        source=work/f'{name}.aeth';source.write_text(frame+body)
        for action in ['check','build']:
            result=run([compiler,action,source,'--target',target,'--error-format','json','--emit-obj','-o',work/f'{name}.o'],status=1)
            diagnostic=json.loads(result.stderr);assert diagnostic['code']=='E2402',diagnostic
finally:
    (work/'results.json').write_text(json.dumps(dict(compiler=str(compiler),compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),records=records,scope='freestanding host execution; interrupt/naked object verification only, no hardware interrupt execution'),indent=2))
print('Freestanding compute, explicit externs, x86_64 interrupt ABI, x86_64/aarch64 naked objects and diagnostics passed')
