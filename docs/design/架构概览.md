# AetherLang 架构概览

> 本文档描述 AetherLang 编译器的整体架构和模块职责。

## 1. 编译流程

```
源代码 (.aeth)
     ↓
┌─────────────────────────────────────┐
│            前端 (Frontend)           │
│  ┌─────────┐  ┌─────────┐  ┌──────┐ │
│  │  Lexer  │→ │ Parser  │→ │ Sema │ │
│  └─────────┘  └─────────┘  └──────┘ │
│     Token流      AST      类型化AST  │
└─────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────┐
│           中端 (Middle-end)          │
│  ┌─────────────┐  ┌───────────────┐ │
│  │  IR Gen     │→ │  Optimizer    │ │
│  └─────────────┘  └───────────────┘ │
│     Aether IR      优化后的 IR       │
└─────────────────────────────────────┘
                  ↓
┌─────────────────────────────────────┐
│            后端 (Backend)            │
│  ┌─────────────────────────────────┐│
│  │         CodeGen Trait           ││
│  └─────────────────────────────────┘│
│           ↓              ↓          │
│  ┌────────────┐   ┌─────────────┐   │
│  │ LLVM (v1.0)│   │ Native(v2.0)│   │
│  └────────────┘   └─────────────┘   │
└─────────────────────────────────────┘
                  ↓
            机器码 / 可执行文件
```

## 2. 模块职责

### 2.1 前端 (`src/frontend/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| **Token** | `token.rs` | Token 和 TokenKind 定义 |
| **Lexer** | `lexer.rs` | 源码 → Token 流 |
| **AST** | `ast.rs` | 抽象语法树定义 |
| **Parser** | `parser.rs` | Token 流 → AST |
| **Semantic** | `semantic.rs` | 类型检查、所有权分析 |

### 2.2 中端 (`src/middle/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| **IR** | `ir.rs` | Aether IR 数据结构 |
| **IR Gen** | `ir_gen.rs` | AST → IR 转换 |
| **Optimizer** | `optimize.rs` | DCE、常量折叠、CSE |

### 2.3 后端 (`src/backend/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| **CodeGen** | `codegen.rs` | 后端抽象 trait |
| **LLVM** | `llvm/*.rs` | LLVM 代码生成 |
| **Native** | `native/*.rs` | 自写后端 (v2.0) |

### 2.4 类型系统 (`src/types/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| **Type System** | `type_system.rs` | 类型定义和辅助方法 |

### 2.5 工具 (`src/utils/`)

| 模块 | 文件 | 职责 |
|------|------|------|
| **Span** | `span.rs` | 源码位置跟踪 |
| **Error** | `error.rs` | 错误类型和报告 |

## 3. 数据流

```
源码字符串
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
Vec<u8> (机器码)
    ↓ 写入文件
可执行文件
```

## 4. 关键设计决策

### 4.1 后端可插拔

通过 `CodeGen` trait 实现后端抽象：

```rust
pub trait CodeGen {
    fn generate(&mut self, module: &IRModule) -> Result<Vec<u8>>;
    fn target_triple(&self) -> &str;
    fn name(&self) -> &str;
}
```

这允许在 LLVM 和自写后端之间切换。

### 4.2 简化的所有权系统

- 无生命周期标注
- 三种模式：`own` / `ref` / `mut`
- 编译时基础检查 + 运行时边界检查

### 4.3 增量编译 (未来)

- 文件级别的增量编译
- 缓存已编译的 IR

## 5. 目录结构

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
│   ├── spec/           # 语言规范
│   ├── design/         # 设计文档
│   └── dev/            # 开发日志
├── stdlib/             # 标准库 (AetherLang 源码)
├── tests/              # 测试用例
└── examples/           # 示例代码
```

## 6. 测试策略

| 层级 | 测试类型 | 位置 |
|------|----------|------|
| 单元测试 | Lexer, Parser, Semantic | 各模块内 `#[cfg(test)]` |
| 集成测试 | 端到端编译 | `tests/` 目录 |
| 示例测试 | 验证示例可运行 | `examples/` 目录 |

## 7. 相关文档

- [词法规范](./spec/词法规范.md)
- [语法规范](./spec/语法规范.md)
- [类型系统](./spec/类型系统.md)
