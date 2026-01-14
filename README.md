# AetherLang

> **AI-Native Systems Programming Language** - Designed to reduce AI hallucinations and enable AI self-iteration

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
![Tests](https://img.shields.io/badge/tests-42%20passing-green)

## 🎯 Vision

AetherLang is an **AI-Native Programming Language** built from the ground up to:

1. **Reduce AI Hallucinations** - Explicit interfaces, constrained syntax, semantic annotations
2. **Enable AI Self-Iteration** - AI-readable IR, structured feedback, sandboxed optimization
3. **Maintain Rigor & Safety** - Contract programming, effect system, ownership semantics

## ✨ Dual-Layer Architecture

AetherLang introduces a novel **Dual-Layer Architecture** to balance high-level productivity with system-level control.

### Layer 1: Aether Script (`.ath`)
The high-level logic layer. Used for rapid development, scripting, and business logic.
- **Indentation-based syntax** for readability.
- **Mutable-by-default** to align with algorithmic pseudocode.
- **Implicit Context** management (Anti-Leak System) to handle allocations safely.
- **Transpiles directly** to Layer 0 (Aether Core) with zero hidden runtime.

### Layer 0: Aether Core (`.aeth`)
The low-level system layer. Used for kernel, drivers, and performance-critical paths.
- **Explicit Ownership & Lifetimes**.
- **Effect System** (`pure`, `effect[io]`) tracking side-effects.
- **Contract Programming** (`requires`/`ensures`) for formal verification.

## 🏗️ Technical Stack

```
src/
├── frontend/     # Lexer, Parser, Semantic Analysis (Core)
├── script/       # Aether Script Frontend & Transpiler (Layer 1)
├── middle/       # IR Generation and Optimization  
├── backend/      # ELF Linker, C / LLVM Code Generation
├── ai_ir/        # 🆕 AI-Readable IR Layer
│   ├── semantic_graph.rs   # Nodes & Edges (calls, dataflow)
│   ├── intent.rs           # High-level intent annotations
│   └── query.rs            # AI Query API
└── types/        # Type System
```

## 🚀 Quick Start

```bash
# Build the compiler
cargo build --release

# Run tests
cargo test

# Compile an Aether Script file (Layer 1)
cargo run -- build examples/hello.ath

# Compile an Aether Core file (Layer 0)
cargo run -- build examples/kernel/main.aeth
```

## 📊 Development Status

| Phase | Status | Description |
|-------|--------|-------------|
| **Core Frontend** | ✅ | Lexer, Parser, Semantic Analysis |
| **Script Layer** | ✅ | Script Frontend, Transpiler, Source Mapping |
| **AI-IR Layer** | ✅ | Semantic Graph, Intent Propagation |
| **System Features**| ✅ | FFI, Unions, Volatile, Inline ASM |
| **Backend** | 🚧 | ELF Builder (Partial), C-Gen (Partial) |

**Tests: 42 passing** ✅

## 📚 Documentation

### Specifications
- [Aether Script Spec (Layer 1)](docs/AETHER_SCRIPT_SPEC.md) - High-level language rules
- [Grammar Spec (Layer 0)](docs/grammar.ebnf) - Formal BNF grammar
- [AI-IR Design](docs/design/ai_ir_design.md) - AI-Native Interface

## 🔬 For AI Models

AetherLang is designed to be **AI-friendly**:

1. **Constrained Syntax** - Fewer ways to express the same thing
2. **Explicit Semantics** - Ownership, effects, contracts all visible
3. **Structured Errors** - Machine-readable output with fix suggestions
4. **AI-IR Layer** - Semantic graph + intent annotations for AI understanding

## License

Apache License 2.0 - see [LICENSE](LICENSE)
