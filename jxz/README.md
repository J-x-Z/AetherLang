# JXZ local project driver

JXZ's active implementation is `src/main.aeth` and its imported Aether modules.
Build it with the upgraded Rust compiler (historical checked-in `.c` files are
not build inputs):

```sh
AETHC=/absolute/path/to/aethc ./jxz/build.sh /external/build/jxz
python3 jxz/tests/integration.py --jxz /external/build/jxz \
  --aethc /absolute/path/to/aethc --work-dir /external/build/jxz-tests
# Optional backend comparison:
AETHER_BACKEND=c AETHC=/absolute/path/to/aethc ./jxz/build.sh /external/build/jxz-c
python3 jxz/tests/integration.py --jxz /external/build/jxz-c \
  --aethc /absolute/path/to/aethc --backend c --work-dir /external/build/jxz-tests-c
```

`jxz init [name]` creates a project in the current directory without overwriting
sources. `build [--release]`, `run [-- args...]`, `test`, and `clean` operate on
its `Jxz.toml`. JXZ invokes the normal `aethc build` entry point, inheriting the
compiler's default backend (LLVM in a build with LLVM enabled). `AETHER_BACKEND`
can select `llvm` or `c` explicitly. `AETHC` selects the compiler executable and
`CC` selects its linker/C compiler; neither accepts shell text or flag strings.
Compiler diagnostics and nonzero exit codes are preserved. `run` returns the program's exit status (or 128 + terminating signal).

```toml
[package]
name = "example"
version = "0.1.0"

[build]
src = "src"
target = "target"
# If omitted, main defaults to <src>/main.aeth.
main = "src/application.aeth"
```

The manifest reader supports quoted basic/literal strings, whitespace, comments,
and the fields above. It is a deliberately limited project schema, not a full
TOML implementation. Known duplicate keys, invalid paths, malformed quoted
values, unknown tables/keys and dependency declarations fail. The only accepted
tables are `[package]`, `[build]`, and an empty `[dependencies]`; dotted, quoted,
inline, target-specific and array dependency tables are explicitly unsupported. Package names must be a single
filename (at most 255 bytes) and may not start with the reserved `.jxz-` prefix.

Tests are `tests/**/test_*.aeth` programs; zero means pass, compile/link/runtime
failure means fail. Paths are discovered as NUL-delimited records, so spaces and
newlines are preserved. Test binaries are rebuilt before execution. `clean`
removes every output recorded by successful JXZ builds and reserved intermediates;
it never recursively deletes the target directory or removes source `.c` files.

This implementation currently requires a 64-bit POSIX host with `find` and the
linker required by `aethc`. macOS is exercised by the integration suite; Windows is not supported
by this POSIX process layer. Registry and dependency commands return an explicit
error; the older experimental source files remain for later development.

Migration: `build` now defaults to debug (`-O0`); pass `--release` for `-O2`.
`run` forwards arguments, `test` reports failures, and `clean` preserves unrelated
files. A successful build atomically updates the deduplicated `.jxz-outputs` collection
in the target directory, so cleanup retains ownership across package renames.
A failed metadata write preserves the previous collection.
Cleanup also reads the previous `.jxz-output-name` format, rejects corrupt or
incomplete records, and preserves unrelated files. Path and process operations
are provided by `stdlib/path.aeth` and `stdlib/process.aeth`.
