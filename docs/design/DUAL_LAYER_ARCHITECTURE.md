# Dual-Layer Architecture & Aether Script Design

## Overview
AetherLang adopts a **Dual-Layer Architecture** to resolve the "Systems Language vs Scripting Language" dilemma. instead of burying complexity under a heavy runtime (like Go or Java), AetherLang exposes a zero-cost abstraction layer that transpiles 1:1 to its own core systems language.

- **Layer 0 (Core)**: Aether Core (`.aeth`). Rust-like, strictly typed, manual memory management (identifiers/ownership), no GC. The "Truth".
- **Layer 1 (Script)**: Aether Script (`.ath`). Python-like, gradual typing, indentation-based. Transpiles to valid Core code.

## The "Radical Core" Philosophy (Stage 13 Update)
Since User Experience (UX) is handled by the Script Layer, the Core Layer abandons "Convenience" in favor of **"Radical Explicitness" (极致显式)**.

1.  **Zero Implicit Inference**:
    *   Script: `let x = 10`
    *   Core: `let x: i32 = 10;` (Transpiler resolves all types. Core compiler is dumb & fast).
2.  **Explicit Allocation**:
    *   Script: `list.append(item)`
    *   Core: `Vec::push_in(&mut list, item, &mut ctx.allocator)` (No global allocator magic).
3.  **Mandatory Error Handling**:
    *   Core has no `unwrap`. All `Result` types must be explicitly matched or propagated. Scripting syntax sugars this away (`?` operator or exception-like flow).

This makes Aether Core essentially a **"Type-Safe Portable Assembly"**. It is readable, but tedious to write manually—exactly what a generated language (or AI-written language) should be.

### 4. AI-Native Optimization (AI-First)
The "Radical" design is specifically tuned for LLMs:
- **Maximum Context Sensitivity**: Every Core file includes extensive explicit contexts (imports, type definitions). An AI agent reading a Core file needs *zero* external context to understand function logic.
- **Dumb Compiler, Smart AI**: The compiler does minimal magic. If code optimization or refactoring is needed, we rely on the `ai_ir` mutation engine.
- **Self-Correction Friendly**: Error messages are designed to be mechanically parseable JSON, feeding directly back into the AI loop for auto-repair.

### 5. Backend Independence
- The entire toolchain (Script -> Core -> IR) is decoupled from any specific backend.
- **Primary Target**: LLVM (for optimization and cross-platform support on Linux/macOS).
- **Fallback**: Self-Hosted ELF Linker (for bootstrafing).
- **Forbidden**: No dependency on system C compilers (`gcc`/`clang`) for the core language features.


## The "Anti-Leak" System
To prevent the "Abstraction Leak" problem where high-level abstractions make low-level debugging impossible, we enforce a strict **Anti-Leak System**.

### 1. White-box Expansion (白盒展开)
**Principle**: Every construct in Script Layer must map to a *visible* and *comprehensible* construct in Core Layer. There is no "Hidden Runtime".

- **Rule**: `list.append(x)` in Script MUST NOT call a hidden C function `_ath_list_append`.
- **Implementation**: It must transpile to `vec.push(x)` using the standard library `Vec` which the user can inspect. if `Vec` is not imported, the script fails to compile.
- **Goal**: A developer reading the generated Core code should feel like they wrote it manually. It serves as a learning tool for the Core language.

### 2. Source Mapping (源码映射)
**Principle**: Debugging should happen at the Script level, even if execution happens at the Core level.

- **Implementation**: The Transpiler injects `#line` directives into the generated Core code.
    ```aether
    // Generated Core Code
    #line 15 "myscript.ath"
    let x = calculation();
    #line 16 "myscript.ath"
    if x > 10 { panic!("Too high"); }
    ```
- **Effect**: When a panic occurs or a GDB breakpoint hits, the toolchain reports `myscript.ath:16` instead of `build/gen.aeth:402`.

### 3. Transpilation Lens (IDE Insight)
**Principle**: The cost of abstraction must be visible *during development*.

- **Implementation**: IDE/LSP feature. Hovering over a high-level Script construct (e.g. `await future`) shows a "Lens" popup containing the generated Core state machine code.
- **Goal**: Prevent "Performance blindness". Users see that `await` generates 50 lines of struct code, encouraging mindful usage.

## Aether Script Specification

### Syntax
Python-inspired indentation-based syntax for clean, high-level logic.

```python
# script.ath

@comptime
def build_lookup_table():
    # Regular Python code running at compile time!
    return {x: x*x for x in range(10)}

fn main():
    # Generates: let table = [0, 1, 4, 9, ...];
    let table = $build_lookup_table() 
    
    # Generates: println!("Hello");
    print("Hello")
```

### @comptime Metaprogramming
Instead of complex macro syntaxes (referencing `macro_rules!`), Aether Script uses embedded Python (via `RustPython` or similar) to allow arbitrary compile-time code generation.

- **Mechanism**: Functions marked `@comptime` are executed by the compiler's embedded interpreter.
- **Output**: The return value is injected into the AST (like Zig's comptime, but using Python syntax).

## Workflow

1.  **Source**: `main.ath`
2.  **Transpile**: `aethc transpile main.ath` -> `main.aeth` (with `#line` directives)
3.  **Compile**: `aethc build main.aeth` -> `main.o` -> `main`
4.  **Debug**: User debugs `main`, sees `main.ath`.
