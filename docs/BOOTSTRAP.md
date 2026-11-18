# Self-hosting Core compiler

`src_aether/core/` is a complete compiler written in Aether Core: lexer,
recursive-descent/Pratt parser, name and type checker, typed C11 emission, and
direct LLVM IR emission.
It compiles all of its own source. The Rust compiler remains the full language
compiler, with LLVM as the primary backend and C as the portable alternative.
Core does not implement the full Rust compiler's language yet.

Run the entire chain with an existing freshly built compiler:

```sh
python3 tests/bootstrap_core.py --compiler /path/to/aethc \
  --work-dir /external/work/bootstrap --backend llvm
```

Use `--backend c` for a C-only seed. The runner copies the Core source files
verbatim into its work directory and builds stage1 from the real entry
`src_aether/core/main.aeth`. Each Aether compiler then finds, opens, lexes and
parses its own module graph to generate stages2, 3 and 4. No source concatenation,
Python import expansion, or generated front-end shim is involved. Clang compiles
generated C into machine code. Stages1 onward never invoke the Rust compiler;
the runner places a failing `aethc` command in PATH to enforce that boundary.
The three generated C files must be byte-identical. The self-compiled compiler
also compiles and executes independent single-file and multi-module workloads.

A built Core compiler accepts:

```sh
aethc-core project/main.aeth -I shared/modules -I other/modules --check
aethc-core project/main.aeth -I shared/modules -o program.c
clang -std=c11 -O2 program.c -lm -o program
```

The current Core driver can also invoke the host compiler and publish native
artifacts. This is the entry point JXZ uses when `AETHC` points to `aethc-core`.

```sh
aethc-core check main.aeth --backend llvm --sysroot /path/to/aetherlang
aethc-core build -O 2 --module mathlib=deps/math/src/lib.aeth --link-object native.o -o app -- main.aeth
aethc-core --emit-obj --backend c -O2 main.aeth -o main.o
```

Native build defaults to direct LLVM IR and Clang. Select `--backend c` to
compile generated C11. Set `CC` to one compiler executable. The driver passes
arguments directly, checks child termination and native output type, and
publishes only after a successful build. Existing executables survive a failed
link. Linux ELF64 and macOS Mach-O64 are verified; target selection in this
driver is native only. Bare `input.aeth -o output.c` retains the source-emission
interface used by the preceding bootstrap generation. `--emit-llvm` emits IR
without invoking the host compiler. `-I`, exact module bindings, `AETHER_PATH`,
and the installed standard library are supported. Directory roots never expose
siblings of an exact `NAME=FILE.aeth` package binding.

Imports may appear between top-level declarations. Supported forms are `use library;`,
`use library as alias;`, `use library::item;`, `use library::item as alias;`,
and `use library::*;`. Flat groups such as
`use library::{first, second as alias};` are supported. Named imports permit `library::function()`,
`library::CONSTANT`, and `*library::Type`; selected and wildcard imports bring
public declarations into the file's scope. A wildcard also binds its module
name. Nested import trees and re-exports are not yet supported.

Each module has private functions, constants and nominal struct types; `pub`
exports a declaration, while `extern "C"` declarations expose their ABI name.
Exported struct fields follow the existing Core field-access rules. Imports do
not expose another module's imports. Conflicting imported names, duplicate
bindings, private-name access and incompatible foreign declarations are errors.
Repeated compatible declarations of the same C symbol share that ABI identity;
an alias of a different C symbol cannot silently replace a local declaration.

Resolution tries `name.aeth`, then `name/mod.aeth` in the importing file's real
source directory, then each `-I` directory in argument order. Paths are
canonicalized, so diamonds and symlink aliases reuse one parsed module and one
nominal type identity. Cyclic imports are rejected. The root file alone supplies
the executable `main`; an imported file's private `main` remains an ordinary
function. Diagnostics retain the originating file, line and column.

Core v1 supports functions, C FFI and variadics, explicit scalar/pointer/struct
types, fixed arrays, transparent type aliases, constants, lexical locals, casts, fields, indexing, `size_of`/`align_of`, arithmetic,
conditionals, loops, returns, and raw-address operations. All locals require an
initializer. Non-void functions must return a value on every reachable finishing path; a
provably diverging `loop` or `while true` does not require a trailing return.
Only `main()` or `main(argc:i32, argv:**u8)` is an entry point, returning void/i32.
Semicolons are optional at the same token boundaries as the primary parser. A
final expression without a semicolon supplies a non-void function value,
including final blocks and `if` branches; a semicolon discards that value.
`if` and block expressions also produce typed values inside initializers, calls
and larger expressions. Early returns and loop exits retain their enclosing
function/loop destination; only paths that continue need to yield the value.
Struct values can be constructed, copied and returned; recursive layouts require
pointers. Initializers must supply every field exactly once, may use shorthand
names, and evaluate fields in source order regardless of declaration order.
Fixed arrays use `[T; N]` types with a nonnegative integer literal extent, `[a, b]`
values and `[value; N]` repeat values. They can be copied, passed and returned by
value, nested, or stored in structs. A repeat evaluates its initializer once,
including when `N` is zero. Array indexing traps before access when the index is
negative or outside the extent; raw pointer indexing retains its unchecked
system-programming semantics. Impossible object sizes are rejected before native
compilation. Foreign by-value arrays remain explicitly unsupported.

`type Name = Type` and `pub type Name = Type` are transparent, including forward
aliases and imported aliases; cyclic aliases are rejected. `loop`, `break` and
`continue` use the same reachable-exit rules as ordinary `while` statements.
Compound arithmetic and bitwise assignments evaluate the place once, load its
old value, then evaluate the right operand before storing the result.

`()` is a real zero-sized value with alignment one; `void` is its existing
alias. Unit values can be passed internally, copied, stored in structs and arrays,
and returned. Returning Unit keeps the native `void` return ABI. Raw Unit pointers
use C `void*`; their zero-stride arithmetic and zero-byte loads/stores never access
sentinel memory. Foreign by-value Unit parameters remain explicitly rejected.
Unit-returning functions discard an ordinary final expression consistently even
when their return type is spelled through an alias.

Integer operations wrap by width except division/remainder by zero and signed
MIN/-1, which trap. Operand and argument evaluation is left-to-right, Boolean
operators short-circuit, and float-to-integer casts saturate (NaN becomes zero).

String escapes use the same UTF-8 rules as the primary compiler: `\xHH`
consumes exactly two hexadecimal digits and accepts only ASCII `00..7f`;
`\u{H...}` accepts one to six hexadecimal digits naming a Unicode scalar
through `10ffff`, excluding surrogates `d800..dfff`, and emits its UTF-8 bytes.
Raw Unicode text and the existing `\n`, `\r`, `\t`, `\0`, `\\`, `\"`, and `\'`
escapes remain supported. Embedded NUL is preserved in the token length.
Unknown or malformed escapes are errors. Migrate former `\x80..\xff` string
escapes to a Unicode scalar such as `\u{e9}` (UTF-8 `c3 a9`), or write numeric
`u8` values into a byte buffer when the intended data is arbitrary binary.
Core still has no character-literal syntax. The semantic corpus includes
native byte checks for UTF-8 boundaries, NUL, ASCII escapes, and raw Unicode.

The bootstrap compiler still rejects generics, enums, methods,
references, attributes and other syntax outside this subset. These features
remain available in the primary compiler. The Core compiler targets the
existing 64-bit host C ABI; its `usize`/`isize` and libc declarations do not yet
provide a cross-target data model. This is a real self-host with native module
loading and LLVM IR generation, not a replacement of the full language compiler.
LLVM optimization, native instruction selection and linking still use the host
LLVM/Clang toolchain.

The compiler owns an arena released on success and failure, limits input to
32 MiB across the source graph and arena allocations to 512 MiB. Module
loading is limited to 256 unique files and an import depth of 64; syntactic and
actual AST depth are also bounded. Output is written to a temporary sibling and atomically renamed; failed
compilation leaves an existing output intact. Output may not replace the
entry source or any imported source, including canonical path aliases. Handles and raw pointers in compiled programs have explicit
lifetimes; this subset does not add an automatic destructor system.

Validation includes recursive heap structures, NUL-containing files, call and
assignment order, numeric boundaries, malformed inputs, and ASan/UBSan builds
of the compiler compiling itself and loading valid/invalid module graphs. The
external file-statistics project combines heap ownership, byte parsing, private
helpers, imported structs and filesystem I/O. Run its module corpus separately
with `python3 tests/bootstrap_modules.py --compiler /path/to/aethc-core
--work-dir /external/work/core-modules`; `--compare` accepts another generation
for byte-identical generated C checks. Results contain stage hashes, sizes, commands
and single-run timings. The earlier placeholder `src_aether/*.aeth` pipelines
and stale bootstrap binaries were removed; their history remains in Git.

The Core emitter selects a C identifier prefix disjoint from the program's
foreign function names. Generated helpers, types and bindings share this
prefix. External calls retain the requested ABI names. The bootstrap suite
links an independently compiled C companion and executes both the foreign
functions and the generated arithmetic helpers at O0, O2 and with sanitizers.

A positive integer literal explicitly cast to `u64` or a raw pointer accepts
the full 64-bit unsigned range. Ordinary signed arithmetic before a cast keeps
its signed division semantics.

The direct LLVM path is selected explicitly:

```sh
aethc-core project/main.aeth --emit-llvm -o program.ll
clang -O2 program.ll -lm -o program
python3 tests/bootstrap_llvm.py --compiler /external/work/bootstrap/stage4 \
  --work-dir /external/work/bootstrap-llvm
```

This second runner starts with an already self-hosted Core compiler. That
compiler builds the new compiler's complete module graph through C;
the resulting compiler emits LLVM IR for its complete source. Three consecutive
LLVM-built generations must reproduce identical `.ll` bytes. The LLVM-built
compiler must also reproduce the current emitter's C output exactly, comparing
the same implementation built through C and through LLVM. The runner
blocks accidental calls to Rust `aethc`, runs external module projects, and
executes heap/aggregate, file I/O, short-circuit, numeric, UTF-8, and C ABI
workloads at O0/O2. Its ASan/UBSan build instruments the C-built compiler while
it executes the Aether-written LLVM emitter; it does not claim C-front-end
UBSan instrumentation for arbitrary LLVM IR inputs.

The LLVM emitter uses explicit control flow and typed values: local storage is
allocated once at function entry, struct values retain their layout, pointer
indexing uses `getelementptr`, and short-circuit operands have actual conditional
branches. Integer operations wrap without `nsw`/`nuw`; division/remainder guards
trap zero and signed MIN/-1. Float casts use LLVM saturating conversion intrinsics, following the
[LLVM language reference](https://llvm.org/docs/LangRef.html#saturating-floating-point-to-integer-conversions).
Scalar and pointer C ABI calls include narrow-integer extensions and C varargs
promotions. Internal aggregate arguments/results are supported. External C
aggregate-by-value parameters/results are rejected with a source diagnostic
until target-specific C aggregate ABI lowering is implemented; ordinary pointer
to struct FFI remains supported. The LLVM path is currently a 64-bit host path,
with no cross-target ABI claim.

Existing application/standard-library validation is separate from compiler
fixtures:

```sh
python3 tests/bootstrap_production.py --compiler /external/work/bootstrap-llvm/stage4 \
  --work-dir /external/work/bootstrap-production
```

This runner compiles the unchanged `examples/upgrade/accumulator.aeth` and its
real `stdlib/parse_int.aeth` dependency through both self-hosted emitters at
O0/O2. It verifies the live request/response loop, journal bytes, input limits,
and the established stdout/write/flush/close failure oracle. A second consumer
imports the unchanged `stdlib/args.aeth` and `stdlib/string.aeth`, exercising
owned buffers, argv boundaries, grouped aliases and aggregate values. There is
no application reimplementation in Python or C; C is used only for the existing
fault-injection shim and native code compilation.

The runner separately records `jxz-capability.json`; an unsupported JXZ check
is not a passing native-JXZ claim. The current active JXZ graph uses features
still absent from Core, including generic Vec/Result types, payload enums,
match/error propagation, arrays, type aliases and intrinsics. Those are further
front-end/backend migrations; this checkpoint does not rename Core into the
full production compiler or mark full self-hosting complete.
