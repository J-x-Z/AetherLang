#!/usr/bin/env python3
"""Native Core module loading, scope, diagnostics and external project regression."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import resource
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compiler', type=Path, required=True, help='self-hosted Core binary')
parser.add_argument('--format', choices=['c', 'llvm'], default='c')
parser.add_argument('--compare', type=Path, help='another Core generation for deterministic output')
parser.add_argument('--work-dir', type=Path, required=True)
parser.add_argument('--sanitize', action='store_true', help='sanitize generated external programs too')
parser.add_argument('--cc', default=os.environ.get('CC', 'clang'))
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
work = args.work_dir.resolve()
work.mkdir(parents=True, exist_ok=True)
compiler = args.compiler.resolve()
native_flags = ['-fsanitize=address,undefined', '-fno-omit-frame-pointer', '-fno-sanitize-recover=all'] if args.sanitize else []
cc = shutil.which(args.cc)
if not cc:
    raise SystemExit(f'C compiler not found: {args.cc}')
records = []
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
emit_flags = ['--emit-llvm'] if args.format == 'llvm' else []
env = {**os.environ, 'TMPDIR': str(work)}


def record(name, passed, **facts):
    records.append(dict(name=name, passed=bool(passed), **facts))
    (work / 'results.json').write_text(json.dumps(records, indent=2) + '\n')
    print(f'{"PASS" if passed else "FAIL"} {name}', flush=True)
    if not passed:
        raise SystemExit(f'Module regression failed: {name}')


def run(name, command, expected=0, stdout=None, diagnostic=None):
    command = list(map(str, command))
    started = time.monotonic()
    try:
        result = subprocess.run(command, cwd=work, env=env, capture_output=True, text=True, timeout=45)
    except subprocess.TimeoutExpired:
        record(name, False, command=command, error='timeout')
    correct_exit = result.returncode in expected if isinstance(expected, list) else result.returncode == expected
    record(name, correct_exit and (stdout is None or result.stdout == stdout)
           and (diagnostic is None or diagnostic in result.stdout + result.stderr),
           command=command, exit=result.returncode, expected_exit=expected,
           stdout=result.stdout, stderr=result.stderr, diagnostic=diagnostic,
           elapsed_seconds=time.monotonic()-started)
    return result


def files(name, contents):
    folder = work / name
    folder.mkdir(exist_ok=True)
    for relative, text in contents.items():
        file = folder / relative
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)
    return folder


def positive(name, contents, roots=(), expected='', levels=('2',), arguments=()):
    folder = files(name, contents)
    entry = folder / 'main.aeth'
    flags = [part for directory in roots for part in ('-I', folder / directory)]
    run(name+'-check', [compiler, entry, *flags, '--check'])
    generated = folder / ('output.ll' if args.format == 'llvm' else 'output.c')
    run(name+'-emit', [compiler, entry, *flags, *emit_flags, '-o', generated])
    if args.compare:
        compared = folder / ('compared.ll' if args.format == 'llvm' else 'compared.c')
        run(name+'-compare-emit', [args.compare.resolve(), entry, *flags, *emit_flags, '-o', compared])
        record(name+'-generations-identical', generated.read_bytes() == compared.read_bytes(),
               sha256=hashlib.sha256(generated.read_bytes()).hexdigest())
    for level in levels:
        binary = folder / ('program-O'+level)
        run(name+'-cc-O'+level, [cc, '-O'+level, *native_flags, generated, '-o', binary])
        run(name+'-run-O'+level, [binary, *arguments], stdout=expected)
    return folder


fixture = root / 'tests/regressions/bootstrap/modules'
content = {str(p.relative_to(fixture)): p.read_text() for p in fixture.rglob('*.aeth')}
data = work / 'numbers.txt'
data.write_text('\n'.join(map(str, range(1, 65))))
project = positive('external-project 空间', content, levels=('0', '2'), arguments=(data,),
                   expected='count=64 sum=2080 squares=89440\n')
bad_data = work / 'bad-numbers.txt'
bad_data.write_text('1\n2\ninvalid\n')
run('external-project-invalid-data', [project / 'program-O2', bad_data], expected=4)
run('external-project-missing-file', [project / 'program-O2', work / 'missing-data'], expected=4)

positive('alias-private', {
 'main.aeth':'use a::value as first; use b as other; fn main()->i32{return first()+other::value()-42;}',
 'a.aeth':'struct Hidden{x:i32} fn helper()->i32{return 20;} pub fn value()->i32{return helper();}',
 'b.aeth':'struct Hidden{x:u8} fn helper()->i32{return 22;} pub fn value()->i32{return helper();}'})
positive('roots-siblings', {
 'main.aeth':'use lib; fn main()->i32{return lib::value()-42;}',
 'first/lib.aeth':'use sibling; pub fn value()->i32{return sibling::value();}',
 'first/sibling.aeth':'pub fn value()->i32{return 42;}',
 'second/lib.aeth':'pub fn value()->i32{return 7;}',
 'second/sibling.aeth':'pub fn value()->i32{return 9;}'}, roots=('first','second'))
positive('local-before-roots', {
 'main.aeth':'use value; fn main()->i32{return value::number()-42;}',
 'value.aeth':'pub fn number()->i32{return 42;}',
 'root/value.aeth':'pub fn number()->i32{return 9;}'}, roots=('root',))
positive('diamond', {
 'main.aeth':'use a;use b;fn main()->i32{return a::value()+b::value()-42;}',
 'a.aeth':'use common::*;pub fn value()->i32{return number;}',
 'b.aeth':'use common as shared;pub fn value()->i32{return shared::number;}',
 'common.aeth':'pub const number:i32=21;'})
# Both aliases resolve to the same canonical file; its nominal type must be shared.
folder = files('canonical', {
 'main.aeth':'use original;use alias;fn main()->i32{let p:*original::Value=0 as *original::Value;alias::take(p);return 0;}',
 'original.aeth':'pub struct Value{x:i32} pub fn take(p:*Value){}'})
alias = folder / 'alias.aeth'
if alias.is_symlink():
    alias.unlink()
alias.symlink_to('original.aeth')
positive('canonical', {})
positive('foreign-compatible', {
 'main.aeth':'use a::*;use b::*;fn main()->i32{puts("shared foreign");puts("shared foreign");return 0;}',
 'a.aeth':'extern "C"{fn puts(text:*u8)->i32;}',
 'b.aeth':'extern "C"{fn puts(other:*u8)->i32;}'}, expected='shared foreign\nshared foreign\n')
positive('imported-main-private', {
 'main.aeth':'use library;fn main()->i32{return library::value()-42;}',
 'library.aeth':'fn main(value:i64)->i64{return value;}pub fn value()->i32{return main(42) as i32;}'})

positive('late-import-and-type-alias', {
 'main.aeth':'extern "C"{fn puts(s:*u8)->i32;}fn main()->i32{let values:Pair=[20,22];return sum(values)-42;}use lib::{Pair,sum};',
 'lib.aeth':'pub type Pair=[i32;2];pub fn sum(values:Pair)->i32{values[0]+values[1]}'},levels=('0','2'))
positive('alias-forward-pointer', {
 'main.aeth':'type First=Later;fn take(p:*First)->i32{*p}type Later=i32;fn main()->i32{let x:i32=42;return take(&x)-42;}'})
positive('container-values', {'main.aeth':(root/'tests/regressions/bootstrap/container_values.aeth').read_text()},
         expected='containers=ok\n',levels=('0','2'))

positive('block-values', {'main.aeth':(root/'tests/regressions/bootstrap/block_values.aeth').read_text()},
         expected='block-values=ok\n',levels=('0','2'))

positive('unit-values', {'main.aeth':(root/'tests/regressions/bootstrap/unit_values.aeth').read_text()},
         expected='unit-values=ok\n',levels=('0','2'))

bad = {
 'foreign-unit-parameter': ({'main.aeth':'extern "C"{fn foreign(value:());}fn main(){}'}, 'foreign unit parameter ABI'),
 'unit-arithmetic': ({'main.aeth':'fn main(){let x:()=()+();}'}, 'invalid binary operands'),
 'value-missing-else': ({'main.aeth':'fn main(){let flag:bool=false;let x:i32=if flag{1};}'}, 'without yielding'),
 'value-discards-tail': ({'main.aeth':'fn main(){let x:i32={1;};}'}, 'without yielding'),
 'value-branch-type': ({'main.aeth':'fn main(){let x:i32=if true{1}else{false};}'}, 'type mismatch'),
 'value-scope': ({'main.aeth':'fn main(){let x:i32={let local:i32=1;local};let y:i32=local;}'}, 'unknown value'),
 'value-break-outside-loop': ({'main.aeth':'fn main(){let x:i32={break;};}'}, 'outside a loop'),
 'alignment-unknown': ({'main.aeth':'fn main(){align_of(Missing);}'}, 'unknown struct type'),
 'array-wrong-length': ({'main.aeth':'fn main(){let a:[i32;2]=[1];}'}, 'array initializer length'),
 'array-wrong-element': ({'main.aeth':'fn main(){let a:[i32;1]=[true];}'}, 'type mismatch'),
 'array-repeat-length': ({'main.aeth':'fn main(){let a:[i32;2]=[1;3];}'}, 'array initializer length'),
 'array-negative-length': ({'main.aeth':'fn main(){let a:[i32;-1]=[];}'}, 'nonnegative integer'),
 'array-size-overflow': ({'main.aeth':'fn main(){let p:*[i64;1152921504606846976]=0 as *[i64;1152921504606846976];}'}, 'array object exceeds'),
 'alias-cycle': ({'main.aeth':'type A=B;type B=A;fn main(){}'}, 'cyclic'),
 'alias-pointer-cycle': ({'main.aeth':'type A=*A;fn main(){}'}, 'cyclic'),
 'array-immutable': ({'main.aeth':'fn main(){let a:[i32;2]=[1,2];a[0]=3;}'}, 'immutable'),
 'compound-immutable': ({'main.aeth':'fn main(){let a:i32=1;a+=2;}'}, 'immutable'),
 'compound-wrong-type': ({'main.aeth':'fn main(){let mut a:i32=1;a+=true;}'}, 'binary operand types'),
 'compound-array': ({'main.aeth':'fn main(){let mut a:[i32;1]=[1];a+=[2];}'}, 'invalid binary'),
 'foreign-array': ({'main.aeth':'extern "C"{fn foreign(a:[i32;2]);}fn main(){}'}, 'foreign array value ABI'),
 'loop-reachable-break': ({'main.aeth':'fn f()->i32{loop{break;}}fn main(){}'}, 'function may finish without returning'),
 'alias-private': ({'main.aeth':'use lib;fn main(){let x:lib::Hidden=0;}','lib.aeth':'type Hidden=i32;'}, 'unknown or private'),
 'group-separator': ({'main.aeth':'use lib::{a b};fn main(){}','lib.aeth':'pub fn a(){}pub fn b(){}'}, "expected ',' or '}'"),
 'struct-duplicate': ({'main.aeth':'struct S{x:i32}fn main(){let x:S=S{x:1,x:2};}'}, 'duplicate struct initializer field'),
 'struct-missing': ({'main.aeth':'struct S{x:i32,y:i32}fn main(){let x:S=S{x:1};}'}, 'missing struct initializer field'),
 'struct-unknown': ({'main.aeth':'struct S{x:i32}fn main(){let x:S=S{y:1};}'}, 'unknown struct initializer field'),
 'struct-wrong-type': ({'main.aeth':'struct S{x:i32}fn main(){let x:S=S{x:true};}'}, 'type mismatch'),
 'tail-semicolon-discards': ({'main.aeth':'fn value()->i32{42;}fn main()->i32{return 0;}'}, 'function may finish without returning'),
 'tail-if-semicolon-discards': ({'main.aeth':'fn value(flag:bool)->i32{if flag{1}else{2};}fn main()->i32{return 0;}'}, 'function may finish without returning'),
 'foreign-main': ({'main.aeth':'use lib;fn main()->i32{return 0;}','lib.aeth':'extern \"C\"{fn main()->i64;}'}, 'main requires a non-foreign'),
 'private-function': ({'main.aeth':'use lib;fn main()->i32{return lib::secret();}', 'lib.aeth':'fn secret()->i32{return 7;}'}, 'unknown or private imported name'),
 'private-type': ({'main.aeth':'use lib;fn main(){let p:*lib::Secret=0 as *lib::Secret;}', 'lib.aeth':'struct Secret{x:i32}'}, 'unknown or private imported name'),
 'private-selected': ({'main.aeth':'use lib::secret;fn main(){}', 'lib.aeth':'fn secret(){}'}, 'unknown or private imported name'),
 'missing-module': ({'main.aeth':'use absent;fn main(){}'}, 'imported module not found'),
 'cycle': ({'main.aeth':'use lib;fn main(){}', 'lib.aeth':'use main;'}, 'cyclic module import'),
 'duplicate-alias': ({'main.aeth':'use a as same;use b as same;fn main(){}', 'a.aeth':'pub fn f(){}','b.aeth':'pub fn g(){}'}, 'duplicate import binding'),
 'ambiguous': ({'main.aeth':'use a::*;use b::*;fn main()->i32{return number;}', 'a.aeth':'pub const number:i32=1;','b.aeth':'pub const number:i32=2;'}, 'ambiguous imported name'),
 'imported-ambiguity-location': ({'main.aeth':'use lib;fn main(){}','lib.aeth':'use a::*;use b::*;\n\npub fn value()->i32{return number;}','a.aeth':'pub const number:i32=1;','b.aeth':'pub const number:i32=2;'}, 'lib.aeth:3:'),
 'local-import': ({'main.aeth':'use lib::*;fn name(){}fn main(){}', 'lib.aeth':'pub fn name(){}'}, 'local declaration conflicts'),
 'nominal': ({'main.aeth':'use a;use b;fn main(){let p:*a::Value=0 as *a::Value;b::take(p);}', 'a.aeth':'pub struct Value{x:i32}', 'b.aeth':'pub struct Value{x:i32}pub fn take(p:*Value){}'}, 'type mismatch'),
 'foreign-alias-collision': ({'main.aeth':'use lib::foo as bar;extern \"C\"{fn bar(x:i32)->i32;}fn main()->i32{return bar(1);}','lib.aeth':'extern \"C\"{fn foo(x:i32)->i32;}'}, 'local declaration conflicts'),
 'foreign-mismatch': ({'main.aeth':'use a;use b;fn main(){}','a.aeth':'extern "C"{fn foreign(value:i32);}','b.aeth':'extern "C"{fn foreign(value:i64);}'}, 'incompatible cross-module foreign declarations'),
 'malformed-import': ({'main.aeth':'use lib:'}, 'expected ::'),
 'imported-syntax': ({'main.aeth':'use lib;fn main(){}','lib.aeth':'\npub fn broken( {'}, 'lib.aeth:2:'),
 'imported-type': ({'main.aeth':'use lib;fn main(){}','lib.aeth':'\npub fn broken()->i32{return true;}'}, 'lib.aeth:2:'),
 'unbound-module': ({'main.aeth':'fn main()->i32{return missing::value();}'}, 'unknown imported module'),
 'module-limit': ({**{f'm{i}.aeth':'pub fn value(){}' for i in range(256)}, 'main.aeth':''.join(f'use m{i};' for i in range(256))+'fn main(){}'}, 'source graph exceeds 256 modules'),
 'depth-limit': ({**{f'm{i}.aeth':f'use m{i+1};' for i in range(65)}, 'm65.aeth':'pub fn end(){}', 'main.aeth':'use m0;fn main(){}'}, 'module import depth exceeds 64'),
}
for name, (contents, diagnostic) in bad.items():
    folder = files('reject-'+name, contents)
    output = folder / 'prior.c'
    output.write_text('USER CONTENT\n')
    run(name+'-check-rejected', [compiler, folder / 'main.aeth', '--check'], expected=1, diagnostic=diagnostic)
    run(name+'-emit-rejected', [compiler, folder / 'main.aeth', *emit_flags, '-o', output], expected=1, diagnostic=diagnostic)
    record(name+'-output-preserved', output.read_text() == 'USER CONTENT\n')

trap_folder = files('container-traps', {'main.aeth':(root/'tests/regressions/bootstrap/container_traps.aeth').read_text()})
trap_source = trap_folder / ('output.ll' if args.format=='llvm' else 'output.c')
run('container-traps-emit',[compiler,trap_folder/'main.aeth',*emit_flags,'-o',trap_source])
for level in ['0','2']:
    binary=trap_folder/('program-O'+level)
    run('container-traps-build-O'+level,[cc,'-O'+level,*native_flags,trap_source,'-o',binary])
    for case in range(7):
        run(f'container-trap-{case}-O{level}',[binary,str(case)],expected=[-signal.SIGILL,-signal.SIGTRAP])

originals = {str(p): p.read_bytes() for p in project.rglob('*.aeth')}
imported = project / 'reader.aeth'
run('protect-imported-source', [compiler, project/'main.aeth', *emit_flags, '-o', imported], expected=1,
    diagnostic='output must not replace any module source')
record('all-imported-source-preserved', all(Path(p).read_bytes()==data for p,data in originals.items()))
run('missing-root-argument', [compiler, project/'main.aeth', '-I'], expected=1, diagnostic='-I requires a directory')
run('invalid-search-root', [compiler, project/'main.aeth', '-I', work/'missing-root', '--check'], expected=1,
    diagnostic='cannot resolve -I root')
print(f'Core modules complete: {len(records)} checks; evidence {work / "results.json"}')
