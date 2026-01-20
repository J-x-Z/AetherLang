# AetherLang Project Roadmap

> **AI-Native Systems Programming Language**  
> **Last Updated**: 2026-01-21  
> **Status**: P0 Self-Hosting Complete, P1 Core Extensions Complete

## 1. Executive Summary

AetherLang is the first **AI-Native Systems Language**, designed for AI to **read, understand, and iterate upon safely**.

### Dual-Layer Architecture
- **Layer 0 (Aether Core `.aeth`)**: Rust-like explicit systems programming
- **Layer 1 (Aether Script `.ath`)**: Python-like syntax, transpiles 1:1 to Core

---

## 2. Completed Milestones

### ✅ P0: Self-Hosting (Complete)

**5/5 compiler modules written in AetherLang:**

| Module | Functions | Status |
|--------|-----------|--------|
| Lexer | 23 | ✅ |
| Parser | 76 | ✅ |
| Semantic Analyzer | 68 | ✅ |
| IR Generator | 52 | ✅ |
| Code Generator | 40 | ✅ |
| **Total** | **259** | ✅ |

### ✅ P1: Core Language Extensions (Complete)

| Phase | Features | Status |
|-------|----------|--------|
| **A: Numerics** | `f32`/`f64` support, type inference | ✅ |
| **B: Generics** | `Vec<T>`, type erasure codegen | ✅ |
| **C: Closures** | `\|x, y\| x + y` lambda syntax | ✅ |
| **D: Traits** | `trait Display { fn display(&self); }` | ✅ |
| **E: Lifetimes** | `&'a T`, `&'static T` annotations | ✅ |
| **F: Type Aliases** | `type Int = i64;` | ✅ |
| **G: Modules** | `use foo::{A, B}`, ModuleLoader | ✅ |

---

## 3. Current Roadmap

### 🚧 P2: Cross-Platform Support (4h estimated)

| Task | Target | Status |
|------|--------|--------|
| Linux x86_64 | ELF64 output | 📋 Planned |
| Windows | MSVC/MinGW | 📋 Planned |
| Cross-compilation | `--target` flag | 📋 Planned |

### 📋 P3: SIMD & Matrix (12h estimated)

| Task | Description | Status |
|------|-------------|--------|
| `#[simd]` annotation | Auto-vectorization hints | 📋 Planned |
| Vector types | `f32x4`, `f64x2`, `i32x8` | 📋 Planned |
| Matrix library | `Matrix<T, M, N>` generic type | 📋 Planned |
| BLAS FFI | Basic linear algebra bindings | 📋 Planned |

### 📋 P4: Kernel Development (16h estimated)

| Task | Description | Status |
|------|-------------|--------|
| `asm!` macro | Inline assembly | 📋 Planned |
| `#[naked]` | Naked functions | 📋 Planned |
| `#[interrupt]` | Interrupt handlers | 📋 Planned |
| `#[repr(C)]` | C-compatible layout | 📋 Planned |
| `#![no_std]` | Freestanding mode | 📋 Planned |

### 📋 P5: AI/GPU Compute (20h estimated)

| Task | Description | Status |
|------|-------------|--------|
| CUDA FFI | NVIDIA GPU binding | 📋 Planned |
| Metal FFI | Apple GPU binding | 📋 Planned |
| `Tensor<T, Shape>` | Tensor type with shape inference | 📋 Planned |
| Autodiff | Automatic differentiation | 📋 Planned |

---

## 4. Future Stages

### Stage 16: Package Manager
- `apm` package manager
- Dependency resolution
- Central registry

### Stage 17: IDE Support
- Language Server Protocol (LSP)
- Syntax highlighting
- Code completion

### Stage 18: AI-Native Features
- AI-IR semantic graph
- Intent propagation
- AI-guided optimization

---

## 5. Platform Support

| Platform | Arch | Status |
|----------|------|--------|
| macOS | ARM64 | ✅ Complete |
| macOS | x86_64 | ✅ Complete |
| Linux | x86_64 | 📋 P2 |
| Windows | x86_64 | 📋 P2 |

---

## 6. Time Estimates

| Phase | Estimated Time | Dependencies |
|-------|----------------|--------------|
| P2 Platform | 4h | None |
| P3 SIMD/Matrix | 12h | P1 |
| P4 Kernel | 16h | P1 |
| P5 AI/GPU | 20h | P3, P4 |
| **Total Remaining** | **52h** | |
