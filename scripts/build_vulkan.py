#!/usr/bin/env python3
"""Compile the Vulkan 1.0 compute provider with a native or NDK C compiler."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--cc', default=os.environ.get('CC', 'clang'))
parser.add_argument('--glslc', default='glslc')
parser.add_argument('--include', type=Path, action='append', default=[])
parser.add_argument('--library', default='-lvulkan', help='Vulkan linker flag or absolute library file')
args = parser.parse_args()
source = Path(__file__).resolve().parents[1]/'runtime/vulkan'
out = args.output.resolve()
out.parent.mkdir(exist_ok=True, parents=True)
with tempfile.TemporaryDirectory(prefix='.vulkan-', dir=out.parent) as tmp:
    work = Path(tmp)
    subprocess.run([args.glslc, '--target-env=vulkan1.0', '-O', '-mfmt=c', source/'gemm.comp',
                    '-o', work/'gemm_spv.h'], check=True)
    link = '-dynamiclib' if out.suffix == '.dylib' else '-shared'
    includes = [flag for path in [work,*args.include] for flag in ['-I', str(path)]]
    subprocess.run([args.cc,'-std=c11','-O2','-fPIC','-Wall','-Wextra','-Werror',*includes,
                    link,source/'provider.c',args.library,'-o',work/out.name], check=True)
    os.replace(work/out.name, out)
