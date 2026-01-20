# AetherLang

> **AI-Native Systems Programming Language** - Designed to reduce AI hallucinations and enable AI self-iteration

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
![Tests](https://img.shields.io/badge/tests-passing-green)
![Self-Hosting](https://img.shields.io/badge/self--hosting-100%25-brightgreen)

## 🎯 Vision

AetherLang is an **AI-Native Programming Language** built from the ground up to:

1. **Reduce AI Hallucinations** - Explicit interfaces, constrained syntax, semantic annotations
2. **Enable AI Self-Iteration** - AI-readable IR, structured feedback, sandboxed optimization
3. **Maintain Rigor & Safety** - Contract programming, effect system, ownership semantics

## 🔄 Self-Hosting Progress

🎉 **Self-hosting complete!** AetherLang compiler is written in AetherLang.

| Component | Status | Functions | Notes |
|-----------|--------|-----------|-------|
| **Lexer** | ✅ 100% | 23 | Tokenization with 80+ token types |
| **Parser** | ✅ 100% | 76 | Full AST generation |
| **Semantic Analyzer** | ✅ 100% | 68 | Type checking, ownership analysis |
| **IR Generator** | ✅ 100% | 52 | Three-address code generation |
| **Codegen** | ✅ 100% | 40 | Native object file output |
| **Total** | ✅ | **259** | All modules compile to native code |

## ✨ Key Features (P1 Complete)

### Language Features
- **Type System**: `i8`-`i64`, `u8`-`u64`, `f32`, `f64`, `bool`, `char`, `*T`, `&T`, `&mut T`
- **Generics**: Type erasure with `Vec<T>`, `Option<T>` style syntax
- **Traits**: `trait Display { fn display(&self); }`
- **Lifetimes**: `&'a T`, `&'static T` annotations
- **Type Aliases**: `type Int = i64;`
- **Closures**: `|x, y| x + y` lambda syntax

### Module System
- **Use Imports**: `use module::{Item1, Item2}`
- **Dynamic Loading**: Auto-search in `.`, `src_aether/`, `stdlib/`
- **Public Exports**: `pub struct`, `pub fn`

### System Features
- **FFI**: `extern "C" { fn puts(s: *u8) -> i32; }`
- **Unions**: Memory-overlapping types
- **Volatile**: `*volatile T` for memory-mapped I/O
- **Inline ASM**: `asm!("mov eax, 1")` (planned)

## ✨ Dual-Layer Architecture

AetherLang introduces a novel **Dual-Layer Architecture** to balance high-level productivity with system-level control.

### Layer 1: Aether Script (`.ath`)
The high-level logic layer. Used for rapid development, scripting, and business logic.
- **Indentation-based syntax** for readability
- **Mutable-by-default** to align with algorithmic pseudocode
- **Implicit Context** management (Anti-Leak System)
- **Transpiles directly** to Layer 0 (Aether Core)

### Layer 0: Aether Core (`.aeth`)
The low-level system layer. Used for kernel, drivers, and performance-critical paths.
- **Explicit Ownership & Lifetimes**
- **Effect System** (`pure`, `effect[io]`) tracking side-effects
- **Contract Programming** (`requires`/`ensures`) for formal verification

## 🏗️ Technical Stack

```
src/
├── frontend/     # Lexer, Parser, Semantic Analysis, Module System
│   ├── lexer.rs      # 80+ tokens, unicode support
│   ├── parser.rs     # Full expression/statement parsing
│   ├── semantic.rs   # Type inference, ownership, traits
│   └── module.rs     # ModuleLoader for use statements
├── script/       # Aether Script Frontend & Transpiler (Layer 1)
├── middle/       # IR Generation and Optimization
│   ├── ir.rs         # Three-address code IR
│   └── ir_gen.rs     # AST to IR translation
├── backend/      # ELF Linker, C / LLVM Code Generation
│   └── llvm/         # LLVM backend integration
├── ai_ir/        # 🆕 AI-Readable IR Layer
└── types/        # Type System & Resolution
```

## 🚀 Quick Start

```bash
# Build the compiler
cargo build --release --features llvm

# Compile and run a program
cargo run --features llvm -- build examples/hello.aeth
./hello

# Run all tests
cargo test
```

### Example: Hello World
```rust
// hello.aeth
extern "C" {
    fn puts(s: *u8) -> i32;
}

fn main() -> i32 {
    puts("Hello, AetherLang!" as *u8);
    return 0;
}
```

### Example: Generic Function
```rust
fn identity<T>(x: T) -> T {
    return x;
}

fn main() -> i32 {
    let a: i64 = identity(42);
    return a as i32;
}
```

### Example: Module Import
```rust
// point.aeth
pub struct Point { x: i64, y: i64 }
pub fn create_point(x: i64, y: i64) -> Point { Point { x: x, y: y } }

// main.aeth
use point::{Point, create_point}
fn main() -> i32 { ... }
```

## 📊 Development Status

| Phase | Status | Description |
|-------|--------|-------------|
| **P0: Self-Hosting** | ✅ | 5/5 modules, 259 functions compiled |
| **P1: Core Language** | ✅ | Floats, generics, closures, traits, lifetimes, modules |
| **P2: Platforms** | 🚧 | Linux, Windows (planned) |
| **P3: SIMD/Matrix** | 📋 | Vector types, BLAS FFI (planned) |
| **P4: Kernel Dev** | 📋 | Inline ASM, naked functions (planned) |
| **P5: AI/GPU** | 📋 | CUDA, Metal, tensors (planned) |

## 📚 Documentation

### Language Reference
- [Language Guide](docs/LANGUAGE.md) - Complete language reference
- [Grammar Spec](docs/grammar.ebnf) - Formal EBNF grammar
- [Type System](docs/type_system.rules) - Type inference rules

### Design Documents
- [AI-IR Design](docs/design/ai_ir_design.md) - AI-Native Interface
- [Aether Script Spec](docs/AETHER_SCRIPT_SPEC.md) - Layer 1 syntax

### Tutorials
- [Quick Start Guide](docs/GUIDE.md) - Getting started
- [Migration Guide](docs/MIGRATION.md) - From Rust/C

## 🔬 For AI Models

AetherLang is designed to be **AI-friendly**:

1. **Constrained Syntax** - Fewer ways to express the same thing
2. **Explicit Semantics** - Ownership, effects, contracts all visible
3. **Structured Errors** - Machine-readable output with fix suggestions
4. **AI-IR Layer** - Semantic graph + intent annotations for AI understanding

## 🤝 Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Apache License 2.0 - see [LICENSE](LICENSE)
