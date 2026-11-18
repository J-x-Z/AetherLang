#!/usr/bin/env python3
"""Build Windows x64 GPU fixtures using Aether and a MinGW Clang driver, or run them on Windows."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

repo = Path(__file__).resolve().parents[1]
fixtures = ('provider_abi', 'runtime', 'statevector16')
parser = argparse.ArgumentParser(description=__doc__)
sub = parser.add_subparsers(dest='action', required=True)
build = sub.add_parser('build', help='compile the provider, reusable loader object and Aether programs')
build.add_argument('--compiler', type=Path, required=True, help='Aether stage-0 compiler with target-aware code generation')
build.add_argument('--cc', default='clang', help='MinGW Clang or Zig cc wrapper')
build.add_argument('--glslc', default='glslc')
build.add_argument('--include', type=Path, help='Vulkan-Headers include directory')
build.add_argument('--library', type=Path, help='Vulkan loader import library for Windows x64')
test = sub.add_parser('run', help='require actual GPU dispatch and record the results')
test.add_argument('--vendor', default='0', help='expected PCI vendor in hexadecimal, or 0 for any')
for cmd in (build, test):
    cmd.add_argument('--dir', type=Path, required=True, help='Windows executable and provider directory')
    cmd.add_argument('--backends', nargs='+', choices=['c','llvm'], default=['c','llvm'])
args = parser.parse_args()
out = args.dir.resolve()
dll = out/'aether_vulkan.dll'
loader = out/'loader.obj'
if args.action == 'build':
    out.mkdir(exist_ok=True, parents=True)
    target = 'x86_64-windows-gnu'
    cc = [args.cc, '--target=' + target, '-O2']
    provider = [sys.executable, repo/'scripts/build_vulkan.py', '--cc', args.cc, '--target', target,
                '--glslc', args.glslc, '--output', dll]
    if args.include:
        provider.extend(['--include', args.include])
    if args.library:
        provider.extend(['--library', args.library.resolve()])
    subprocess.run(provider, check=True)
    subprocess.run([*cc, '-Wall','-Wextra','-Werror','-std=c11','-c',repo/'runtime/compute/loader.c','-o',loader], check=True)
    with tempfile.TemporaryDirectory(prefix='aether-gpu-', dir=out) as temp:
        for name in fixtures:
            source = repo/'tests/regressions/compute'/f'{name}.aeth'
            for backend in args.backends:
                emitted = Path(temp)/(name + ('.c' if backend=='c' else '.obj'))
                emit = '--emit-c' if backend=='c' else '--emit-obj'
                subprocess.run([args.compiler,source,'--sysroot',repo,'--backend',backend,'--target',
                                'x86_64-pc-windows-gnu','-O2',emit,'-o',emitted],check=True)
                subprocess.run([*cc,'-s',emitted,loader,'-o',out/f'{name}-{backend}.exe'],check=True)
else:
    if os.name != 'nt':
        parser.error('run requires native Windows')
    if not 0 <= int(args.vendor, 16) <= 0xffff:
        parser.error('--vendor must fit a PCI vendor ID')
    files = [dll, loader, *(out/f'{f}-{b}.exe' for f in fixtures for b in args.backends)]
    report = dict(host=platform.node(), platform=platform.platform(),
                  hashes={f.name: hashlib.sha256(f.read_bytes()).hexdigest() for f in files}, runs=[])
    def run(name, backend, argv, expected, matches):
        binary = out/f'{name}-{backend}.exe'
        result = subprocess.run([binary,*argv],cwd=out,capture_output=True,text=True,timeout=60)
        report['runs'].append(dict(binary=binary.name,args=list(map(str,argv)),exit=result.returncode,
                                   stdout=result.stdout,stderr=result.stderr))
        (out/'windows-vulkan-results.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        if result.returncode!=expected or any(value not in result.stdout for value in matches):
            raise SystemExit(f'{binary.name} {argv}: {result.returncode}\n{result.stdout}{result.stderr}')
    for backend in args.backends:
        run('provider_abi',backend,[r'.\aether_vulkan.dll',args.vendor],0,['gpu=','checked=99'])
        run('provider_abi',backend,[dll.name,args.vendor],3,['open error 1 native 0'])
        run('provider_abi',backend,[r'.\missing-aether-provider.dll',args.vendor],3,['open error 10 native 126'])
        for mode, path, ident in [('cpu',[],1),('vulkan',[str(dll)],3)]:
            run('runtime',backend,[mode,*path],0,[f'compute backend={ident} ','error=0.00000000'])
            run('statevector16',backend,[mode,*path],0,[f'quantum backend={ident} qubits=16 amplitudes=65536',
                                                        'submissions=42 ','remaining=0\n'])
    print(out/'windows-vulkan-results.json')
