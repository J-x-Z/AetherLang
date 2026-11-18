#!/usr/bin/env python3
"""Compare a self-hosted compiler against an independent native C ABI caller and callee."""
import argparse
import hashlib
import json
import os
import platform
import subprocess
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
args = p.parse_args()
tool = args.compiler.resolve()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
fixtures = Path(__file__).resolve().parent/'regressions/bootstrap/native_abi'
records = []
cc = os.environ.get('CC', 'clang')
def run(label, cmd):
    result = subprocess.run(list(map(str,cmd)), capture_output=True, text=True, cwd=work,
                            env={**os.environ,'ASAN_OPTIONS':'detect_stack_use_after_return=1',
                                 'UBSAN_OPTIONS':'halt_on_error=1'}, timeout=60)
    records.append(dict(label=label, command=result.args, status=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work/'results.json').write_text(json.dumps(dict(host=platform.platform(),
        compiler_sha256=hashlib.sha256(tool.read_bytes()).hexdigest(),checks=records),indent=2))
    assert result.returncode == 0, (label,result)

source=fixtures/'values.aeth'
oracle=fixtures/'oracle.c'
obj=work/'oracle.o'
run('independent C object', [cc,'-std=c11','-O2','-c',oracle,'-o',obj])
for backend in ('c','llvm'):
    for level in (0,2,3):
        binary=work/f'{backend}-{level}'
        run('build '+binary.name,[tool,'build',source,'--backend',backend,f'-O{level}','--link-object',obj,'-o',binary])
        run('bidirectional oracle '+binary.name,[binary])
    generated=work/('program.c' if backend=='c' else 'program.ll')
    run('emit '+backend,[tool,source,'--emit-'+backend,'-o',generated])
    if backend == 'llvm':
        ir=generated.read_text()
        for name in ('pair','addup','native_pair','native_big','sret_spill'):
            assert any(line.startswith(('declare ','define ')) and '@'+name+'(' in line for line in ir.splitlines())
        if platform.machine() == 'x86_64':
            for name in ('spill','sret_spill','sse_spill'):
                line=next(line for line in ir.splitlines() if line.startswith('declare ') and '@'+name+'(' in line)
                assert 'byval(' in line, line
        assert 'sret(' in next(line for line in ir.splitlines() if line.startswith('define ') and '@native_big(' in line)
    binary=work/f'sanitized-{backend}'
    run('sanitizer link '+backend,[cc,'-O1','-g','-fsanitize=address,undefined','-fno-omit-frame-pointer',generated,oracle,'-o',binary])
    run('sanitizer execute '+backend,[binary])
print(f'{len(records)} native aggregate ABI commands passed on {platform.system()} {platform.machine()}')
