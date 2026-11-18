# JXZ local project driver

Local development version: `0.3.0-dev.3` (project-aware LSP and exact-version registry).

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

### Machine-readable project analysis

Run `jxz check [file]` or `jxz symbols [file]` in the manifest directory. Both
select the build entry by default, resolve the same locked modules as the
language server and skip native link inputs. A successful check is silent.
Symbols are JSON on stdout. Compiler diagnostics are JSON on stderr with exit
status 1. Project configuration errors are reported on stderr. For paged
source queries from an MCP client, see the [local server](../docs/MCP.md).

### Editor language server

Run `jxz lsp` with the directory containing `Jxz.toml` as the working directory.
Configure the editor's server command as the absolute JXZ executable, with
`["lsp"]` as its arguments; `AETHC` selects the compiler as for builds.
The command loads the same manifest and local dependency graph, forwarding
`module_paths` and the exact package entries to `aethc lsp`. Cross-package
definitions and unsaved documents therefore use the project's module mapping.
Restart the server after changing manifest paths or dependency declarations.

Startup neither builds sources nor creates the target directory, locks, or
dependency records. Standard output contains only LSP messages; configuration
and dependency errors go to standard error. Native link flags are not passed to
the language server, and dependency native objects need not exist yet; their
manifest fields still undergo normal syntax validation. JXZ replaces itself with the server process, preserving
its PID, exit status and signal handling. `jxz lsp` accepts no extra arguments.

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
same name, and conflicting local module names are errors. Dependency aliases
and target-specific dependency tables are unsupported.

Build, run, test and LSP pass exact library entries to the compiler. Dependencies
can provide module search directories and native link inputs relative to their
own project. Search directories apply to the compilation unit. The sorted
`target/.jxz-dependencies` record describes the most recently resolved local
graph. Compilation reads the actual sources on every build. Clean removes the
project's owned outputs. Package names are single filenames of at most 255
bytes. JXZ reserves the `.jxz-` prefix for its records.

### Exact-version HTTP(S) packages

```toml
[dependencies]
geometry = "1.2.3"
vectors = { version = '2.0.0' }
# Local path dependencies can coexist with registry dependencies.
local_helpers = { path = '../local_helpers' }
```

Set `JXZ_REGISTRY=https://example.org/aether` (no trailing slash) for first
resolution. Set `JXZ_CACHE=/external/build/jxz-cache` to choose cache storage;
the default is `$HOME/.cache/jxz`. `jxz fetch` resolves and verifies the complete
graph without compiling. Build, run and test fetch missing packages through the
same graph. Versions are exactly three unsigned decimal components `x.y.z`,
without leading zeros. Ranges, prereleases and build metadata are rejected.
One package name has one version across the complete graph; conflicts are errors.

The first successful resolution writes sorted `Jxz.lock` pins containing each
name, version, package-manifest SHA-256 and registry origin. Check this file into
the project. Normal commands retain existing pins even if the registry publishes
new bytes for that version. A changed version/origin requires `jxz update`, which
explicitly reselects pins for the versions currently requested by `Jxz.toml`.
It reuses pinned origins unless `JXZ_REGISTRY` explicitly selects a new origin.
Update does not edit the manifest or publish anything. A failed graph leaves the
old lock intact; unrecognized, symbolic or concurrently changed lock files are
preserved. New dependencies may add pins; successful resolution drops unused pins.

`JXZ_OFFLINE=1` forbids downloads and requires matching pins plus intact cache
contents. Every cache hit verifies the manifest, every file hash and the complete
file set. Corrupt files, additional files and symlinks are rejected. Online repair
preserves corrupt entries in `.jxz-corrupt-*` directories before downloading a
verified replacement. Downloads use isolated `.jxz-fetch-*` directories; only
complete verified packages are atomically published. Process interruption cannot
publish partial packages or locks. Retrying fetch recovers; interrupted staging
directories and quarantines are retained for inspection, with no automatic cache
collector. `clean` preserves the cache and `Jxz.lock`.

LSP resolution only reads existing pins and verified cache entries. It neither
fetches nor updates the lock; run `jxz fetch` before opening an uncached project.
Registry packages require matching `[package] name/version`, relative source,
module and native-object paths without dot/up-level components, and exact-version
registry dependencies. A downloaded package cannot introduce local path
dependencies outside its verified file set.

A registry is an ordinary static HTTP(S) directory. It needs no executable server
API. All files below are UTF-8, tab-separated, LF-terminated, without blank rows:

```text
# index.jxz (the explanatory comment is not part of the file):
JXZ-REGISTRY-1
geometry<TAB>1.2.3<TAB><sha256 of geometry/1.2.3/files.jxz>

# geometry/1.2.3/files.jxz:
JXZ-PACKAGE-1
<sha256><TAB>Jxz.toml
<sha256><TAB>src/lib.aeth
```

Each listed file is served at `<name>/<version>/<relative-path>`; JXZ percent-
encodes spaces and Unicode in URL paths. Files may be binary. The package list
must include `Jxz.toml`. Absolute paths, empty/dot/up-level components, backslashes,
control bytes, duplicate paths and root `.jxz-*` metadata names are rejected.
There is no archive extraction. Index and file-list limits are 1 MiB/4096 rows;
files are at most 16 MiB each and packages at most 64 MiB in total. JXZ requires `curl` 8.4 or newer (older versions did not bound downloads
without Content-Length), with only HTTP(S), no redirect following, bounded time/size and default HTTPS
certificate verification; Aether streams SHA-256 over bounded file reads and `find` enumerates
files. Selection, validation, locks, cache policy and graph handling are Aether.
Initial resolution trusts the configured registry; lock hashes pin later reads.

Real loopback HTTP validation (including interruption and offline rebuild):

```sh
python3 jxz/tests/registry.py --jxz /external/build/jxz \
  --aethc /absolute/path/to/aethc --backend llvm \
  --work-dir /external/build/jxz-registry-tests
```

Tests are `tests/**/test_*.aeth` programs; zero means pass, compile/link/runtime
failure means fail. Paths are discovered as NUL-delimited records, so spaces and
newlines are preserved. Test binaries are rebuilt before execution. `clean`
removes every output recorded by successful JXZ builds and reserved intermediates;
it never recursively deletes the target directory or removes source `.c` files.

The default host implementation requires a 64-bit POSIX host with `find` and
the linker required by `aethc`. macOS is exercised by the integration suite.
The separate Windows x64 adapter passed native project and registry-cache tests
on Windows 11. Online registry commands additionally require `curl`. The dependency-edit/install commands `add`, `remove` and `install`
remain unsupported and return an explicit error; remote publication is not implemented.

Migration: `build` now defaults to debug (`-O0`); pass `--release` for `-O2`.
`run` forwards arguments, `test` reports failures, and `clean` preserves unrelated
files. A successful build atomically updates the deduplicated `.jxz-outputs` collection
in the target directory, so cleanup retains ownership across package renames.
A failed metadata write preserves the previous collection.
Cleanup also reads the previous `.jxz-output-name` format, rejects corrupt or
incomplete records, and preserves unrelated files. Path and process operations
are provided by `stdlib/path.aeth` and `stdlib/process.aeth`.

### Windows x64 build and acceptance

The native Windows adapter is Aether code using Win32/UCRT. Project parsing,
package verification and build publication share the POSIX implementation.
Build preparation (does not execute Windows binaries):

```sh
python3 scripts/bootstrap_jxz_windows.py --compiler /path/to/native/aethc --work-dir /external/windows-jxz
```

The runner freezes sources and emits C/LLVM at O0/O2, links real x64 PE files
with Zig, and checks imports and the SDK enumeration layout. Select
`host_windows.aeth` as `host.aeth` only in that frozen build; the normal source
checkout uses POSIX. `windows_api.aeth` and command quoting are shared with the
self-hosted compiler. No Wine or Windows host is contacted.

On an actual Windows host with native Clang/SDK configured:

```powershell
python jxz/tests/windows_native.py --jxz C:\bundle\jxz.exe --compiler C:\bundle\aethc.exe --cc C:\LLVM\bin\clang.exe --stdlib C:\bundle\stdlib --host-source C:\bundle\source\jxz --work-dir C:\aether-tests
```

That entry validates local/transitive dependencies, changes and deterministic
records, test discovery, output preservation, inherited directory locks,
verified offline cache, and the project LSP. On other hosts it exits **77** with
an explicit skip. Cross-linking is not native runtime acceptance.

Windows uses UTF-8 project paths with wide native APIs and `.exe` output names.
Test discovery and cache verification do not require `find`, `tar`, a shell,
or symlink privileges. Online fetch requires **curl 8.4+**. Windows curl
8.22.0 fetched a three-package graph from a loopback registry with UTF-8 paths.
After the server stopped, both backends rebuilt from the verified cache.
Ownership markers are regular files published atomically through a hard link.
Killing a writer before marker publication can leave a random `.jxz-marker-*`
sibling in the parent directory; it is preserved as an uncommitted artifact,
never treated as ownership or recursively deleted. A filesystem without hard
links fails that operation without replacing existing files.

JXZ acquires the Windows lock by exclusively creating
`.jxz-directory-lock`. Windows removes it when the last owning or inherited
handle closes. A pre-existing file with that name makes the operation fail
and retains its contents. The native suite kills the parent, observes the
child still holding the lock, and builds again after the child exits.
