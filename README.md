# AetherLang

> A self-hosting systems programming language designed for Aether OS

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

## Features

- **Simplified Ownership System** - `own`/`ref`/`mut` semantics for memory safety
- **Compile-time Safety Checks** - Ownership analysis prevents dangling pointers
- **Fast Compilation** - Designed for incremental builds
- **Self-hosting** - The compiler is written in AetherLang (planned)

## Getting Started

```bash
# Clone the repository
git clone https://github.com/J-x-Z/AetherLang.git
cd AetherLang

# Build the compiler
cargo build --release

# Run tests
cargo test
```

## Syntax Example

```rust
fn main() {
    print("Hello, AetherLang!")
}

// Ownership system
fn process(ref data: Buffer) {
    // Borrow data without transferring ownership
}

fn consume(own data: Buffer) {
    // Take ownership, released when function ends
}

// Error handling
fn read_file(path: str) -> Result<String, Error> {
    let content = fs::read(path) or return Err(Error::NotFound)
    return Ok(content)
}
```

## Project Structure

```
src/
├── frontend/     # Lexer, Parser, Semantic Analysis
├── middle/       # IR Generation and Optimization
├── backend/      # Code Generation (LLVM)
└── main.rs       # CLI Entry Point
```

## Development Status

- [x] **Frontend** - Lexer, Parser, Semantic Analyzer
- [x] **Middle-end** - Aether IR, Optimizer, IR Printer
- [ ] **Backend** - LLVM Code Generation
- [ ] **Standard Library** - core, collections, io
- [ ] **Bootstrapping** - Self-hosting compiler

## Testing

```bash
cargo test
```

Current: **25 tests passing** ✅

## Documentation

- [Lexical Specification](docs/spec/词法规范.md)
- [Syntax Specification](docs/spec/语法规范.md)
- [Type System](docs/spec/类型系统.md)
- [Architecture Overview](docs/design/架构概览.md)

## License

Apache License 2.0 - see [LICENSE](LICENSE)
