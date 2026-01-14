# AetherLang 后续开发项目书

> **用途**: 在其他平台继续开发时的参考文档
> **生成时间**: 2026-01-14
> **当前状态**: Phase 1-5 完成, 35 测试通过

---

## 1. 项目概述

AetherLang 是一门 **AI-Native 系统编程语言**，专门设计来：
1. **减少 AI 幻觉** - 通过 API 可发现性和约束语法
2. **提升容错性** - 通过智能错误恢复和契约编程
3. **支持 AI 自迭代** - 通过可迭代区域和 AI-IR 接口

---

- ✅ 契约子句 `[requires ..., ensures ...]`
- ✅ 效果标注 `pure`, `effect[io, alloc]`
- ✅ 所有权类型 `own T`, `shared T`
- ✅ 可见性 `pub`

---

## 3. 待完成内容 (按优先级)

### 高优先级: 核心 AI-Native 特性

#### Phase A: API 可发现性
```rust
// 目标: 实现这个查询
module.get_available_operations(type_id) -> Vec<Operation>
```
- 扩展 `SemanticGraph` 存储方法-类型关联
- JSON 格式输出操作列表
- 让 AI 能"看见"合法操作

#### Phase B: 智能错误恢复
- `ErrorReport` 支持多个 `Suggestion`
- Parser 错误恢复 (部分解析)
- `result` 变量支持 (ensures 契约)
- 精化类型 `NonZero: type = i64 [invariant self != 0]`

#### Phase C: AI 自迭代
- `@optimizable` 注解 (可迭代区域)
- AI-IR Mutation API (`replace_expression`, `inline_call`)
- AI-IR Validation API (`validate`, `validate_incremental`)

### 中优先级: 阶段 6-7

#### 阶段 6: 后端适配
- 契约断言生成 (C 后端 `assert()`)
- 效果标注处理

#### 阶段 7: 工具链
- Trait 系统 (特征定义/实现/自动派生)
- 宏系统 (声明式 + 过程式)
- 模块系统 / 包管理
- LSP 支持

### 低优先级: 后端切换

#### LLVM 后端
- 替换 C 后端
- 需要 LLVM 21+ 环境
- 现有 `llvm-sys` 依赖 (optional feature)

#### 自研后端
- 自主研发代码生成器
- 目标: x86_64, ARM, RISC-V

---

## 4. 关键文件位置

```
src/
├── frontend/          # 前端
│   ├── lexer.rs       # 词法分析
│   ├── parser.rs      # 语法分析
│   ├── ast.rs         # AST 定义
│   └── semantic.rs    # 语义分析
├── middle/            # 中端
│   ├── ir.rs          # IR 定义
│   ├── ir_gen.rs      # IR 生成
│   └── optimize.rs    # 优化器
├── backend/           # 后端
│   └── c_codegen.rs   # C 代码生成
├── ai_ir/             # AI-IR 层 (核心创新)
│   ├── mod.rs
│   ├── semantic_graph.rs
│   ├── intent.rs
│   ├── constraint.rs
│   ├── query.rs
│   └── converter.rs
├── feedback/          # 反馈模块
│   ├── mod.rs
│   └── iteration.rs
├── types/             # 类型系统
└── utils/             # 工具
    └── error.rs       # 错误定义
```

---

## 5. 参考文档

| 文档 | 路径 | 内容 |
|------|------|------|
| 项目书 | `项目书方向补充.txt` | 三大支柱设计理念 |
| 语法详细 | `语法详细补充.txt` | 836 行详细语法设计 |
| 语言规范 | `docs/LANGUAGE.md` | 语法/类型/AI-Native |
| 使用指南 | `docs/GUIDE.md` | 快速开始 |
| AI-IR API | `docs/design/ai-ir-api.md` | Query/Mutation API |

---

## 6. 构建和测试

```bash
# 构建
cargo build --release

# 测试 (35 tests)
cargo test

# 编译示例
cargo run -- examples/ai_native_test.aeth --emit-c

# 启用 LLVM (需要 LLVM 21)
cargo build --features llvm
```

---

## 7. 依赖

```toml
[dependencies]
anyhow = "1.0"
thiserror = "1.0"
clap = { version = "4.0", features = ["derive"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
llvm-sys = { version = "211", optional = true }
```

---

## 8. 下一步建议

1. **先完成 Phase A-C** - 核心 AI-Native 差异化功能
2. **再完成阶段 6-7** - 工具链完善
3. **最后后端切换** - LLVM/自研后端

**关键成功指标**:
- AI 生成代码编译通过率 > 80%
- 迭代优化收敛时间 < 10 轮
