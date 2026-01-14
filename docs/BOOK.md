# AetherLang: The AI-Native Systems Programming Language

> **Version**: 1.0  
> **Status**: Windows Frontend Complete, Cross-Platform Backend Pending

---

## 1. Introduction

AetherLang is a systems programming language designed from the ground up to be **AI-Native**. It solves the three fundamental problems of AI-assisted code generation:

1. **Anti-Hallucination**: Explicit APIs, formal grammar, no implicit magic
2. **Fault Tolerance**: Contracts, strict types, effect system
3. **Self-Iteration**: AI-IR layer, mutation API, intent preservation

---

## 2. Quick Start

```bash
# Build the compiler
cargo build --release

# Compile an AetherLang file to C
cargo run -- examples/hello.aeth --emit-c

# Run tests
cargo test
```

---

## 3. Language Features

### 3.1 Functions with Contracts

```aether
fn divide(a: i64, b: i64) -> i64
    [requires b != 0, ensures result * b == a]
    pure
{
    a / b
}
```

### 3.2 Ownership System

```aether
fn consume(data: own String) { }      // Takes ownership
fn borrow(data: ref String) { }       // Immutable borrow
fn mutate(data: mut String) { }       // Mutable borrow
fn share(data: shared Arc<i32>) { }   // Shared ownership
```

### 3.3 Effect System

```aether
fn pure_add(a: i64, b: i64) -> i64 pure { a + b }

fn impure_log(msg: String) effect[io] { println(msg); }
```

### 3.4 Smart Unsafe (Phase 9)

```aether
unsafe(reason = "Direct hardware access", verifier = check_port) {
    *(0x3F8 as *volatile u8) = 'A';
}
```

### 3.5 Smart FFI (Phase 9)

```aether
extern "C" {
    @pure @reads(input)
    fn compute_hash(input: *u8, len: u32) -> u32;
    
    @allocs(size)
    fn malloc(size: usize) -> *void;
}
```

---

## 4. Type System (Strict Mode)

AetherLang enforces **zero implicit conversions**:

```aether
let x: i32 = 42;
let y: i64 = x;      // ERROR: Type mismatch
let y: i64 = x as i64;  // OK: Explicit cast required
```

See `docs/type_system.rules` for formal inference rules.

---

## 5. AI-IR Layer

The AI-IR provides:
- **Query API**: `get_callers()`, `get_callees()`, `get_type()`, `get_effects()`
- **Mutation API**: `replace_expression()`, `inline_call()`, `validate()`
- **Intent Preservation**: High-level semantics preserved through compilation

See `docs/design/ai-ir-api.md` for complete API reference.

---

## 6. Project Structure

```
AetherLang/
├── src/
│   ├── frontend/      # Lexer, Parser, Semantic Analysis
│   ├── middle/        # IR, Optimizer
│   ├── backend/       # C codegen, (LLVM pending)
│   ├── ai_ir/         # AI-Native IR layer
│   └── feedback/      # Structured error reporting
├── docs/
│   ├── grammar.ebnf   # Formal grammar (AI-readable)
│   ├── type_system.rules  # Type inference rules
│   ├── effects.json   # Effect manifest
│   └── HANDOFF.md     # Cross-platform development guide
└── examples/          # Example programs
```

---

## 7. Roadmap

### Completed ✅
- Phase 1-7: Frontend Core (Lexer, Parser, Semantic, AI-IR)
- Phase 8: System Features (FFI, Static, Union, Volatile)
- Phase 9: AI-Enhanced Features (Smart Unsafe, Strict Types)
- Phase 10: Documentation Suite

### Pending 🔜
- LLVM Backend (requires cross-platform environment)
- Standard Library (core no_std types)
- Package Manager (`apm`)

---

## 8. References

| Document | Purpose |
|----------|---------|
| `docs/grammar.ebnf` | Machine-readable grammar for AI/Parser |
| `docs/type_system.rules` | Formal type inference rules |
| `docs/effects.json` | Built-in function effects |
| `docs/HANDOFF.md` | Cross-platform development guide |
| `docs/GUIDE.md` | User guide with examples |
