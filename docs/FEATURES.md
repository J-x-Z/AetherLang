# AetherLang Feature Cheat Sheet

> **One-page summary of core features for developers.**

| Category | Feature | Why? (Problem solved) | Syntax Example |
| :--- | :--- | :--- | :--- |
| **System** | **Smart Unsafe** | Human error in `unsafe` blocks | `unsafe(reason="DMA", verifier=check_dma) { ... }` |
| | **Smart FFI** | Unknown C function side-effects | `extern "C" { @pure @reads(p) fn hash(p: *u8); }` |
| | **Volatile** | Compiler optimizing away MMIO | `let p = 0xB8000 as *volatile u8;` |
## 🏗️ Dual-Layer Architecture (Stage 13)
A unique approach to systems programming:
- **Core Layer (`.aeth`)**: The "Truth". Uncompromising systems language.
- **Script Layer (`.ath`)**: Python-like syntax, compiles 1:1 to Core.
- **Anti-Leak System**:
    - **White-box Expansion**: Generated code is human-readable and standard-library based. No hidden runtimes.
    - **Source Mapping**: Debuggers see the Script file, not the generated file.

## 🔗 Self-Hosted Toolchain
- **Linker**: Built-in ELF64 linker (no dependency on `ld` for basic binaries).
- **LSP**: Integrated Language Server.
| | **Linear Types** | Resource leaks (forgetting free) | `fn take(x: own String)` (Must accept or drop) |
| **Safety** | **Effects** | Hidden IO/Panics deep in call stack | `fn log() effect[io] { ... }` |
| | **Contracts** | Implicit assumptions leading to bugs | `fn div(a,b) [requires b != 0] { a/b }` |
| | **Strict Types** | Implicit conversion bugs (Ariane 5) | `let x: i64 = y as i64;` (No implicit casts) |
| **Structure** | **Zero Ambiguity** | Parser quirks / AI hallucination | Context-free LL(1) grammar (No complex lookahead) |
| | **Explicit Life** | Hidden object lifetimes | `fn f(x: shared T) -> shared T` |
| | **Modules** | Namespace pollution | `mod kernel { pub struct Process {} }` |

## Quick Comparison

| Feature | C | Rust | AetherLang |
| :--- | :---: | :---: | :---: |
| Memory Management | Manual | Ownership/Borrow | Ownership + Contexts |
| Null Safety | ❌ | `Option<T>` | `Option<T>` |
| Implicit Casts | ✅ (Dangerous) | ❌ | ❌ (Strict Mode) |
| Side Effects | Implicit | Implicit | **Explicit Effects** |
| Contracts | ❌ | Crates (`contracts`) | **First-class** |
| AI Readability | ❌ (Macros) | ⚠️ (Macros/Sugar) | **✅ (Zero Sugar)** |
