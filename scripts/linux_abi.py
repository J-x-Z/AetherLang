#!/usr/bin/env python3
"""Inspect the shipped ELF files and resolve their dependencies after relocation."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess


def output(*args):
    env = {k: v for k, v in os.environ.items() if not k.startswith('LD_')}
    return subprocess.check_output(args, env={**env, 'LC_ALL': 'C'}, text=True,
                                   stderr=subprocess.STDOUT)


def version_needs(path):
    # readelf also lists versions exported by a library. Only .gnu.version_r
    # describes what this particular ELF file needs from its providers.
    lines = output('readelf', '-W', '-V', path).splitlines()
    active = False
    requirements = {}
    for line in lines:
        if line.startswith('Version '):
            active = line.startswith('Version needs section ')
        elif active:
            name = re.search(r'\bFile: (\S+)', line)
            if name:
                versions = requirements.setdefault(name[1], [])
            else:
                name = re.search(r'\bName: (\S+)', line)
                if name:
                    versions.append(name[1])
    return requirements


def inspect(path, root, machine):
    header = output('readelf', '-W', '-h', path)
    if 'ELF64' not in header or not re.search(r'^\s*Machine:\s+' + machine + r'\s*$', header, re.M):
        raise ValueError(f'{path.name} is not a native 64-bit {machine} ELF')
    dynamic = output('readelf', '-W', '-d', path)
    needed = re.findall(r'\(NEEDED\).*?\[(.+?)\]', dynamic)
    loaded = output('ldd', path)
    if 'not found' in loaded:
        raise ValueError(f'unresolved dependencies for {path.name}:\n{loaded}')
    resolved = {}
    for line in loaded.splitlines():
        dep = re.match(r'^\s*(\S+) => (/.+?) \(0x[0-9a-f]+\)', line)
        if dep:
            location = Path(dep[2]).resolve(strict=True)
            resolved[Path(dep[1]).name] = ('$PACKAGE/' + location.relative_to(root).as_posix()
                                  if location.is_relative_to(root) else str(location))
        else:
            loader = re.match(r'^\s*(/\S+) \(0x[0-9a-f]+\)', line)
            if loader:
                resolved[Path(loader[1]).name] = loader[1]
    if missing := set(needed) - resolved.keys():
        raise ValueError(f'{path.name}: unresolved DT_NEEDED {sorted(missing)}')
    return dict(needed=needed, resolved=resolved, version_needs=version_needs(path),
                soname=re.findall(r'\(SONAME\).*?\[(.+?)\]', dynamic),
                runpath=re.findall(r'\((?:RUNPATH|RPATH)\).*?\[(.+?)\]', dynamic))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--package', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = args.package.resolve(strict=True)
    manifest = json.loads((root/'manifest.json').read_text())
    arch = platform.machine()
    machine = {'aarch64': 'AArch64', 'x86_64': 'Advanced Micro Devices X86-64'}.get(arch)
    if platform.system() != 'Linux' or machine is None or manifest['platform']['machine'] != arch:
        parser.error('run this check on the native Linux release architecture')
    files = {}
    for item in manifest['artifacts']:
        path = root/item['name']
        with path.open('rb') as stream:
            if stream.read(4) == b'\x7fELF':
                files[item['name']] = inspect(path, root, machine)
    seed = files['aethc']
    llvm, = [name for name in seed['needed'] if name.startswith('libLLVM')]
    library = 'lib/' + llvm
    if (seed['resolved'][llvm] != '$PACKAGE/' + library or files[library]['soname'] != [llvm]
            or '$ORIGIN/lib' not in ':'.join(seed['runpath']).split(':')):
        raise ValueError('the seed must resolve the matching LLVM SONAME inside the relocated package')
    versions = {s for binary in files.values() for tags in binary['version_needs'].values() for s in tags}
    # These are minimum *symbol versions* in the shipped ELF files. Separate
    # SONAMEs and named capabilities (e.g. GLIBC_ABI_DT_RELR) matter too.
    floors = {}
    for family in ('GLIBC', 'GLIBCXX', 'CXXABI'):
        numbers = [s[len(family)+1:] for s in versions if re.fullmatch(family+r'_\d+(?:\.\d+)+', s)]
        if numbers:
            floors[family] = max(numbers, key=lambda v: tuple(map(int, v.split('.'))))
    report = dict(source_commit=manifest['source_commit'], machine=arch,
                  distro=platform.freedesktop_os_release()['PRETTY_NAME'],
                  host_libc=output('getconf', 'GNU_LIBC_VERSION').strip(),
                  tested_clang=manifest['clang'], bundled_llvm=llvm,
                  required_symbol_versions=floors, files=files)
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True)+'\n')
    print(f'{report["distro"]} {arch}: {llvm}, {floors}, {len(files)} ELF files')


if __name__ == '__main__':
    main()
