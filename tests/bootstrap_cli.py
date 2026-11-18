#!/usr/bin/env python3
"""Ordinary self-hosted CLI keeps diagnostics off data stdout."""
import argparse, hashlib, json, os, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--compiler',type=Path,required=True);p.add_argument('--work-dir',type=Path,required=True);a=p.parse_args()
c=a.compiler.resolve();w=a.work_dir.resolve();w.mkdir(parents=True,exist_ok=True);env={**os.environ,'TMPDIR':str(w)};rows=[]
(w/'compiler.json').write_text(json.dumps(dict(path=str(c),sha256=hashlib.sha256(c.read_bytes()).hexdigest()),indent=2))
def check(name,ok,**facts):
 rows.append(dict(name=name,passed=bool(ok),**facts));(w/'results.json').write_text(json.dumps(rows,indent=2));print(('PASS ' if ok else 'FAIL ')+name,flush=True)
 if not ok:raise SystemExit(name)
def run(name,args,status=0,error=None,json_error=False):
 cmd=list(map(str,[c,*args]));q=subprocess.run(cmd,env=env,cwd=w,capture_output=True,text=True,timeout=30)
 check(name,q.returncode==status and (status==0 or q.stdout=='') and (error is None or error in q.stderr),command=cmd,status=q.returncode,stdout=q.stdout,stderr=q.stderr)
 if json_error:
  data=json.loads(q.stderr);check(name+'-location',data['location']['file']==str(bad) and data['location']['line']==2 and data['location']['column']>0)
 elif status and error=='type mismatch':check(name+'-location',str(bad)+':2:' in q.stderr)
 return q
bad=w/'invalid Ω.aeth';bad.write_text('// invalid return\nfn main()->i32{true}\n');old=w/'previous';old.write_bytes(b'keep user output\n')
run('check',["check",bad],1,'type mismatch')
run('symbols-failure',['symbols',bad],1,'type mismatch')
for mode in ['--emit-c','--emit-llvm']:
 run(mode,[bad,mode,'-o',old],1,'type mismatch');check(mode+'-preserve',old.read_bytes()==b'keep user output\n')
for backend in ['c','llvm']:
 run(backend+'-build',['build',bad,'--backend',backend,'-o',old],1,'type mismatch');check(backend+'-preserve',old.read_bytes()==b'keep user output\n')
run('json',['check',bad,'--error-format','json'],1,'type mismatch',True)
run('symbols-json',['symbols',bad,'--error-format','json'],1,'type mismatch',True)
run('argument-missing-directory',[bad,'-I'],1,'-I requires a directory')
run('argument-missing-source',['build'],1,'expected input')
for option in ['--help','--version']:
 q=run(option,[option]);check(option+'-data-stdout',bool(q.stdout) and q.stderr=='')
good=w/'valid.aeth';good.write_text('fn main()->i32{0}');q=run('symbols-success',['symbols',good]);data=json.loads(q.stdout);check('symbols-data-only',q.stderr=='' and [x['name'] for x in data['symbols']]==['main'])
print(f'{len(rows)} CLI checks passed')
