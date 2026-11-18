#!/usr/bin/env python3
"""Run the native CSV analysis CLI against exact rational sample statistics."""
import argparse
import csv
import hashlib
import json
import math
import os
from fractions import Fraction
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(exist_ok=True, parents=True)
c = args.compiler.resolve()
source = root/'examples/analysis/regress.aeth'
results = []


def run(command, status=0, input_text=None):
    p = subprocess.run(list(map(str,command)), cwd=work, capture_output=True, text=True, input=input_text,
                       env=dict(os.environ, UBSAN_OPTIONS='halt_on_error=1'), timeout=60)
    results.append(dict(command=p.args, exit=p.returncode, stdout=p.stdout, stderr=p.stderr))
    (work/'results.json').write_text(json.dumps(dict(compiler=hashlib.sha256(c.read_bytes()).hexdigest(),
                                                      commands=results), indent=2))
    assert p.returncode == status, results[-1]
    if status: assert p.stderr.strip(), results[-1]
    return p.stdout


def expected(x, y):
    n=len(x)
    mx, my = sum(x)/n, sum(y)/n
    xx = sum((v-mx)**2 for v in x)
    yy = sum((v-my)**2 for v in y)
    xy = sum((a-mx)*(b-my) for a,b in zip(x,y))
    return dict(n=n, skipped=1, mean_x=mx,mean_y=my,variance_x=xx/(n-1),
                variance_y=yy/(n-1),covariance=xy/(n-1),slope=xy/xx,
                intercept=my-xy/xx*mx,r_squared=xy*xy/(xx*yy))


data=work/'观测 samples.csv'
xs,ys=[],[]
with data.open('w',newline='') as f:
    writer=csv.writer(f)
    writer.writerow(['note','x, time','value'])
    for i in range(-50,51):
        x=Fraction(i,4)
        y=5*x+2+Fraction((i*i)%11,8)
        xs.append(x);ys.append(y)
        writer.writerow(['line one\n"quoted", 文',float(x),float(y)])
    writer.writerow(['missing','','4'])
truth=expected(xs,ys)
# Include no final newline, a trailing empty field and a quoted numeric field.
flat=work/'constant.csv'
flat.write_bytes(b'x,y,unused\r\n1,2,z\r\n"2",2,')
shifted=work/'shifted.csv'
shifted.write_text('x,y\n1000000000001,3\n1000000000002,5\n1000000000003,7')
bad_cases=[
    ('unterminated',b'x,y\n"1,7',3), ('foreign byte',b'x,y\n1,2\x00',3),
    ('junk after quote',b'x,y\n"7"junk,4',3), ('lone cr',b'x,y\r1,3',3),
    ('short record',b'x,y\n1\n',3), ('wide record',b'x,y\n1,2,3',3),
    ('duplicate',b'x,y,x\n1,2,3',4), ('missing header',b'x,z\n1,2',4),
    ('partial number',b'x,y\n1,2z',5), ('nonfinite',b'x,y\nNaN,4',5),
    ('too few',b'x,y\n1,2',6), ('no variance',b'x,y\n2,2\n2,4',6)
]


def verify(exe):
    result=json.loads(run([exe,data,'x, time','value']))
    for name,value in truth.items():
        assert math.isclose(result[name],float(value),rel_tol=1e-12,abs_tol=1e-12),(name,result,truth)
    piped=json.loads(run([exe,'-','x, time','value'], input_text=data.read_text()))
    assert piped==result
    result=json.loads(run([exe,flat,'x','y']))
    assert result['n']==2 and result['slope']==0 and result['r_squared'] is None
    result=json.loads(run([exe,shifted,'x','y']))
    assert result['slope']==2 and result['covariance']==2 and result['intercept']==-1999999999999
    for name,raw,status in bad_cases:
        bad=work/(name+'.csv');bad.write_bytes(raw)
        assert not run([exe,bad,'x','y'],status)
    run([exe,work/'no such file','x','y'],3)
    run([exe],2)


for backend in args.backends:
    for opt in (0,2):
        exe=work/f'regress-{backend}-{opt}'
        run([c,'build',source,'--sysroot',root,'--backend',backend,f'-O{opt}','-o',exe])
        verify(exe)
generated=work/'regress-san.c'
run([c,'build',source,'--sysroot',root,'--emit-c','-o',generated])
exe=work/'regress-san'
run([os.environ.get('CC','clang'),'-O1','-fsanitize=address,undefined',generated,'-lm','-o',exe])
verify(exe)
print(f'{len(results)} terminal statistics commands, {len(xs)} complete pairs, exact-rational oracle passed')
