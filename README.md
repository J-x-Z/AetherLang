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

## ✨ AI-Native Features (P5 Complete)

AetherLang enforces **"Radical Explicitness"** to reduce AI hallucinations:

### 1. Mandatory Type Annotations
```aether
// ❌ Forbidden - type inference
let x = 10;

// ✅ Required - explicit types
let x: i32 = 10;
let name: *u8 = "hello\0" as *u8;
```

### 2. Effect System (Hard Errors)
```aether
// ❌ Compile error - calling effect[io] without declaring it
fn bad_caller() {
    puts("hello\0" as *u8);
}

// ✅ Correct - effect declared
fn good_caller() effect[io] {
    puts("hello\0" as *u8);
}
```

| Effect | Description |
|--------|-------------|
| `io` | Input/Output |
| `alloc` | Memory allocation |
| `read` | Read global state |
| `write` | Write global state |
| `panic` | May panic |

### 3. No `unwrap()` on Option/Result
```aether
// ❌ Does not exist - unwrap() is not defined
let value: i32 = maybe_value.unwrap();

// ✅ Use match or unwrap_or
let value: i32 = maybe_value.unwrap_or(0);
```

### 4. Explicit Allocators
```aether
use alloc::{Allocator, GlobalAllocator, ArenaAllocator}

// Vec requires allocator parameter
let v: Vec<i32, GlobalAllocator> = Vec::new_in(GlobalAllocator::new());

// Use Arena for batch deallocation
let arena: ArenaAllocator = ArenaAllocator::new(1024);
let v: Vec<i32, ArenaAllocator> = Vec::new_in(arena);
```

## ✨ Dual-Layer Architecture

AetherLang introduces a novel **Dual-Layer Architecture** to balance high-level productivity with system-level control.

### Layer 1: Aether Script (`.ath`)
The high-level logic layer. Used for rapid development, scripting, and business logic.
```python
# hello.ath - Python-like syntax
def main():
    print("Hello from Script!")
    return 0
```
- **Indentation-based syntax** for readability
- **Mutable-by-default** to align with algorithmic pseudocode
- **Transpiles directly** to Layer 0 (Aether Core)

### Layer 0: Aether Core (`.aeth`)
The low-level system layer. Used for kernel, drivers, and performance-critical paths.
```aether
// hello.aeth - Rust-like syntax
extern "C" { fn puts(s: *u8) -> i32; }

fn main() -> i32 {
    puts("Hello, AetherLang!\0" as *u8);
    return 0;
}
```
- **Explicit Ownership & Lifetimes**
- **Effect System** (`pure`, `effect[io]`) tracking side-effects
- **Contract Programming** (`requires`/`ensures`) for formal verification

## 🏗️ Technical Stack

```
src/
├── frontend/     # Lexer, Parser, Semantic Analysis, Module System
├── script/       # Aether Script Transpiler (Layer 1 → Layer 0)
├── middle/       # IR Generation and Optimization
├── backend/      # C / LLVM Code Generation
│   ├── c/            # C backend (portable)
│   └── llvm/         # LLVM backend (optimized)
├── ai_ir/        # AI-Readable IR Layer
└── types/        # Type System & Resolution
```

## 🚀 Quick Start

```bash
# Build the compiler
cargo build --release --features llvm

# Compile and run a program
cargo run --features llvm -- build examples/hello.aeth
./hello

# Or use C backend (no LLVM required)
cargo build --release --no-default-features
./target/release/aethc --emit-c examples/hello.aeth
cc -o hello examples/hello.c && ./hello

# Run all tests
cargo test
```

## 📊 Development Status

| Phase | Status | Description |
|-------|--------|-------------|
| **P0: Self-Hosting** | ✅ | 5/5 modules, 259 functions compiled |
| **P1: Core Language** | ✅ | Floats, generics, closures, traits, lifetimes, modules |
| **P2: Platforms** | ✅ | Linux, macOS, Windows CI passing |
| **P3: SIMD/Matrix** | ✅ | Vector types `f32x4`, BLAS FFI, Matrix4x4 |
| **P4: Kernel Dev** | ✅ | Inline ASM, naked functions, atomic, MMIO |
| **P5: AI-Native** | ✅ | Mandatory types, effect system, no unwrap, explicit allocators |
| **P6: Engineering** | ✅ | CI/CD, .ath transpiler, jxz config, C backend fixes |

## 📦 JXZ Package Manager

AetherLang includes **JXZ** - a cross-platform package manager written in AetherLang:

```bash
# Project management
jxz init          # Create new project with Jxz.toml
jxz build         # Build project (reads config from Jxz.toml)
jxz run           # Build and run
jxz test          # Run tests
jxz clean         # Remove build artifacts

# Dependency management
jxz add <pkg>     # Add dependency to Jxz.toml
jxz remove <pkg>  # Remove dependency
jxz install       # Install from Jxz.lock
```

**161 functions** written entirely in AetherLang (self-hosting!)

## 📚 Documentation

### Language Reference
- [Language Guide](docs/LANGUAGE.md) - Complete language reference with P5 rules
- [Grammar Spec](docs/grammar.ebnf) - Formal EBNF grammar

### Design Documents
- [AI-IR Design](docs/design/ai_ir_design.md) - AI-Native Interface
- [Dual-Layer Architecture](docs/design/DUAL_LAYER_ARCHITECTURE.md) - .ath/.aeth layers
- [Aether Script Spec](docs/AETHER_SCRIPT_SPEC.md) - Layer 1 syntax
- [Context Pattern](docs/context-pattern.md) - Explicit context passing

### Tutorials
- [Quick Start Guide](docs/GUIDE.md) - Getting started

## 🔬 For AI Models

AetherLang is designed to be **AI-friendly**:

1. **Constrained Syntax** - Fewer ways to express the same thing
2. **Explicit Semantics** - Ownership, effects, contracts all visible
3. **Structured Errors** - Machine-readable output with fix suggestions
4. **AI-IR Layer** - Semantic graph + intent annotations for AI understanding
5. **No Hidden Magic** - Every operation is explicit and traceable

## 🤝 Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Apache License 2.0 - see [LICENSE](LICENSE)
