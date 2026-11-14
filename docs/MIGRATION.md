# AetherLang Migration Guide

> **Purpose**: Guide for migrating existing C/Rust codebases to AetherLang

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
