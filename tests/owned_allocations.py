"""Observe real host allocation balance for scoped vectors, tensors and leases."""
import argparse
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', required=True, type=Path)
parser.add_argument('--work-dir', required=True, type=Path)
args = parser.parse_args()
fixtures = Path(__file__).resolve().parent / 'regressions'
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
env = {**os.environ, 'TMPDIR': str(work), 'UBSAN_OPTIONS': 'halt_on_error=1'}
def run(command):
    subprocess.run(command, check=True, env=env, timeout=60)
cc = os.environ.get('CC', 'clang')
source = work / 'program.c'
obj = work / 'program.o'
exe = work / 'count-resources'
sanitize = ['-g', '-O1', '-fsanitize=address,undefined', '-fno-omit-frame-pointer']
run([args.compiler.resolve(), fixtures / 'owned_resources.aeth', '--emit-c', '-o', source])
run([cc, '-std=c11', *sanitize, '-Dmain=owned_resource_program', '-Dmalloc=counted_malloc',
     '-Drealloc=counted_realloc', '-Dfree=counted_free', '-c', source, '-o', obj])
run([cc, *sanitize, obj, fixtures / 'owned_allocator.c', '-lm', '-o', exe])
run([exe])
