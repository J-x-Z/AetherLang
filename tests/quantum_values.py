#!/usr/bin/env python3
"""Aether quantum simulation, byte budgets and lease lifetime checks on both backends."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
from ir_toolchain import emitted_source_compiler

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c', 'llvm'], default=['c', 'llvm'])
parser.add_argument('--budgets-only', action='store_true', help='Run only the pool additions when the existing reference suite already passed')
args = parser.parse_args()
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
root = Path(__file__).resolve().parent
env = {**os.environ, 'TMPDIR': str(work)}
cc = os.environ.get('CC', 'clang')
records = []

def run(name, command, environment=None):
    command = list(map(str, command))
    result = subprocess.run(command, text=True, capture_output=True, timeout=60, env=environment or env)
    records.append(dict(name=name, command=command, exit=result.returncode,
                        stdout=result.stdout, stderr=result.stderr))
    (work / 'results.json').write_text(json.dumps(records, indent=2))
    assert result.returncode == 0, records[-1]
    return result

for name in (['budget'] if args.budgets_only else ['reference', 'budget']):
    source = root / f'regressions/quantum/{name}.aeth'
    run(f'{name}/semantic check', [compiler, 'check', source])
    outputs = []
    for backend in args.backends:
        for level in [0, 2]:
            binary = work / f'{name}-{backend}-{level}'
            run(f'{name}/{backend}/O{level}/build', [compiler, source, '--backend', backend, f'-O{level}', '-o', binary])
            outputs.append(run(f'{name}/{backend}/O{level}/run', [binary]).stdout)
    assert len(set(outputs)) == 1, f'{name}: backend/optimization output disagreement: {outputs}'
    source_c, binary = work / f'{name}.c', work / f'{name}-sanitized'
    run(f'{name}/emit C', [compiler, source, '--emit-c', '-o', source_c])
    run(f'{name}/sanitizer build', [cc, '-std=c11', '-O1', '-g', '-fsanitize=address,undefined', source_c, '-lm', '-o', binary])
    run(f'{name}/sanitizer run', [binary], {**env, 'UBSAN_OPTIONS': 'halt_on_error=1'})

# Reuse the allocator fault shim already used for tensor ownership. This changes
# only emitted allocator symbol names; all pool/state logic remains Aether.
failure_source = work / 'pool-allocation-failure.aeth'
failure_source.write_text('''
use quantum::{QuantumPool,QuantumLease,quantum_pool_new,quantum_pool_state,quantum_pool_close,quantum_lease_free,quantum_probability,Q_ALLOC};
use result::Result;
extern "C" {fn test_fail_after(count:i64);fn test_live()->i64;}
fn main()->i32 {
 let mut budget:i64=0;let mut failures:u64=0;let mut successes:u64=0;
 while budget<4 {
  test_fail_after(budget);
  match quantum_pool_new(128,2,2) {
   Result::Ok(pool)=>{quantum_pool_close(pool);},
   Result::Err(error)=>{if error!=Q_ALLOC{return 1;}}
  }
  if test_live()!=0{return 2;}
  test_fail_after(-1);
  let pool:*QuantumPool=match quantum_pool_new(128,2,2){Result::Ok(p)=>p,Result::Err(_)=>{return 3;}};
  test_fail_after(budget);
  match quantum_pool_state(pool,2) {
   Result::Ok(lease)=>{successes=successes+1;quantum_lease_free(lease);},
   Result::Err(error)=>{if error!=Q_ALLOC{return 4;}failures=failures+1;}
  }
  if pool.used_bytes!=0 || pool.live_states!=0 || pool.refs!=1 || test_live()!=1{return 5;}
  test_fail_after(-1);
  let lease:*QuantumLease=match quantum_pool_state(pool,2){Result::Ok(s)=>s,Result::Err(_)=>{return 6;}};
  if pool.used_bytes!=64 || pool.live_states!=1 || pool.refs!=2 || test_live()!=4{return 7;}
  quantum_pool_close(pool);
  if test_live()!=4{return 8;}
  let probability:f64=match quantum_probability(lease.state,0){Result::Ok(p)=>p,Result::Err(_)=>{return 9;}};
  if probability!=1.0{return 10;}
  quantum_lease_free(lease);
  if test_live()!=0{return 11;}
  budget=budget+1;
 }
 if failures!=3 || successes!=1{return 12;}
 0
}
''')
run('pool allocation/semantic check', [compiler, 'check', failure_source])
for backend in args.backends:
    extension, flag = ('c', '--emit-c') if backend == 'c' else ('ll', '--emit-llvm')
    emitted = work / f'pool-allocation-{backend}.{extension}'
    run(f'pool allocation/{backend}/emit', [compiler, failure_source, flag, '-O0', '-o', emitted])
    source = emitted.read_text()
    for name in ['malloc', 'realloc', 'free']:
        pattern = rf'\b{name}\b' if backend == 'c' else rf'@{name}\b'
        replacement = f'test_{name}' if backend == 'c' else f'@test_{name}'
        source = re.sub(pattern, replacement, source)
    if backend == 'c':
        source = '#include <stdint.h>\nvoid *test_malloc(uint64_t); void *test_realloc(void *, uint64_t); void test_free(void *);\n' + source
    emitted.write_text(source)
    binary = work / f'pool-allocation-{backend}'
    run(f'pool allocation/{backend}/sanitizer build', [*emitted_source_compiler(backend, cc), '-O1', '-g', '-fsanitize=address,undefined', emitted,
        root / 'regressions/math/allocation_shim.c', '-lm', '-o', binary])
    run(f'pool allocation/{backend}/sanitizer run', [binary], {**env, 'UBSAN_OPTIONS': 'halt_on_error=1'})
print(f'{len(records)} quantum checks passed; {work / "results.json"}')
