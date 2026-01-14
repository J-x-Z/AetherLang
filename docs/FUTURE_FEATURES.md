# AetherLang Future Features & Tooling
### 1. Dual-Layer Architecture (In Progress - Stage 13)
*Previously "High Level IDE Scripting"*
- **Status**: Design Complete. Implementation Started.
- **Goal**: `.ath` (Python-like) -> `.aeth` (Rust-like) transpiler.
- **Details**: See `docs/design/DUAL_LAYER_ARCHITECTURE.md`.

> **Status**: Ideas for future consideration or tooling implementation. Not part of the v1.0 Core.

## 🛠️ Tooling-Supported Features (Linter/LSP)

These features should be implemented in the language server or linter (`clippy`-like), not the compiler core.

*   **Time Complexity Annotations**:
    *   Idea: `/// @complexity O(n)` doc comments.
    *   Constraint: Checked by AI/Linter, not formally verified by compiler.
*   **Dependency Graph Visualization**:
    *   Idea: Generate `.dot` files for module dependencies.
    *   Status: Natural fit for the AI-IR layer export.
*   **Mandatory Public Tests**:
    *   Idea: Warn if exported functions lack associated test blocks.
    *   Goal: Improve library reliability.
*   **Explicit Type Suffix Suggestion**:
    *   Idea: Suggest `1u32` instead of `1` where type inference is ambiguous.

## 🔮 Language Extensions (Post-Bootstrap)

Features to consider after the language is self-hosted.

*   **Linear Types**:
    *   Current: `own` keyword provides basic linear semantics.
    *   Future: Stricter "must-use" analysis to prevent resource leaks at compile time.
*   **Versioned Standard Library**:
    *   Idea: Explicitly import `std@v1`.
    *   Goal: Long-term stability assurance.

## 🗑️ Discarded (Too Radical)

Ideas considered but rejected to maintain usability:
*   ❌ **No Syntax Sugar**: `?`, `for` loops are essential for readability.
*   ❌ **Mandatory Explicit Lifetimes**: Reduces productivity for simple cases.
*   ❌ **Mandatory Layout Annotations**: Only necessary for FFI/Hardware, not general logic.
*   ❌ **Forced Semantic Aliasing**: Defining `Age = i32` everywhere is excessive boilerplate.
