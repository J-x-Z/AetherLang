# JXZ local project driver

Local development version: `0.3.0-dev.2` (local path dependencies and transaction recovery).

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
# Additional search roots, in order; relative to this project directory.
module_paths = ["src", "build/generated", "/external/pinned-stdlib"]
# Existing native objects/archives. JXZ does not compile these files.
link_objects = [
    "build/native/platform.o",
    "build/native/ui.o", # comments and trailing commas are supported
    "build/native/events.o",
]
```

The manifest reader supports quoted basic/literal strings, whitespace, comments,
and the fields above. `module_paths` and `link_objects` are optional arrays of
nonempty quoted path strings, defaulting to `[]`. Arrays may span lines and use
comments and a trailing comma; nested arrays, non-string entries, duplicate keys,
control characters and trailing garbage are rejected before the compiler runs.
The existing basic-string escapes (`\\`, `\"`, `\n`, `\r`, `\t`) are decoded;
control characters are then rejected in paths. Raw UTF-8 is preserved; TOML
Unicode escape sequences and multiline strings are not supported.

`build`, `run` and every `test` compilation forward these arrays as repeated
`-I PATH` and `--link-object PATH` arguments, preserving each array's order.
Paths are relative to the directory containing the current `Jxz.toml`, or may be
absolute. Spaces, Unicode, quotes and shell metacharacters remain literal; no
glob, environment or shell expansion occurs. A relative path beginning with `-` or `@`
is prefixed with `./` so neither the compiler nor linker interprets it as an
option or response file. `src` still selects the default main file; add it to `module_paths` when
other entry points (including tests) need that search root. Native files must
already exist, match the target, and expose the declared ABI. Their build policy
remains with the project; this setting does not introduce arbitrary build hooks.
`build` and the build phase of `run` preserve compiler exit codes. `test` retains
its existing aggregate status: 1 if any compilation or test program fails.

### Local path dependencies

```toml
[dependencies]
geometry = { path = "../geometry" }
```

The dependency key must match the library's `[package] name`. Its entry is
`<build.src>/lib.aeth` (default `src/lib.aeth`). Paths are relative to the
declaring package. Each canonical package is loaded once, including a library
reached through both sides of a diamond. Cycles, different packages with the
same name, and conflicting local module names are errors. Registry versions,
dependency aliases and target-specific dependency tables are unsupported.

Build, run and test pass exact library entries to the compiler. Dependencies
can provide module search directories and native link inputs relative to their
own project. Search directories apply to the compilation unit. The sorted
`target/.jxz-dependencies` record describes the most recently resolved local
graph. Compilation reads the actual sources on every build. Clean removes the
project's owned outputs. Package names are single filenames of at most 255
bytes. JXZ reserves the `.jxz-` prefix for its records.

Tests are `tests/**/test_*.aeth` programs; zero means pass, compile/link/runtime
failure means fail. Paths are discovered as NUL-delimited records, so spaces and
newlines are preserved. Test binaries are rebuilt before execution. `clean`
removes every output recorded by successful JXZ builds and reserved intermediates;
it never recursively deletes the target directory or removes source `.c` files.

This implementation currently requires a 64-bit POSIX host with `find` and the
linker required by `aethc`. macOS is exercised by the integration suite; Windows is not supported
by this POSIX process layer. Online registry and dependency-edit commands return an explicit
error; the older experimental source files remain for later development.

Migration: `build` now defaults to debug (`-O0`); pass `--release` for `-O2`.
`run` forwards arguments, `test` reports failures, and `clean` preserves unrelated
files. A successful build atomically updates the deduplicated `.jxz-outputs` collection
in the target directory, so cleanup retains ownership across package renames.
A failed metadata write preserves the previous collection.
Cleanup also reads the previous `.jxz-output-name` format, rejects corrupt or
incomplete records, and preserves unrelated files. Path and process operations
are provided by `stdlib/path.aeth` and `stdlib/process.aeth`.
