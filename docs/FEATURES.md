# AetherLang Feature Cheat Sheet

> **Complete feature summary for developers**

## 🎯 Type System

| Category | Feature | Syntax Example |
|----------|---------|----------------|
| **Primitives** | Integers | `i8, i16, i32, i64, u8, u16, u32, u64` |
| | Floats | `f32, f64` |
| | Boolean | `bool` (true/false) |
| | Character | `char` |
| **Pointers** | Raw Pointer | `*T`, `*mut T` |
| | Reference | `&T`, `&mut T` |
| | Lifetime | `&'a T`, `&'static T` |
| **Composite** | Struct | `struct Point { x: i64, y: i64 }` |
| | Enum | `enum Option<T> { Some(T), None }` |
| | Tuple | `(i32, i64, bool)` |
| | Array | `[i32; 10]` |
| **Generics** | Type Params | `fn identity<T>(x: T) -> T` |
| | Struct | `struct Vec<T> { data: *T, len: u64 }` |
| **Aliases** | Type Alias | `type Int = i64;` |

## 🔗 Module System

| Feature | Syntax | Description |
|---------|--------|-------------|
| **Import Single** | `use foo::Bar` | Import one item |
| **Import Multiple** | `use foo::{A, B, C}` | Import several items |
| **Import All** | `use foo::*` | Import all public items |
| **Public Export** | `pub struct X {}` | Make item visible to other modules |
| **Search Paths** | `.`, `src_aether/`, `stdlib/` | Automatic module discovery |

## 🎭 Traits & Interfaces

```rust
trait Display {
    fn display(&self);
}

impl Display for Point {
    fn display(&self) {
        // ...
    }
}
```

## 🔒 Ownership & Safety

| Feature | Syntax | Description |
|---------|--------|-------------|
| **Ownership** | `own T` | Exclusive ownership |
| **Shared Ref** | `&T` | Immutable borrow |
| **Mutable Ref** | `&mut T` | Mutable borrow |
| **Lifetime** | `&'a T` | Explicit lifetime annotation |

## ⚡ Effects & Contracts

| Feature | Syntax | Description |
|---------|--------|-------------|
| **Pure Function** | `pure fn add(a: i32, b: i32) -> i32` | No side effects |
| **Effect Annotation** | `effect[io] fn log(msg: &str)` | Declares IO effects |
| **Precondition** | `requires x > 0` | Function precondition |
| **Postcondition** | `ensures result > 0` | Function postcondition |

## 🖥️ System Features

| Feature | Syntax | Description |
|---------|--------|-------------|
| **FFI** | `extern "C" { fn puts(s: *u8) -> i32; }` | C function binding |
| **Volatile** | `*volatile T` | Prevents optimization |
| **Union** | `union Value { i: i64, f: f64 }` | Overlapping memory |
| **Static** | `static GLOBAL: i32 = 42;` | Global variable |

## 🏗️ Dual-Layer Architecture

### Layer 0: Aether Core (`.aeth`)
- Rust-like syntax with explicit ownership
- Full system programming capabilities
- Effect system and contracts

### Layer 1: Aether Script (`.ath`)
- Python-like indentation syntax
- Mutable-by-default
- Transpiles 1:1 to Layer 0

## 📊 Comparison

| Feature | C | Rust | AetherLang |
|---------|:-:|:----:|:----------:|
| Memory Safety | ❌ Manual | ✅ Borrow Checker | ✅ Ownership |
| Null Safety | ❌ | ✅ Option<T> | ✅ Option<T> |
| Generics | ❌ | ✅ | ✅ |
| Traits | ❌ | ✅ | ✅ |
| Lifetimes | ❌ | ✅ | ✅ |
| Effects | ❌ | ❌ | ✅ |
| Contracts | ❌ | Crate | ✅ Built-in |
| Module System | ❌ Headers | ✅ | ✅ |
| Self-Hosting | ❌ | ✅ | ✅ 100% |
