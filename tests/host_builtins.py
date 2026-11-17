#!/usr/bin/env python3
"""Hosted builtin parity, explicit-symbol identity, and real wasm32 C ABI calls."""
import argparse, hashlib, json, os, pathlib, re, shutil, subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--compiler', required=True)
parser.add_argument('--work-dir', required=True)
parser.add_argument('--backends', nargs='+', default=['c', 'llvm'])
args = parser.parse_args()
compiler = str(pathlib.Path(args.compiler).resolve())
work = pathlib.Path(args.work_dir).resolve(); work.mkdir(parents=True, exist_ok=True)
root = pathlib.Path(__file__).resolve().parent.parent
sources = root / 'tests/regressions/host_builtins'
env = os.environ.copy(); (work/'tmp').mkdir(exist_ok=True); env['TMPDIR'] = str(work/'tmp')
results = {'compiler_sha256': hashlib.sha256(pathlib.Path(compiler).read_bytes()).hexdigest(), 'commands': []}

def run(command, expected=0):
    command = list(map(str, command))
    result = subprocess.run(command, cwd=root, env=env, text=True, capture_output=True, timeout=40)
    results['commands'].append({'command': command, 'status': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr})
    (work/'results.json').write_text(json.dumps(results, indent=2))
    valid = result.returncode in (-4, -5) if expected == 'trap' else result.returncode == expected
    assert valid, results['commands'][-1]
    return result

exit_source = work/'exit.aeth'; exit_source.write_text('fn main()->i32{println("before");exit(7);println("after");return 0;}')
assert_source = work/'assert_false.aeth'; assert_source.write_text('fn main()->i32{assert(false);return 0;}')
for backend in args.backends:
    for level in [0, 2]:
        for name, source, status, stdout in [
            ('hosted', sources/'hosted.aeth', 0, 'hello world\n-92233720368547758089223372036854775807\n'),
            ('explicit', sources/'explicit_names.aeth', 0, ''),
            ('exit', exit_source, 7, 'before\n'),
            ('assert_false', assert_source, 'trap', ''),
        ]:
            output = work/f'{name}-{backend}-O{level}'
            run([compiler, source, '--backend', backend, f'-O{level}', '-o', output])
            execution = run([output], status)
            assert execution.stdout == stdout and not execution.stderr, execution

# Clang 23 diagnoses uint8_t** passed to libc char** as an error.
for backend in args.backends:
    for level in (0, 2, 3):
        source = sources/'parse_end.aeth'
        binary = work/f'parse-end-{backend}-{level}'
        if backend == 'c':
            emitted = binary.with_suffix('.c')
            run([compiler, source, '--emit-c', '-o', emitted])
            run(['clang', '-std=c11', '-Werror=incompatible-pointer-types', f'-O{level}', emitted, '-o', binary])
        else:
            run([compiler, source, '--backend', backend, f'-O{level}', '-o', binary])
        run([binary])
for target,bits in [('x86_64-pc-windows-msvc',32),('x86_64-unknown-linux-gnu',64)]:
    for width in (32,64):
        declaration = work/f'long-{width}.aeth'
        declaration.write_text(f'extern "C"{{fn strtoul(p:*u8,end:**u8,base:i32)->u{width};}}fn main(){{}}')
        result = run([compiler,'check',declaration,'--target',target], 0 if bits==width else 1)
        if bits!=width: assert 'E2202' in result.stderr

# Explicit declarations remain available in freestanding modules; implicit host
# dependencies fail in the same semantic session before either backend.
explicit = work/'no_std_explicit.aeth'; explicit.write_text('#![no_std]\n'+(sources/'explicit_names.aeth').read_text())
for backend in args.backends:
    output=work/f'no-std-explicit-{backend}'
    run([compiler, explicit, '--backend', backend, '-O2', '-o', output]); run([output])
for index, body in enumerate(['print("x");', 'println_i64(1);', 'alloc(1);', 'malloc(1);', 'exit(1);']):
    source=work/f'no-std-{index}.aeth';source.write_text('#![no_std]\nfn main(){'+body+'}')
    for mode in ['check','build']:
        rejected=run([compiler,mode,source,'--backend','c','-o',work/'rejected'],1)
        assert 'E2402' in rejected.stderr, rejected.stderr
narrow=work/'oversize.aeth';narrow.write_text('#![no_main]\nfn bad(size:u64)->*u8{malloc(size)}')
rejected=run([compiler,'check',narrow,'--target','wasm32-unknown-unknown'],1)
assert 'Type mismatch' in rejected.stderr, rejected.stderr
for index, expression in enumerate(['strcmp("same","same")==0', 'atof("1.25")==1.25']):
    source=work/f'pointer-read-{index}.aeth'
    source.write_text('fn reads()->bool pure{'+expression+'}fn main(){}')
    rejected=run([compiler,'check',source],1)
    assert 'effect' in rejected.stderr.lower(), rejected.stderr
for index, declaration in enumerate([
    '#[no_mangle] fn malloc(size:i32)->i32{0}',
    'extern "C"{fn malloc(size:i32)->i32;}',
    'extern "C"{fn printf(format:*u8)->i32;}',
    'extern "C"{static strlen:i32;}',
    'extern "C"{fn strtod(s:*u8,end:i32)->f64;}',
]):
    source=work/f'fixed-libc-{index}.aeth'; source.write_text(declaration+'fn main(){}')
    for mode in ['check','build']:
        rejected=run([compiler,mode,source,'--backend','c','-o',work/'invalid-libc'],1)
        assert 'E2202' in rejected.stderr and 'hosted symbol' in rejected.stderr, rejected.stderr

# Inspect full declarations before optimization can remove unused imports.
for backend in args.backends:
    text=work/f'abi32-{backend}.txt'
    run([compiler,sources/'abi32.aeth','--backend',backend,'--target','wasm32-unknown-unknown', '--emit-llvm' if backend=='llvm' else '--emit-c','-O0','-o',text])
    emitted=text.read_text()
    if backend=='llvm':
        for name in ['malloc','realloc','memcpy']:
            declaration=next(line for line in emitted.splitlines() if line.startswith('declare') and '@'+name+'(' in line)
            assert 'i64' not in declaration and 'i32' in declaration, declaration
        assert re.search(r'declare i32 @strlen\(ptr',emitted),emitted
        assert '@printf(' not in emitted and '@exit(' not in emitted, emitted

if all(shutil.which(tool) for tool in ['node','wasm-ld','clang']):
    resource=run(['clang','-print-resource-dir']).stdout.strip()
    headers=work/'headers';headers.mkdir(exist_ok=True)
    for name, content in {
        'stdio.h':'int printf(const char*,...);int puts(const char*);',
        'stdlib.h':'#include <stddef.h>\nvoid*malloc(size_t);void*realloc(void*,size_t);void free(void*);void exit(int);double atof(const char*);',
        'string.h':'#include <stddef.h>\nvoid*memcpy(void*,const void*,size_t);size_t strlen(const char*);int strcmp(const char*,const char*);',
        'math.h':'',
    }.items(): (headers/name).write_text(content)
    harness=work/'run-wasm.js'
    harness.write_text('''const fs=require('fs');
let instance;let next=131072;const calls=[];
const num=(n)=>{if(typeof n!=='number')throw Error('size_t is not wasm i32');return n;};
const imports={env:{malloc(n){num(n);calls.push(['malloc',n]);const p=next;next+=64;return p;},
realloc(p,n){num(p);num(n);calls.push(['realloc',n]);return p;},
memcpy(d,s,n){num(n);calls.push(['memcpy',n]);new Uint8Array(instance.exports.memory.buffer).copyWithin(d,s,s+n);return d;},
strlen(p){calls.push(['strlen']);const bytes=new Uint8Array(instance.exports.memory.buffer);let n=0;while(bytes[p+n])n++;return n;},
free(p){num(p);calls.push(['free']);}}};
instance=new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[2])),imports);
if(process.argv[3]==='explicit'){
 if(instance.exports.abi32()!==3)throw Error('bad size_t result');
 if(!calls.some(c=>c[0]==='malloc'&&c[1]===16)||!calls.some(c=>c[0]==='realloc'&&c[1]===32))throw Error(JSON.stringify(calls));
}else{
 const a=instance.exports.alias32(16),b=instance.exports.direct32(16);
 instance.exports.release32(a);instance.exports.release32(b);
 if(calls.filter(c=>c[0]==='malloc'&&c[1]===16).length!==2||calls.filter(c=>c[0]==='free').length!==2)throw Error(JSON.stringify(calls));
}
''')
    for backend in args.backends:
        custom=work/f'custom-{backend}.aeth';custom.write_text('#![no_std]\n#![no_main]\n#[no_mangle] fn malloc(size:i32)->i32{size+1}')
        run([compiler,custom,'--backend',backend,'--emit-obj','-O2','-o',work/f'custom-native-{backend}.o'])
        obj=custom.with_suffix('.o');wasm=custom.with_suffix('.wasm')
        run([compiler,custom,'--backend',backend,'--target','wasm32-unknown-unknown','--emit-obj','--cc','clang','-O2','-o',obj])
        run(['wasm-ld','--no-entry','--export=malloc',obj,'-o',wasm])
        run(['node','-e',"const fs=require('fs');const m=new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(process.argv[1])));if(m.exports.malloc(41)!==42)process.exit(1);",wasm])
        for level in [0,2]:
            for kind,source,exports in [('explicit',sources/'abi32.aeth',['abi32']),('implicit',sources/'implicit32.aeth',['alias32','direct32','release32'])]:
                obj=work/f'wasm-{kind}-{backend}-O{level}.o';wasm=obj.with_suffix('.wasm')
                if backend=='c' and kind=='implicit':
                    generated=obj.with_suffix('.c')
                    run([compiler,source,'--backend','c','--target','wasm32-unknown-unknown','--emit-c',f'-O{level}','-o',generated])
                    run(['clang','--target=wasm32-unknown-unknown','-ffreestanding','-nostdinc','-isystem',pathlib.Path(resource)/'include','-I',headers,f'-O{level}','-c',generated,'-o',obj])
                else:
                    run([compiler,source,'--backend',backend,'--target','wasm32-unknown-unknown','--emit-obj','--cc','clang',f'-O{level}','-o',obj])
                run(['wasm-ld','--no-entry','--export-memory','--initial-memory=262144','--allow-undefined',*[f'--export={name}' for name in exports],obj,'-o',wasm])
                run(['node',harness,wasm,kind])
else:
    results['wasm_execution']='skipped: requires node, wasm-ld and clang'
    (work/'results.json').write_text(json.dumps(results,indent=2))
print(f"{len(results['commands'])} host builtin commands matched their expected outcomes")
