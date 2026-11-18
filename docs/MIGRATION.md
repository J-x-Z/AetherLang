# AetherLang Migration Guide

> **Purpose**: Guide for migrating existing C/Rust codebases to AetherLang

## C ABI callbacks defined in Aether (2026-11-15)

Matching `extern "C"` declarations can now resolve to one non-generic
`#[no_mangle]` Aether definition in the same or another loaded module. A pure
forwarding C trampoline is no longer required for this case; applications should
switch only after validating their chosen compiler snapshot. Signatures must
match after alias resolution, including variadic status; duplicate bodies still
fail. Generic functions cannot export one fixed `#[no_mangle]` symbol: write a
concrete non-generic wrapper that calls the generic implementation instead.
See [C ABI declaration rules](LANGUAGE.md).

## Literal escape correction (2026-11-15)

Unknown escapes now fail instead of silently discarding the backslash. Write
`"\\q"` for the literal two bytes backslash + q; similarly double backslashes
in Windows paths and regular expressions. The same rules apply to Core and
Script strings and to primary-compiler character literals.

`"\x1b"` now emits one ESC byte. Use `\xHH` for ASCII (00–7F), or
`\u{...}` for a Unicode scalar encoded as UTF-8. The standalone Core compiler
previously accepted `\x80..\xFF` as raw bytes; replace those with numeric `u8`
storage when binary bytes are intended, or Unicode escapes when text is intended.
The C backend now preserves NUL before digits: `"\07"` means bytes `00 37`,
matching LLVM and Core. Programs that accidentally relied on the previous
C octal-escape merging must be corrected rather than preserving that behavior.

See [literal semantics](LANGUAGE.md) section 1.4 and the
byte-level regression runner `tests/literal_escapes.py`.

---


## 1. C to AetherLang

### Basic Syntax

| C | AetherLang |
|---|------------|
| `int x = 5;` | `let x: i32 = 5;` |
| `int* p;` | `let p: *i32;` |
| `void f(int x)` | `fn f(x: i32)` |
| `#include <stdio.h>` | `extern "C" { fn printf(...); }` |

### FFI Interop

```aether
// Declare C functions
extern "C" {
    fn malloc(size: usize) -> *void;
    fn free(ptr: *void);
    fn printf(fmt: *i8, ...) -> i32;
}

// Call C functions
fn example() {
    unsafe { let p = malloc(1024); }
}
```

---

## 2. Rust to AetherLang

### Similarities
- Ownership system (`own`, `ref`, `mut`)
- Pattern matching (`match`)
- Explicit error handling (`Result`, `?` operator)

### Key Differences
| Rust | AetherLang |
|------|------------|
| `fn f() -> i32` | `fn f() -> i32` ✓ Same |
| `let mut x` | `let mut x` ✓ Same |
| `&x` | `&x` ✓ Same |
| Implicit deref | Explicit only (Strict Mode) |
| Trait bounds `T: Clone` | `where T: Clone` |
| `unsafe { }` | `unsafe(reason="...") { }` |

---

## 3. Adding Contracts

When migrating, add contracts to document invariants:

```aether
// Before (C-style)
fn divide(a: i64, b: i64) -> i64 { a / b }

// After (AetherLang with contracts)
fn divide(a: i64, b: i64) -> i64
    [requires b != 0, ensures result * b == a]
    pure
{
    a / b
}
```

---

## 4. Effect Annotations

Mark side effects explicitly:

```aether
// IO operations
fn log(msg: String) effect[io] { println(msg); }

// Pure computation
fn add(a: i64, b: i64) -> i64 pure { a + b }
```

---

## 5. Gradual Migration Strategy

1. **Start with FFI**: Wrap existing C libraries
2. **Add type annotations**: Make implicit types explicit
3. **Add contracts**: Document preconditions/postconditions
4. **Mark effects**: Annotate pure vs impure functions
5. **Migrate logic**: Rewrite hot paths in AetherLang

## Hosted builtin consistency

`print`, `println`, `print_i64`, `println_i64`, `alloc`, `malloc`, `free`,
`puts`, `atof`, `strcmp`, `exit`, and `assert` now use checked common IR on
both native backends. An explicit function or `extern "C"` declaration with
one of these names keeps its declared identity and ABI; it is not rewritten
as a language builtin. Only actually used host dependencies are declared.
Fixed libc names such as `malloc`, `strlen`, and `printf` must nevertheless
match the target C ABI in a hosted module, including exported definitions.
Freestanding `#![no_std]` modules may explicitly provide their own symbols.

Implicit `alloc` and `malloc` take the target-width unsigned integer: `u64`
on 64-bit targets (preserving existing host callers), `u32` on 32-bit
targets. Oversized integer arguments are rejected, not truncated. Explicit
libc declarations should use `usize` for C `size_t` parameters and results.
Implicit hosted calls remain unavailable in `#![no_std]`; provide an explicit
external declaration for a runtime the application supplies.

`assert(false)` now traps on both backends, matching failed language
contracts. The old C-only `Assertion failed` stderr message and `exit(1)`
behavior are no longer emitted. Use an explicit diagnostic and `exit(code)`
when an application requires a particular message or process status.
`atof` and `strcmp` carry the `read` effect because they inspect pointed-to
memory; wrappers must include it when declaring an explicit effect bound.

## Explicit affine resources and opt-in cleanup

`own Descriptor` now retains a distinct canonical type. A by-value assignment,
argument, or return moves the owner; later reads of the old binding fail with
`E2301`, including paths on which only one branch moved it. `&own Descriptor`
borrows without transferring ownership. Moving while a live reference still
uses the owner is rejected. A mutable slot may be reused after its previous
owner has been consumed on every incoming path.

Adopting a manual value requires an explicit `unsafe` conversion of the same
nominal payload: `unsafe { raw as own Descriptor }`. This is a transfer contract:
the caller must already own the raw resource uniquely and must not subsequently
use aliases retained by the old manual representation. Conversion back to
`Descriptor` likewise requires `unsafe` and consumes the affine value. Ordinary
raw pointers and ordinary `Descriptor`, `Vec`, `Tensor`, or `QuantumLease` APIs
retain their existing manual lifetime contracts.

`owned_fd::owned_fd_number(&descriptor)` borrows its numeric descriptor;
`owned_fd::owned_fd_close(descriptor)` consumes and explicitly closes it, even
when close reports an error. Without a registered destructor, abandoned affine values can still leak; close
successfully acquired owners explicitly or return them to another owner.

Register automatic cleanup with `@drop fn destroy(value: own Handle) { ... }`.
The function must be concrete, have exactly one owned parameter, return unit,
and have no contracts. A canonical payload type has at most one registered
destructor across the whole program, including aliases and imported modules.
Registration affects only `own Handle`, never ordinary `Handle` values or raw
pointers. The destructor's own parameter is exempt from recursive implicit drop.

Registered owners are destroyed in reverse construction order on normal block
exit, return, `?` error return, break, and continue. Moves transfer that duty;
explicitly consuming close operations cancel the old slot's cleanup. Return
values are evaluated before cleanup, and a returned owner is transferred to the
caller. Arguments are transferred only when the call executes: if evaluating a
later argument returns early, already evaluated owner temporaries are cleaned.
Discarded owner temporaries are destroyed at the full-expression boundary.
Destructor effects propagate through callers; a pure forwarding function that
returns its owner does not gain a cleanup effect.

There is no exception or process-exit unwinding: trap, `exit`, and nontermination
do not promise cleanup. Automatic destruction cannot report a close error;
use `owned_fd_close` explicitly when its captured error code matters. Fallible
operations should first borrow/prepare, then consume only on success. Containers
of owners and `Result<own T, E>` remain outside this stage. Mutating or mutably
borrowing an owner's raw payload requires `unsafe` because replacing a handle
can invalidate uniqueness; transferring the whole owner remains checked.

This first stage accepts named struct payloads without reference or owner fields.
Owner values inside arrays, tuples, records, enum payloads, globals, raw-pointer
storage, C ABI signatures, pattern matches, and contract snapshots are rejected
with `E2405` until their move and cleanup rules are implemented. References to
owners may be borrowed in ordinary language functions. The old `shared T`
placeholder is now rejected rather than being treated as an ordinary copy.

## Const generic arrays and records (2026-11-18)

Core now accepts integer/bool const parameters in functions, structs and enums:

```aether
struct Buffer<T, const N: usize> { values: [T; N] }
fn count<T, const N: usize>(values: [T; N]) -> usize { N }
const WIDTH: usize = 3;
fn main() {
    let buffer: Buffer<i64, WIDTH> = Buffer { values: [2, 4, 6] };
    let count: usize = count(buffer.values);
}
```

Arguments are typed constants. Use braces for expressions, such as
`Buffer<i64, {1 + 2}>`; identical evaluated arguments reuse one nominal instance.
Different const arguments remain different types, even when their fields happen
to have identical layouts. Array lengths must be nonnegative and representable
by the compilation target's `usize`. Const arithmetic uses the existing checked
constant rules: overflow and division by zero are diagnostics. Explicit casts
retain their normal conversion semantics. Ordinary runtime integer arithmetic
continues to wrap.

Function arguments and annotated result types infer parameters from direct
array dimensions and instantiated record/enum types. A dimension such as
`N + 1` is evaluated after another position supplies `N`; the compiler does not
solve arbitrary arithmetic equations. Function calls currently use inference,
not explicit generic call arguments. Generic type aliases remain type-parameter
aliases, and can refer to concrete const-generic layouts. Const parameters may
not be shadowed by parameters, local variables or pattern bindings. Templates
are specialized on demand; unused bodies are not a proof of correctness for all
possible instantiations.

Script uses the same Core specializer:

```python
def copy[const N: int](values: Array[int, N]) -> Array[int, N]:
    return values

def main():
    original = [2, 4, 6]
    copied = copy(original)
    copied[0] = 10
    return original[0] - 2
```

Script `Array[Element, Length]` and array literals are fixed-size values. Copying,
passing and returning them copies their elements; indexing and element assignment
lower to ordinary Core array operations. Nested fixed arrays are supported.
Lengths in Script hints are integer literals or declared const parameters;
empty literals without an element type and dynamic-list operations remain
unsupported. Script `int` is `i64`, including an `int` const parameter; use
`usize` explicitly when target-width arithmetic is desired. Captured closures,
dynamic trait dispatch and async are separate T25 tasks, not part of this change.

### Array repeat evaluation

`[value; count]` now evaluates `value` exactly once, including when `count` is
zero. The old parser duplicated the expression `count` times; code relying on
those repeated side effects must use an explicit loop. The count accepts the
same checked constants and const parameters as array lengths. Generic array
factories can return `[value; N]` with `N` inferred from the annotated result.

Runtime repeats produce a compact fill loop in both backends. Static repeats
retain a compact checked initializer; zero initialization uses zero-filled
storage. Nonzero static data still requires output proportional to its size.
Repeating shared references preserves lifetime checking. More than one copy of
an exclusive reference, including a reference inside a record or enum, is
rejected; affine owners remain excluded from array storage. Script exposes this
operation as `repeat(value, count)`, with the same evaluation and copy semantics.

### Captured closures (Core)

Typed `|x: i64| expression` and `|| expression` now produce concrete, inline environment values. Local captures are evaluated once and copied or moved into fields; a captured ordinary scalar is a snapshot, not an implicit reference to the original variable. Capture an explicit `&T` or `&mut T` variable when borrowing is intended. Global names are looked up at invocation.

Use `let f: _ = |x: i64| base + x;` for the unnamed concrete type, and generic parameters such as `fn apply<F>(f: &F, x: i64) -> i64 { f(x) }` for callbacks. `_` is only inferred for closure-containing values; this does not enable general Core type inference. Closures can be copied when their fields can be copied, passed and returned through generics, stored in generic records, and returned by other closures. Captured exclusive references retain their move and lifetime rules. Owned resources in environments remain rejected until aggregate ownership is supported.

A closure that changes its captured state needs a mutable binding and is invoked through an exclusive environment borrow. Read-only calls borrow the environment immutably. Existing reference and effect checking applies to generated bodies, including indirect environment reads and writes; hidden IO or escaping local references are rejected. Parameters remain explicitly typed. Add a return annotation when the body cannot determine its result (for example, an explicit `return` without a tail value).

The concrete environment has no implicit heap allocation and no C ABI function-pointer conversion. Use generic callbacks for these closures; function-pointer annotations still receive a diagnostic. Named functions returning an unnamed closure must expose an appropriate concrete/generic interface; opaque return syntax is not introduced by this change.

Script exposes the same values as `lambda (x: int) -> int: base + x` (the return annotation is optional). `Callable[int,int]` means one integer argument and an integer result; the last type is the result, so `Callable[int]` takes no arguments. A named Script function's `Callable` parameter becomes a Core generic parameter, and a matching return annotation can preserve the one corresponding callback parameter's concrete type. Closure copies, nested lambdas and fixed arrays use the same Core representation. The first binding of an anonymous closure value must occur at function scope, outside branches/loops; reassignment must retain the same concrete environment type. These restrictions preserve Script's existing definite-assignment and function-scoped local rules without introducing dynamic closures or uninitialized anonymous types.
