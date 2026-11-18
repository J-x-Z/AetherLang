#!/usr/bin/env python3
"""Vec growth/rollback and size_t ABI on a genuinely executing wasm32 target."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--compiler', type=Path, required=True)
p.add_argument('--work-dir', type=Path, required=True)
p.add_argument('--backends', nargs='+', choices=['llvm', 'c'], default=['llvm', 'c'])
p.add_argument('--require-runtime', action='store_true', help='Fail rather than skip when wasm runtime tools are missing')
a = p.parse_args()
root = Path(__file__).resolve().parents[1]
work = a.work_dir.resolve(); work.mkdir(parents=True, exist_ok=True)
compiler = a.compiler.resolve(); source = root / 'tests/regressions/vec32/grow.aeth'
env = {**os.environ, 'TMPDIR': str(work)}
records = []
meta = {'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
        'vec_sha256': hashlib.sha256((root / 'stdlib/vec.aeth').read_bytes()).hexdigest()}

def save():
    (work / 'results.json').write_text(json.dumps({**meta, 'records': records}, indent=2) + '\n')

def run(label, command, expected=0):
    command = list(map(str, command))
    try:
        result = subprocess.run(command, cwd=work, env=env, text=True, capture_output=True, timeout=45)
    except subprocess.TimeoutExpired:
        result = subprocess.CompletedProcess(command, 124, '', 'timeout')
    records.append(dict(label=label, command=command, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr, expected_exit=expected, passed=result.returncode == expected))
    save()
    print(('PASS ' if result.returncode == expected else 'FAIL ') + label, flush=True)
    if result.returncode != expected:
        raise SystemExit(result.stderr or result.stdout or f'exit={result.returncode}')
    return result

# Hosted check catches declarations that disagree with target libc size_t. The
# no_std executable instead gets only its explicit allocator through wasm imports.
hosted = work / 'hosted.aeth'; hosted.write_text(source.read_text().replace('#![no_std]\n', ''))
run('hosted/wasm32/check', [compiler, 'check', hosted, '-I', root / 'stdlib', '--target', 'wasm32-unknown-unknown'])
run('explicit/wasm32/check', [compiler, 'check', source, '-I', root / 'stdlib', '--target', 'wasm32-unknown-unknown'])
wrong = work / 'wrong-size-t.aeth'
wrong.write_text('#![no_main]\nextern "C" { fn malloc(size:u64)->*void; }\n')
rejected = run('wrong-size-t/wasm32/reject', [compiler, 'check', wrong, '--target', 'wasm32-unknown-unknown'], expected=1)
if 'E2202' not in rejected.stderr or 'target C ABI' not in rejected.stderr:
    raise SystemExit('Expected the target C ABI mismatch diagnostic')
cc = env.get('WASM_CC') or shutil.which('clang')
sdk_clang = Path(env.get('AETHER_LLVM_PREFIX') or env.get('LLVM_SYS_211_PREFIX') or '/opt/homebrew/opt/llvm@21')/'bin/clang'
if not env.get('WASM_CC') and sdk_clang.is_file():
    cc = str(sdk_clang)
linker = shutil.which('wasm-ld'); node = shutil.which('node')
if not all([cc, linker, node]):
    meta['wasm_execution'] = 'not run: requires clang, wasm-ld and node'
    save()
    if a.require_runtime: raise SystemExit(meta['wasm_execution'])
    print(meta['wasm_execution'])
    raise SystemExit(0)
for backend in a.backends:
    for level in [0, 2]:
        label = f'{backend}/O{level}'; obj = work / f'vec-{backend}-O{level}.o'; wasm = obj.with_suffix('.wasm')
        run(label + '/compile', [compiler, 'build', source, '-I', root / 'stdlib', '--target', 'wasm32-unknown-unknown',
                                 '--backend', backend, '--cc', cc, '--emit-obj', f'-O{level}', '-o', obj])
        run(label + '/link', [linker, '--no-entry', '--allow-undefined', '--export=vec32', '--export-memory',
                              '--initial-memory=262144', obj, '-o', wasm])
        run(label + '/run', [node, source.parent / 'run.js', wasm])
meta['wasm_execution'] = 'passed: actual wasm32 link and execution, C/LLVM selections as recorded'
save()
print(f'{len(records)}/{len(records)} checks passed; {work / "results.json"}')
