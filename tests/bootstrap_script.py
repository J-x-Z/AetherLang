#!/usr/bin/env python3
"""Aether-built Script frontend: frozen self compilation and ordinary native semantics."""
import argparse,hashlib,json,os,shutil,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--seed',type=Path,required=True,help='Existing Aether-built Core compiler, not Rust aethc')
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--core-source',type=Path,help='Optional frozen Core source directory for coordinated development')
p.add_argument('--backends',nargs='+',choices=['c','llvm'],default=['c','llvm'])
p.add_argument('--no-sanitize',action='store_true')
a=p.parse_args();repo=Path(__file__).resolve().parent.parent;work=a.work_dir.resolve();work.mkdir(parents=True,exist_ok=True)
snapshot=work/'source';assert not snapshot.exists(),'use a fresh work dir to preserve the previous source evidence';shutil.copytree(a.core_source or repo/'src_aether/core',snapshot)
fixtures=work/'fixtures'
for directory in ['tests/regressions/script','tests/regressions/bootstrap/script']:
 shutil.copytree(repo/directory,fixtures/directory)
for name in ['examples/hello.ath','tests/regressions/literals/script.ath']:
 dest=fixtures/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(repo/name,dest)
(work/'sources.json').write_text(json.dumps({str(f.relative_to(snapshot)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(snapshot.rglob('*.aeth'))},indent=2))
(work/'fixtures.json').write_text(json.dumps({str(f.relative_to(fixtures)):hashlib.sha256(f.read_bytes()).hexdigest() for f in sorted(fixtures.rglob('*')) if f.is_file()},indent=2))
records=[];env={**os.environ,'TMPDIR':str(work),'UBSAN_OPTIONS':'halt_on_error=1','ASAN_OPTIONS':'detect_stack_use_after_return=1'}
def run(label,args,expected=0,output=None,contains=None):
 r=subprocess.run(list(map(str,args)),capture_output=True,env=env,timeout=180)
 r.stdout=r.stdout.decode('utf-8',errors='replace');r.stderr=r.stderr.decode('utf-8',errors='replace')
 records.append(dict(label=label,args=list(map(str,args)),status=r.returncode,stdout=r.stdout,stderr=r.stderr));(work/'results.json').write_text(json.dumps(records,indent=2))
 assert r.returncode==expected,(label,r.returncode,r.stdout,r.stderr)
 if output is not None:assert r.stdout==output,(label,r.stdout,output)
 if contains:assert contains in r.stdout+r.stderr,(label,r.stdout,r.stderr)
 return r
main=snapshot/'main.aeth';s1=work/'stage1';s2=work/'stage2';s3=work/'stage3'
run('Aether seed builds Script frontend',[a.seed.resolve(),'build',main,'--backend','c','-O2','-o',s1])
run('new frontend self compiles LLVM stage2',[s1,'build',main,'--backend','llvm','-O2','-o',s2])
run('LLVM stage2 self compiles stage3',[s2,'build',main,'--backend','llvm','-O2','-o',s3])
for stage in [s2,s3]:run(stage.name+'/self emit',[stage,main,'--emit-llvm','-o',work/(stage.name+'.ll')])
assert (work/'stage2.ll').read_bytes()==(work/'stage3.ll').read_bytes(),'self-hosted Script compiler LLVM fixed point mismatch'
cases={fixtures/f'tests/regressions/script/{name}.ath':expected for name,expected in [('compute','338350\n25\n'),('branches','quoted: "Aether"\\path\n'),('float_math','3.25\n'),('compound',''),('loop_hygiene',''),('param_hygiene',''),('nested_hygiene','')]}
cases[fixtures/'examples/hello.ath']='Hello from Script!\n'
cases[fixtures/'tests/regressions/literals/script.ath']='common:\n\r\t\\\"\'\n\x1b\n\x1b[31mred\x1b[0m\nAAAFA1'+'\nAé中😀'+chr(0x10ffff)+'\nraw:é中😀\nnul:\n'
cases[fixtures/'tests/regressions/bootstrap/script/repeat.ath']='once\nonce\n'
cases.update({fixtures/'tests/regressions/bootstrap/script/modules.ath':'Script module: "UTF-8" é中😀\n',fixtures/'tests/regressions/bootstrap/script/control.ath':''})
for source,expected in cases.items():
 run(source.stem+'/check',[s3,'check',source])
 for b in a.backends:
  for o in [0,2]:
   exe=work/f'{source.stem}-{b}-{o}';run(f'{source.stem}/{b}/{o}/build',[s3,'build',source,'--backend',b,f'-O{o}','-o',exe]);run(f'{source.stem}/{b}/{o}/run',[exe],output=expected)
if not a.no_sanitize:
 source=work/'compiler.c';san=work/'stage-sanitized'
 run('san/compiler emit',[s3,main,'--emit-c','-o',source]);run('san/compiler build',['clang','-x','c','-std=c11','-O1','-g','-fsanitize=address,undefined','-fno-omit-frame-pointer',source,'-o',san])
 for source in cases:run(source.stem+'/san-check',[san,'check',source])
 source=work/'control.c';exe=work/'control-san';run('san/app emit',[s3,fixtures/'tests/regressions/bootstrap/script/control.ath','--emit-c','-o',source]);run('san/app build',['clang','-std=c11','-O1','-fsanitize=address,undefined',source,'-o',exe]);run('san/app run',[exe],output='')
negative={
 'one_branch':('def main():\n    if True:\n        value = 1\n    return value\n','before assignment'),
 'loop_zero':((fixtures/'tests/regressions/script/uninitialized.ath').read_text(),'before assignment'),
 'bad_indent':((fixtures/'tests/regressions/script/bad_indent.ath').read_text(),'indentation'),
 'tabs':('def main():\n\treturn 0\n','spaces'),
 'unassigned_range':('def main():\n    for i in range(0):\n        pass\n    return i\n','before assignment'),
 'type_change':('def main():\n    x = 1\n    x = 2.0\n    return 0\n','first inferred type'),
 'unknown_escape':('def main():\n    print("bad\\q")\n    return 0\n','unknown string escape'),
 'missing_annotation':('def helper(x) -> int:\n    return x\ndef main():\n    return 0\n','type annotation'),
 'nested_function':('def main():\n    def nested() -> int:\n        return 0\n    return 0\n','keyword'),
 'range_shadow':('def range(x: int) -> int:\n    return x\ndef main():\n    for i in range(3):\n        pass\n    return 0\n','shadowed range'),
 'private_core':('from core_bridge import hidden\ndef main():\n    return hidden()\n','unknown'),
 'call_argument':('def f(x: int) -> int:\n    return x\ndef main():\n    return f(1.5)\n','type'),
}
for name,(text,message) in negative.items():
 source=work/(name+'.ath');source.write_text(text);run(name+'/reject',[s3,'check',source,'-I',fixtures/'tests/regressions/bootstrap/script'],1,contains=message)
 if not a.no_sanitize:run(name+'/san-reject',[san,'check',source,'-I',fixtures/'tests/regressions/bootstrap/script'],1,contains=message)
print(f'{len(records)} Script bootstrap checks passed; LLVM self-compile fixed point; {work/"results.json"}')
