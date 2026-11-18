#!/usr/bin/env python3
"""Verify freestanding objects and optionally boot the x86-64 interrupt fixtures."""
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
parser.add_argument('--qemu', action='store_true', default=os.environ.get('AETHER_TEST_QEMU') == '1',
                    help='boot and exercise both interrupt forms under qemu-system-x86_64')
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
    prefix=os.getenv('AETHER_LLVM_PREFIX') or os.getenv('LLVM_SYS_211_PREFIX')
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
    if args.qemu:
        qemu, linker, objcopy = tool('qemu-system-x86_64'), tool('ld.lld'), tool('llvm-objcopy')
        boot = work / 'interrupt-boot.o'
        run([clang, '--target=x86_64-unknown-none', '-c', fixtures / 'interrupt_boot.S', '-o', boot])
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
            first_arg = cc_log.stat().st_size
            run(command)
            if target == 'x86_64-unknown-none':
                # Ordinary functions in a separate object obey the same stack
                # contract as code compiled next to an interrupt entry.
                if backend == 'c':
                    with cc_log.open('rb') as arguments:
                        arguments.seek(first_arg)
                        assert b'-mno-red-zone' in arguments.read().splitlines()
                else:
                    generated = obj.with_suffix('.ll')
                    run([compiler,fixtures/'compute.aeth','--backend','llvm','--target',target,
                         '--emit-llvm','-o',generated])
                    ir = generated.read_text()
                    definitions = re.findall(r'^define [^\n]+',ir,re.M)
                    protected = set(re.findall(r'^attributes #(\d+) = \{[^}]*\bnoredzone\b',ir,re.M))
                    assert definitions,ir
                    for definition in definitions:
                        group = re.search(r'#(\d+)\s*\{$',definition)
                        assert group and group[1] in protected,definition
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
            if args.qemu:
                elf, image = obj.with_suffix('.elf'), obj.with_suffix('.bin')
                run([linker, '-T', root / 'examples/kernel.ld', boot, obj, '-o', elf])
                run([objcopy, '-O', 'binary', elf, image])
                # Guest reports success only after the software interrupt and real
                # #GP return to the expected instruction with the original stack.
                run([qemu, '-machine', 'pc,accel=tcg', '-m', '32M', '-nodefaults',
                     '-display', 'none', '-no-reboot', '-device',
                     'isa-debug-exit,iobase=0xf4,iosize=4', '-kernel', image], status=33)
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
    (work/'results.json').write_text(json.dumps(dict(compiler=str(compiler),compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),records=records,qemu=args.qemu, scope='freestanding host execution and object checks; optional x86_64 TCG interrupt execution'),indent=2))
print('Freestanding compute, explicit externs, x86_64 interrupt ABI, x86_64/aarch64 naked objects and diagnostics passed')
if args.qemu:
    print('QEMU x86_64 interrupt and hardware-error-code returns passed at O0 and O2')
