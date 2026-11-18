# AetherLang

AetherLang compiles typed programs to native executables through LLVM or C.
Use `.ath` for indentation-based Script and `.aeth` for explicit Core. Core
provides control over memory layout, ownership and C interfaces. JXZ builds
multi-package projects, runs programs and discovers tests.

## Build and run

On macOS or Linux, install the pinned Rust toolchain and Clang. The Rust LLVM
backend defaults to LLVM 21.1. Set `LLVM_SYS_211_PREFIX` if needed. `CARGO_TARGET_DIR`
and `TMPDIR` can point to an external volume.

```sh
sh scripts/build.sh llvm
build="${CARGO_TARGET_DIR:-target}"
seed="$build/release/aethc"
python3 scripts/build_selfhost.py --seed "$seed" --work-dir "$build/selfhost" --output "$build/aethc-native"

"$build/aethc-native" build tests/regressions/script/compute.ath --sysroot . -O2 -o "$build/squares"
"$build/squares"
# 338350
# 25
```

The self-hosted compiler is written in Aether. This build checks that two
consecutive generations emit byte-identical C and byte-identical LLVM IR for
the full compiler entry. Clang compiles or assembles the output and the host
linker produces the executable. The Rust compiler also supplies cross-target,
inline assembly and vector facilities. The self-hosted compiler supports typed
atomics, memory fences and volatile access. Build a C-only seed with
`sh scripts/build.sh c`. [Capabilities](docs/FEATURES.md) · [Bootstrap](docs/BOOTSTRAP.md)

To opt into LLVM 23.1 for the Rust seed, run `sh scripts/build.sh llvm23`.
Use a separate `CARGO_TARGET_DIR` to keep both executables. Set
`LLVM_SYS_231_PREFIX` when autodetection needs a path. Use Clang 23 to read
textual IR emitted by that seed. The stable packages use LLVM 21.
[Measurements and compatibility](docs/UPGRADE.md#llvm-21-与-23-的迁移决定)

## Work with data in the terminal

The streaming regression example reads a file or stdin, selects two named
columns and writes a JSON record to stdout.

```sh
"$build/aethc-native" build examples/analysis/regress.aeth --sysroot . -O2 -o "$build/regress"
printf 'time,value\n1,3\n2,5\n3,7\n' | "$build/regress" - time value
```

This gives three complete pairs, sample covariance 2, slope 2, intercept 1 and
R² within floating-point rounding of 1. The program handles quoted fields, embedded newlines and missing pairs.
Malformed records and a constant predictor return an error. An independent
test checks the floating-point results using exact rational arithmetic. Both
the parser and statistics routines can be imported into other programs.
[Analysis source](examples/analysis/regress.aeth) · [Tests](tests/terminal_stats.py)

For MATLAB-style work, the library includes dense tensors, matrix multiplication,
views, matrix-chain planning and low-rank decomposition. Reverse-mode
differentiation, finite-difference checks and a gradient-descent workload are
available. For R- and Python-style batch jobs, native programs handle streaming
data, statistics, files, subprocesses and exit status. Interactive notebooks,
a general statistical-model library, dataframe interoperability and plotting
remain gaps for users of those ecosystems. [Numerical API](docs/MATH.md) ·
[Computational workloads](tests/regressions/compute)

## Systems, quantum simulation and project tools

- **Systems.** Fixed-size values, raw pointers, checked references, `own T`
  with typed destructors, packed structs, unions and C linkage are exercised in
  native programs. The self-hosted LLVM backend can emit x86-64 freestanding
  objects. An Aether page pool boots under QEMU at O0 and O2.
  Foreign aggregate ABI coverage includes Apple arm64 and
  x86-64 System V. The [persistent accumulator](examples/upgrade/accumulator.aeth)
  reads commands, writes a journal and closes its resources.
- **Accelerators.** Aether code owns tensor shapes, budgets, operator scheduling
  and resource lifetimes. Compute providers have run on CPU, Apple Metal and
  Vulkan on NVIDIA and Adreno devices. Callers select a device and receive
  errors for unsupported operations or allocation failures.
- **Quantum simulation.** An fp64 state-vector reference and an fp32 simulator
  using the compute queue run Aether circuits. The separate CUDA-Q SDK bridge
  supports QPP and NVIDIA environments and exports validated circuits as Quake MLIR.
  Reusable Aether circuit templates bind shared rotation parameters before execution. Metal and Adreno use Aether's own
  simulator. A 16-qubit workload checks all 65,536 amplitudes, expectations
  and a parameter-shift gradient. [Quantum API and device results](docs/QUANTUM.md)
- **Project tools.** JXZ checks, builds, runs and tests projects with local or
  locked registry dependencies and preserves child failures. The compiler has
  JSON diagnostics, a symbol index and a stdio language server for unsaved
  buffers and imports. [JXZ usage](jxz/README.md)

## Reproduce and install

```sh
sh scripts/test.sh llvm
python3 scripts/package.py --work-dir "$build/package-evidence" --output "$build/local-release"
sh install.sh --package "$build/local-release" --prefix "$HOME/.aetherlang" --manifest-sha256 "$MANIFEST_SHA256"
```

Set `MANIFEST_SHA256` to the hash printed by the package command. A release
contains `aethc` (Rust seed), `aethc-native` (Aether), their JXZ executables and
the standard library. On Linux, `--bundle-linux-llvm` includes the seed's LLVM
library. The installer verifies file checksums and switches versions atomically.
macOS arm64 and Linux x86-64 packages have run on their respective hosts.
The Windows x86-64 bundle runs on Windows 11 with llvm-mingw UCRT Clang.
Its PowerShell entry verifies the files, compiles another generation of the
compiler with both backends, and exercises JXZ, the language server and CSV
statistics. From the extracted bundle run:

```powershell
.\Test-Native.ps1 -Clang 'C:\Tools\llvm-mingw\bin\clang.exe' -Python python -WorkDir '.\native-results'
```

The separate Windows Vulkan package ran the 16-qubit workload on an RTX 4060
Laptop GPU. [Upgrade results](docs/UPGRADE.md) ·
[Engineering handoff](HANDOFF.md)

AetherLang is distributed under the [Apache License 2.0](LICENSE).
