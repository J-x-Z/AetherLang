#!/usr/bin/env python3
"""Run the same Aether tensor/operator program on CPU and an explicit real GPU provider."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True)
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--backends', nargs='+', choices=['c','llvm'], default=['c','llvm'])
parser.add_argument('--metal', action='store_true', default=os.environ.get('AETHER_TEST_METAL')=='1', help='build and require the real Metal provider on this Mac')
parser.add_argument('--provider', type=Path, help='explicit prebuilt GPU provider (no fallback)')
parser.add_argument('--device', choices=['metal','vulkan'], default='vulkan')
parser.add_argument('--no-sanitize', action='store_true')
args = parser.parse_args()
if args.metal and args.provider: parser.error('use either --metal or --provider')
if args.metal and sys.platform!='darwin': parser.error('--metal requires macOS and a real Metal device')
repo = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True,exist_ok=True)
compiler = args.compiler.resolve()
source = repo/'tests/regressions/compute/runtime.aeth'
env = {**os.environ,'TMPDIR':str(work)}
cc = os.environ.get('CC','clang')
records = []
def run(label, command, success=True, environment=env):
    command = list(map(str,command))
    started = time.perf_counter()
    result = subprocess.run(command,capture_output=True,text=True,timeout=60,env=environment)
    records.append(dict(label=label,command=command,exit=result.returncode,stdout=result.stdout,stderr=result.stderr,
                        elapsed_seconds=time.perf_counter()-started))
    (work/'results.json').write_text(json.dumps(records,indent=2))
    assert (result.returncode==0)==success, f'{label}: {result.returncode}\n{result.stdout}\n{result.stderr}'
    return result

loader = work/'loader.o'
run('portable loader', [cc,'-std=c11','-O2','-c',repo/'runtime/compute/loader.c','-o',loader])
provider = args.provider.resolve() if args.provider else None
device = args.device
if args.metal:
    provider = work/'libaether_compute_metal.dylib'
    device = 'metal'
    run('real Metal provider', ['clang','-O2','-fobjc-arc','-dynamiclib',repo/'runtime/metal/provider.m',repo/'runtime/metal/bridge.m',
                               '-framework','Metal','-framework','Foundation','-o',provider])
run('checked operator program',[compiler,'check',source,'--sysroot',repo])
link = [] if sys.platform=='darwin' else ['-l','dl']
output = re.compile(r'compute backend=(\d+) device=(\d+) values=10 error=([0-9.]+) used=(\d+) peak=(\d+) completed=(\d+)\n')
def execution(binary, mode, library=None, environment=env):
    result = run(f'{binary.name}/{mode}',[binary,mode,*([library] if library else [])],environment=environment)
    match = output.fullmatch(result.stdout)
    assert match and not result.stderr, (result.stdout,result.stderr)
    kind, ident, error, used, peak, completed = match.groups()
    assert int(completed)==(4 if mode=="vulkan" else 3)
    assert int(kind)=={'cpu':1,'metal':2,'vulkan':3}[mode]
    assert float(error)<=0.00001
    assert 0<int(used)<=int(peak)<=1048576
    if mode=='cpu': assert int(ident)==0

binaries=[]
for backend in args.backends:
    for level in (0,2):
        binary = work/f'compute-{backend}-O{level}'
        run(f'{backend}/O{level}/build',[compiler,source,'--sysroot',repo,'--backend',backend,f'-O{level}','--link-object',loader,*link,'-o',binary])
        binaries.append(binary)
        execution(binary,'cpu')
        if provider:
            execution(binary,device,provider)

# This adapter intentionally injects a terminal device failure. It tests error
# recovery and resource accounting, and is not GPU execution evidence.
fault_source = repo/'tests/regressions/compute/failure.aeth'
fault_provider = work/('fault-provider.dylib' if sys.platform=='darwin' else 'fault-provider.so')
shared = ['-dynamiclib'] if sys.platform=='darwin' else ['-shared','-fPIC']
run('failure injection provider',[cc,'-std=c11','-O2',*shared,repo/'tests/regressions/compute/fault_provider.c','-o',fault_provider])
for backend in args.backends:
    binary=work/f'compute-failure-{backend}'
    run(f'{backend}/failure/build',[compiler,fault_source,'--sysroot',repo,'--backend',backend,'-O2','--link-object',loader,*link,'-o',binary])
    result=run(f'{backend}/injected failure recovery',[binary,fault_provider])
    assert result.stdout=='compute injected failure recovered\n' and not result.stderr

# A missing/incompatible provider cannot silently execute the CPU algorithm.
run('missing provider fails',[binaries[0],'metal',work/'absent-provider.dylib'],success=False)
if provider:
    run('backend identity mismatch fails',[binaries[0],'vulkan' if device=='metal' else 'metal',provider],success=False)

if not args.no_sanitize:
    csource = work/'compute-sanitized.c'
    sanitized = work/'compute-sanitized'
    run('emit sanitizer C',[compiler,source,'--sysroot',repo,'--emit-c','-o',csource])
    run('ASan UBSan build',[cc,'-std=c11','-O0','-gline-tables-only','-fno-omit-frame-pointer','-fsanitize=address,undefined',
                            csource,repo/'runtime/compute/loader.c',*link,'-o',sanitized])
    sanitized_env = {**env,'ASAN_OPTIONS':'halt_on_error=1:exitcode=86','UBSAN_OPTIONS':'halt_on_error=1:exitcode=87'}
    execution(sanitized,'cpu',environment=sanitized_env)
    failure_c=work/'compute-failure-sanitized.c'
    failure_binary=work/'compute-failure-sanitized'
    run('emit failure sanitizer C',[compiler,fault_source,'--sysroot',repo,'--emit-c','-o',failure_c])
    run('failure ASan UBSan build',[cc,'-std=c11','-O0','-gline-tables-only','-fno-omit-frame-pointer','-fsanitize=address,undefined',
                                 failure_c,repo/'runtime/compute/loader.c',*link,'-o',failure_binary])
    run('sanitized injected failure recovery',[failure_binary,fault_provider],environment=sanitized_env)

    if args.metal:
        sanitized_provider = work/'libaether_compute_metal_sanitized.dylib'
        run('Metal adapter sanitizer build',['clang','-O0','-gline-tables-only','-fobjc-arc','-fsanitize=address,undefined','-dynamiclib',
                    repo/'runtime/metal/provider.m',repo/'runtime/metal/bridge.m','-framework','Metal','-framework','Foundation','-o',sanitized_provider])
        execution(sanitized,'metal',sanitized_provider,sanitized_env)
    elif provider:
        # Only Aether and the loader are instrumented; the supplied provider's
        # instrumentation belongs to its separate build evidence.
        execution(sanitized,device,provider,sanitized_env)

# Sixteen-qubit program shares the same queue, loader and provider. Its
# Aether oracle checks all amplitudes, Pauli values, gradients and cleanup.
statevector=repo/'tests/regressions/compute/statevector16.aeth'
for backend in args.backends:
    binary=work/f'statevector16-{backend}'
    run(f'{backend}/build statevector16',[compiler,statevector,'--sysroot',repo,'--backend',backend,'-O2','--link-object',loader,*link,'-o',binary])
    modes=[('cpu',[])]
    if provider: modes.append((device,[provider]))
    for mode,libraries in modes:
        execution_result=run(f'{backend}/statevector16/{mode}',[binary,mode,*libraries])
        assert 'qubits=16 amplitudes=65536' in execution_result.stdout
        assert 'submissions=42' in execution_result.stdout and execution_result.stdout.endswith('remaining=0\n')

paths=[compiler,source,'--sysroot',repo,statevector,statevector.with_name("circuit16.aeth"),repo/"stdlib/quantum_compute.aeth",repo/"stdlib/quantum.aeth",fault_source,repo/'tests/regressions/compute/fault_provider.c',repo/'stdlib/compute.aeth',repo/'stdlib/compute_types.aeth',repo/'runtime/compute/loader.c',repo/'runtime/compute/provider.h']
if args.metal: paths += [repo/'runtime/metal/provider.m',repo/'runtime/metal/bridge.m',repo/'runtime/metal/bridge.h']
if provider: paths.append(provider)
(work/'hashes.json').write_text(json.dumps({str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},indent=2))
print(f'{len(records)} compute checks passed; GPU: {device if provider else "not requested"}; evidence: {work / "results.json"}')
