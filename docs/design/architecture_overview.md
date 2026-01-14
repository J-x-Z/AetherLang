# AetherLang Architecture Overview

> This document describes the overall architecture and module responsibilities of AetherLang compiler.

## 1. Compilation Flow

```
Source Code (.aeth)
     ↓
┌─────────────────────────────────────┐
│            Frontend                 │
│  ┌─────────┐  ┌─────────┐  ┌──────┐ │
│  │  Lexer  │→ │ Parser  │→ │ Sema │ │
│  └─────────┘  └─────────┘  └──────┘ │
│     Tokens       AST      Typed AST │
└─────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────┐
│           Middle-end                │
│  ┌─────────────┐  ┌───────────────┐ │
│  │  IR Gen     │→ │  Optimizer    │ │
│  └─────────────┘  └───────────────┘ │
│     Aether IR      Optimized IR     │
└─────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────┐
│            Backend                  │
│  ┌─────────────────────────────────┐│
│  │         CodeGen Trait           ││
│  └─────────────────────────────────┘│
│           ↓              ↓          │
│  ┌────────────┐   ┌─────────────┐   │
│  │ LLVM (v1.0)│   │ Native(v2.0)│   │
│  └────────────┘   └─────────────┘   │
└─────────────────────────────────────┘
                  ↓
            Machine Code / Binary
```

## 2. Module Responsibilities

### 2.1 Frontend (`src/frontend/`)

| Module | File | Responsibility |
|--------|------|----------------|
| **Token** | `token.rs` | Token and TokenKind definitions |
| **Lexer** | `lexer.rs` | Source → Token Stream |
| **AST** | `ast.rs` | Abstract Syntax Tree definitions |
| **Parser** | `parser.rs` | Token Stream → AST |
| **Semantic** | `semantic.rs` | Type checking, ownership analysis |

### 2.2 Middle-end (`src/middle/`)

| Module | File | Responsibility |
|--------|------|----------------|
| **IR** | `ir.rs` | Aether IR Data Structures |
| **IR Gen** | `ir_gen.rs` | AST → IR Conversion |
| **Optimizer** | `optimize.rs` | DCE, Constant Folding, CSE |

### 2.3 Backend (`src/backend/`)

| Module | File | Responsibility |
|--------|------|----------------|
| **CodeGen** | `codegen.rs` | Backend Abstraction Trait |
| **LLVM** | `llvm/*.rs` | LLVM Code Generation |
| **Native** | `native/*.rs` | Custom Backend (v2.0) |

### 2.4 Types (`src/types/`)

| Module | File | Responsibility |
|--------|------|----------------|
| **Type System** | `type_system.rs` | Type definitions and helper methods |

### 2.5 Utils (`src/utils/`)

| Module | File | Responsibility |
|--------|------|----------------|
| **Span** | `span.rs` | Source position tracking |
| **Error** | `error.rs` | Error types and reporting |

## 3. Data Flow

```
Source String
    ↓ Lexer::tokenize()
Vec<Token>
    ↓ Parser::parse_program()
Program (AST)
    ↓ SemanticAnalyzer::analyze()
Typed AST + SymbolTable
    ↓ IRGenerator::generate()
IRModule
    ↓ Optimizer::optimize()
Optimized IRModule
    ↓ CodeGen::generate()
Vec<u8> (Machine Code)
    ↓ Write File
Executable
```

## 4. Key Design Decisions

### 4.1 Pluggable Backend

Backend abstraction via `CodeGen` trait:

```rust
pub trait CodeGen {
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>>;
    fn target_triple(&self) -> &str;
    fn name(&self) -> &str;
}
```

This allows switching between LLVM and Custom backends.

### 4.2 Simplified Ownership System

- No lifetime annotations (mostly inferred or scoped)
- Three modes: `own` / `ref` / `mut`
- Compile-time basic checks + Runtime bounds checks

### 4.3 Incremental Compilation (Future)

- File-level incremental compilation
- Cached compiled IR

## 5. Directory Structure

```
AetherLang/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── frontend/
│   │   ├── mod.rs
│   │   ├── token.rs
│   │   ├── lexer.rs
│   │   ├── ast.rs
│   │   ├── parser.rs
│   │   └── semantic.rs
│   ├── middle/
│   │   ├── mod.rs
│   │   ├── ir.rs
│   │   ├── ir_gen.rs
│   │   └── optimize.rs
│   ├── backend/
│   │   ├── mod.rs
│   │   ├── codegen.rs
│   │   ├── llvm/
│   │   │   ├── mod.rs
│   │   │   └── llvm_codegen.rs
│   │   └── native/ (v2.0)
│   ├── types/
│   │   ├── mod.rs
│   │   └── type_system.rs
│   └── utils/
│       ├── mod.rs
│       ├── span.rs
│       └── error.rs
├── docs/
│   ├── spec/           # Language Specs
│   ├── design/         # Design Docs
│   └── dev/            # Dev Logs
├── stdlib/             # Standard Library (AetherLang Source)
├── tests/              # Test Cases
└── examples/           # Example Code
```

## 6. Test Strategy

| Level | Type | Location |
|-------|------|----------|
| Unit Test | Lexer, Parser, Semantic | Inside modules `#[cfg(test)]` |
| Integration Test | End-to-end compilation | `tests/` directory |
| Example Test | Verify examples run | `examples/` directory |

## 7. Related Docs

- [Lexical Spec](./spec/tokens.md)
- [Grammar Spec](./spec/grammar.md)
- [Type System](./spec/types.md)
