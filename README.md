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

## Current boundaries

The native acceptance platform is macOS on Apple Silicon. CI definitions also
cover Linux and Windows compiler builds; those jobs are not evidence of a run
until they actually execute. JXZ and the persistent service currently target
64-bit POSIX hosts.

Raw pointers and C FFI provide direct control and require manual lifetime/ABI
care. This release does not claim complete ownership enforcement, automatic
resource destruction, or a complete generic/trait/closure ecosystem. Unsupported
lowering must report an error instead of generating placeholder behavior. Much
of the older standard library remains experimental; the new workload and JXZ
interfaces are the exercised subset.

`src_aether/` contains an experimental compiler implementation. Its full entry
point does not currently build a working compiler, and its pipeline still
contains placeholder generation. AetherLang is **not currently self-hosting**.
Old standalone binaries and generated C files are historical artifacts, not
build inputs or acceptance evidence.

See [upgrade results and migration notes](docs/UPGRADE.md) and
[the current engineering handoff](HANDOFF.md). Older design documents describe
intent; they are not a claim that every described feature works.

Licensed under Apache-2.0; see [LICENSE](LICENSE).
