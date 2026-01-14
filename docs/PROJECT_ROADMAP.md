# AetherLang Project Roadmap

> **Target**: AI-Native Systems Programming Language v1.0
> **Generated**: 2026-01-14 (End of Stage 13)
> **Status**: Core & Script Layer MVP Complete. Ready for Cross-Platform Port.

## 1. Executive Summary

AetherLang aims to be the first **AI-Native Systems Language**, designed not just for humans to write, but for AI to **read, understand, and iterate upon safely**.

It features a **Dual-Layer Architecture**:
-   **Layer 0 (Aether Core)**: Explicit, low-level system programming (like Rust/C).
    -   Features: Contract Programming (`requires/ensures`), Effect System (`pure/effect`), Explicit Ownership.
-   **Layer 1 (Aether Script)**: High-level Python-style syntax for rapid logic.
    -   Features: Transpiles 1:1 to Core, Implicit Context, Source Mapping.

---

## 2. Achievements (Stage 1-13)

We have successfully bootstrapped the compiler (currently written in Rust) and implemented the foundational pillars.

### Core Compiler (Layer 0)
-   [x] **Frontend**: Complete Lexer, Parser, and Semantic Analyzer.
-   [x] **Type System**: Basic types, Structs, Enums, Generics, Traits.
-   [x] **Memory Safety**: `own` / `ref` / `mut` / `shared` ownership modes.
-   [x] **AI-Native Features**:
    -   Contract Programming syntax & AST.
    -   Effect System propagation.
    -   **AI-IR**: Semantic Graph for AI analysis.

### Script Layer (Layer 1)
-   [x] **Frontend**: Indentation-based syntax (`.ath`).
-   [x] **Transpiler**: Auto-generates type-safe Aether Core (`.aeth`).
-   [x] **Tooling**: Source mapping (`// @source`) for debugging.

### System Integration
-   [x] **CLI**: `aethc build` supports both `.aeth` variable `.ath`.
-   [x] **Backends**:
    -   Basic C Backend (needs external GCC/Clang).
    -   ELF64 Builder (Prototype).
    -   Interpreter/Comptime Skeleton.

---

## 3. Future Roadmap (The Missing Pieces)

This section details the remaining work required to reach v1.0 Self-Hosting.

### 🚩 Stage 14: Cross-Platform & LLVM (Immediate Next Step)
**Goal**: Move development to Linux/macOS to access full toolchains.
-   [ ] **LLVM Backend**: Implement `llvm-sys` codegen to replace C Backend reliability.
-   [ ] **Cross-Compilation**: Support compiling for Linux/macOS/Windows targets.
-   [ ] **Binary Output**: Remove dependency on external `gcc` for Windows users (via LLld).

### 🚩 Stage 15: Self-Hosting (The "Bootstrap")
**Goal**: Re-write the AetherLang compiler **IN AetherLang**.
-   [ ] **Standard Library (Core)**: `Vec`, `String`, `Map`, `File`, `Thread` implemented in Aether.
-   [ ] **Compiler Rewrite**: Port `src/frontend` from Rust to Aether Core.
-   [ ] **Bootstrap**: Use the Rust compiler (Stage 1) to compile the Aether compiler (Stage 2).

### 🚩 Stage 16: Package Manager (`apm`)
**Goal**: A modern, decentralized package manager.
-   [ ] **Manifest**: `Aether.toml` defining dependencies.
-   [ ] **Registry**: Decentralized (Git-based) or Centralized index.
-   [ ] **Build System**: Parallel dependency resolution and building.
-   [ ] **Scripts**: `pre-build`, `post-build` hooks (sandboxed).

### 🚩 Stage 17: IDE Support (LSP)
**Goal**: First-class developer experience.
-   [ ] **Language Server**: Implement LSP protocol (Go to Def, Hover, Completion).
-   [ ] **VS Code Extension**: Syntax highlighting and LSP client.
-   [ ] **Debugger**: `gdb`/`lldb` integration using Source Maps generated in Stage 13.

### 🚩 Stage 18: AI Ecosystem (The "Killer Feature")
**Goal**: Activate the "AI-Native" potential.
-   [ ] **Iteration Engine**: Implement the feedback loop (Edit -> Compile -> Test -> Fix).
-   [ ] **Local LLM Integration**: Compiler calls local LLM to fix semantic errors automagically.
-   [ ] **AI-IR Query CLI**: `aethc query --callers main` for AI agents.

---

## 4. Technical Gaps Overview

| Component | Current Status | Missing / Needs |
| :--- | :--- | :--- |
| **Backend** | C Transpiler (Fragile) | **LLVM / Native Machine Code** |
| **StdLib** | Minimal (C wrappers) | **Full Native Implementation** |
| **Linker** | External (GCC) | **Internal LLD / Mold** |
| **Parallelism**| None | **Async Runtime / Threads** |
| **Error Msg** | Basic Strings | **Rich Diagnostics (Rust-style)** |

## 5. Development Guidelines (For Future You)

-   **Workflow**: Always implement features in Layer 0 (Core) first, then expose sugar in Layer 1 (Script).
-   **Testing**: Maintain the `examples/` folder. Every new feature needs an `.aeth` and `.ath` example.
-   **Documentation**: Keep `docs/design/` updated. AI reads these to understand the language it is writing.

---

**End of Report.**
