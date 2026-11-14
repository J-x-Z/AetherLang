# AetherLang

AetherLang is an independent language project with two source forms: `.ath`
prioritizes human readability; `.aeth` uses explicit types and exposes pointers,
layout and foreign interfaces. Both compile through the same checked pipeline.
The maintained compiler is written in Rust. LLVM is the primary native backend;
a C backend provides a portable fallback and an independent semantic comparison.

## Build and run

Use Rust 1.100.0, a native C compiler/linker, and LLVM 21 for the primary backend.
Set `LLVM_SYS_211_PREFIX` if LLVM 21 is not discoverable. The build script detects
Homebrew LLVM 21. LLVM builds prefer the shared library, which must remain
available when running `aethc`.

```sh
scripts/build.sh llvm
"${CARGO_TARGET_DIR:-target}/release/aethc" build examples/upgrade/compute.aeth -O2 -o ./compute
./compute

# C-only fallback, without an LLVM dependency:
scripts/build.sh c
"${CARGO_TARGET_DIR:-target}/release/aethc" build examples/upgrade/compute.aeth --backend c -O2 -o ./compute
```

`CARGO_TARGET_DIR` and `TMPDIR` can point to an external work volume. The scripts
reuse installed toolchains and caches and default to three Cargo build jobs.
The Cargo lockfile and Rust version are committed.

```aether
// example.aeth
extern "C" { fn printf(format: *u8, ...) -> i32; }
fn main() -> i32 {
    let mut sum: i64 = 0;
    for i in 0..100 {
        if i == 50 { continue; }
        sum = sum + i;
    }
    printf("%lld\n\0" as *u8, sum);
    return 0;
}
```

```python
# example.ath
def sum_to(n: int) -> int:
    total = 0
    for i in range(n):
        total = total + i
    return total

def main():
    print(sum_to(100))
    return 0
```

Imports resolve relative to the importing source file, followed by `-I` module
paths, `AETHER_PATH`, and the bundled standard-library source directory. Imported
bodies are compiled once; public imports, aliases and private helpers use one
resolution stage for checking and both backends. Native language definitions
have private linker names; `main` and explicit `#[no_mangle]` functions retain
their ABI names.

```sh
aethc check example.ath
aethc build example.ath -O2 -o example
aethc build example.aeth --backend llvm --emit-obj -o example.o
aethc example.aeth --emit-c -o example.c
aethc example.aeth --emit-ir -o example.ir
aethc example.aeth --emit-llvm -O2 -o example.ll
aethc check example.ath --error-format json
aethc symbols example.aeth
aethc lsp  # stdio JSON-RPC for editors
```

`check` performs parsing, module resolution and semantic checks. Building also
validates the supported lowering and target code generation. `--emit-obj` is the
explicit object-only mode; `build` links an executable. `--cc` or `CC` selects one
compiler/linker executable. Compiler subprocess failures retain their actual
diagnostics and exit status.

## Verified workloads and tests

```sh
scripts/test.sh llvm  # semantic/CLI regressions, both backends, real workloads
scripts/test.sh c     # equivalent supported C-only checks
```

The native suite compares LLVM and C at O0/O2/O3 using semantic assertions,
including short circuiting, loop exits, typed pointer indexing, aliases,
structure copies/returns, scalar numeric behavior, foreign calls and strings.
The real workloads include:

- `examples/upgrade/compute.aeth`: heap sort, binary search, deterministic PRNG,
  integer reduction and floating-point computation over allocated storage.
- `examples/upgrade/accumulator.aeth`: a persistent input service with bounded
  integer parsing, command errors, state, journal writes and explicit cleanup.
- `tests/regressions/script/`: `.ath` arithmetic, loops, functions, branching,
  printing and rejection of invalid or uninitialized programs.
- JXZ: real project initialization, configuration, building, running with argv,
  test aggregation and cleanup. See [JXZ usage and limits](jxz/README.md).

## Values, resources and system interfaces

Fixed arrays and structures are values: copies do not alias and returning them
preserves their storage. The compiler checks definite initialization and tracks
reference origins, borrows and escapes across branches, calls and aggregate fields.
Raw pointers remain available for explicit low-level memory and FFI operations.

Type parameters are specialized into concrete functions, structures and enums.
`Vec<T>`, `Option<T>`, `Result<T,E>`, payload matching and `?` have native runtime
tests, including zero-sized values and failed allocation or file operations.
Use the free-function APIs in `stdlib/vec.aeth`, `option.aeth`, and `result.aeth`;
see [complete running programs](tests/regressions/generics).

The exercised library set covers strings, byte I/O, files, arguments, paths,
POSIX processes, vectors and result values. Files and allocations are explicitly
closed/freed, including error paths. Static storage, unions, `size_of(Type)`,
`align_of(Type)` and `offset_of(Type, field)` use actual target layout. Union
reads require `unsafe`. Volatile accesses and atomic operations reach native
instructions in both backends; atomic order arguments are checked constants.
Default atomics are sequentially consistent. Raw atomic pointers must address
live, naturally aligned storage with compatible concurrent access.

`stdlib/mmio.aeth` provides portable volatile accesses and fences. Privileged
x86 port instructions are isolated in `stdlib/x86io.aeth`; compiling an object
for x86 does not mean those instructions can run in an ordinary user process.

The editor server shares compiler diagnostics and symbols, supports unsaved
buffers/imports, completion, hover, definition and references. JSON diagnostics
include stable category codes and original UTF-8 source spans; LSP positions use
UTF-16. Script diagnostics map back to the `.ath` input.

## Current boundaries

The native acceptance platform is macOS on Apple Silicon. CI definitions also
cover Linux and Windows compiler builds; those jobs are not evidence of a run
until they actually execute. JXZ and the persistent service currently target
64-bit POSIX hosts.

Raw pointers and C FFI provide direct control and require manual lifetime/ABI
care. This release does not claim complete ownership enforcement, automatic
resource destruction, or a trait/closure ecosystem. Unsupported
lowering must report an error instead of generating placeholder behavior. Const generics, generic impl methods, traits, closures and async are not
implemented. Older library modules outside the exercised set remain experimental.
The LLVM foreign aggregate ABI classifier is currently verified for Apple arm64;
other targets reject those foreign aggregate calls explicitly. Internal LLVM
aggregate calls and the C fallback do not share that foreign-ABI restriction.

`src_aether/core/` now contains a complete minimal self-hosting compiler written
in Aether. It compiles its own lexer, parser, type checker and C emitter through
four native stages with identical generated C. It intentionally implements a
specified Core subset; the Rust/LLVM compiler remains the full language compiler.
See [the reproducible bootstrap chain and subset](docs/BOOTSTRAP.md),
[the tensor/autodiff API](docs/MATH.md), and [native quantum integration](docs/QUANTUM.md).

See [upgrade results and migration notes](docs/UPGRADE.md) and
[the current engineering handoff](HANDOFF.md). Older design documents describe
intent; they are not a claim that every described feature works.

Licensed under Apache-2.0; see [LICENSE](LICENSE).
