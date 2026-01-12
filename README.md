# AetherLang

> 一个为 Aether OS 设计的自托管系统编程语言

[![Build Status](https://github.com/Z1529/AetherLang/actions/workflows/ci.yml/badge.svg)](https://github.com/Z1529/AetherLang/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## 特性

- 🦀 **比 Rust 简单** - 简化的所有权系统 (`own`/`ref`/`mut`)
- 🛡️ **比 C 安全** - 编译期所有权检查，无悬垂指针
- ⚡ **快速编译** - 增量编译，秒级构建
- 🔧 **自托管** - 用 AetherLang 编写的编译器

## 快速开始

```bash
# 克隆仓库
git clone https://github.com/Z1529/AetherLang.git
cd AetherLang

# 构建编译器
cargo build --release

# 编译示例程序
./target/release/aethc examples/hello.aeth
```

## 语法示例

```rust
// Hello World
fn main() {
    print("Hello, AetherLang!")
}

// 所有权系统
fn process(ref data: Buffer) {
    // 借用数据，不转移所有权
}

fn consume(own data: Buffer) {
    // 获取所有权，函数结束时释放
}

// 错误处理
fn read_file(path: str) -> Result<String, Error> {
    let content = fs::read(path) or return Err(Error::NotFound)
    return Ok(content)
}
```

## 项目结构

```
src/
├── frontend/     # 词法分析、语法分析、语义分析
│   ├── lexer.rs
│   ├── parser.rs
│   └── semantic.rs
├── middle/       # IR 生成和优化
│   ├── ir.rs
│   ├── ir_gen.rs
│   └── optimize.rs
├── backend/      # 代码生成 (LLVM)
│   └── llvm/
└── main.rs       # CLI 入口
```

## 开发进度

- [x] **前端** - Lexer, Parser, Semantic Analyzer
- [x] **中端** - Aether IR, Optimizer, IR Printer
- [ ] **后端** - LLVM Code Generation
- [ ] **标准库** - core, collections, io
- [ ] **自举** - 用 AetherLang 重写编译器

## 测试

```bash
cargo test
```

当前测试状态: **25 tests passing** ✅

## 文档

- [词法规范](docs/spec/词法规范.md)
- [语法规范](docs/spec/语法规范.md)
- [类型系统](docs/spec/类型系统.md)
- [架构概览](docs/design/架构概览.md)

## 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md) 了解详情。

## 许可证

MIT License - 详见 [LICENSE](LICENSE)
