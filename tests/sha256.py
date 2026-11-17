#!/usr/bin/env python3
"""Compare native Aether streaming SHA-256 with known answers and hashlib."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler',type=Path,required=True)
p.add_argument('--work-dir',type=Path,required=True)
p.add_argument('--backends',nargs='+',default=['c','llvm'])
args=p.parse_args()
root=Path(__file__).resolve().parents[1]
out=args.work_dir.resolve()
out.mkdir(exist_ok=True,parents=True)
inputs=[b'',b'abc',*(bytes(i%256 for i in range(n)) for n in (55,56,63,64,65,255,256,257,4095,4096,4097)),b'a'*1_000_000]
answers=[hashlib.sha256(data).hexdigest() for data in inputs]
assert answers[1]=='ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
assert answers[-1]=='cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0'
rows=[]
for backend in args.backends:
    for level in (0,2):
        exe=out/f'stream-{backend}-{level}'
        subprocess.run([args.compiler.resolve(),root/'tests/regressions/sha256/stream.aeth','--sysroot',root,'--backend',backend,f'-O{level}','-o',exe],check=True)
        for data,expected in zip(inputs,answers):
            run=subprocess.run([exe],input=data,capture_output=True,check=True,timeout=20)
            assert run.stdout.decode().strip()==expected and not run.stderr
            rows.append(dict(backend=backend,optimization=level,length=len(data),sha256=expected))
(out/'results.json').write_text(json.dumps(rows,indent=2)+'\n')
print(f'{len(rows)} streaming hash results matched')
