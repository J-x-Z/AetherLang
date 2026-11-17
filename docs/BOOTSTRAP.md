# Self-hosting Core compiler

`src_aether/core/` is a complete compiler written in Aether Core: lexer,
recursive-descent/Pratt parser, name and type checker, and typed C11 emission.
It compiles all of its own source. The Rust compiler remains the full language
compiler, with LLVM as the primary backend and C as the portable alternative.
Core does not implement the full Rust compiler's language yet.

Run the entire chain with an existing freshly built compiler:

```sh
python3 tests/bootstrap_core.py --compiler /path/to/aethc \
  --work-dir /external/work/bootstrap --backend llvm
```

Use `--backend c` for a C-only seed. The runner concatenates the seven source
modules verbatim, builds stage1 from stage0, then uses the Aether compiler to
compile the same complete source into stages2, 3 and 4. Clang compiles generated
C into machine code. Stages1 onward never invoke the Rust compiler; the runner
places a failing `aethc` command in PATH to enforce that boundary. Every generated
C file must be byte-identical; the self-compiled compiler must also compile
and execute real workloads.

Core v1 supports functions, C FFI and variadics, explicit scalar/pointer/struct
types, constants, lexical locals, casts, fields, indexing, `size_of`, arithmetic,
conditionals, loops, returns, and raw-address operations. All locals require an
initializer. Non-void functions must explicitly return on every reachable path.
Only `main()` or `main(argc:i32, argv:**u8)` is an entry point, returning void/i32.
Struct values can be copied and returned; recursive layouts require pointers.
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

The bootstrap compiler rejects imports, generics, enums, methods, arrays,
references, attributes and other syntax outside this subset. Its own modules
use a deterministic concatenation manifest instead of an import resolver.
These features remain available in the primary compiler. This is a real minimal
self-host, not a replacement of the full compiler or an assertion that LLVM code
generation itself is self-hosted.

The compiler owns an arena released on success and failure, limits input to
32 MiB and arena allocations to 512 MiB, and checks syntactic and actual AST
depth. Output is written to a temporary sibling and atomically renamed; failed
compilation leaves an existing output intact. Diagnostics include source line
and column. Handles and raw pointers in compiled programs have explicit
lifetimes; this subset does not add an automatic destructor system.

Validation includes recursive heap structures, NUL-containing files, call and
assignment order, numeric boundaries, malformed inputs, and ASan/UBSan builds
of the compiler compiling itself. Results contain stage hashes, sizes, commands
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
