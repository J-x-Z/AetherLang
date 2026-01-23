# AetherLang 项目记忆

> 这个文件供 AI 助手读取，保存项目上下文和待办事项

## 项目概述

AetherLang 是一个 **AI 原生系统编程语言**，目标是：
1. 减少 AI 幻觉 - 显式接口、约束语法、语义标注
2. 支持 AI 自迭代 - AI 可读 IR、结构化反馈、沙盒优化
3. 保持严谨安全 - 契约编程、效果系统、所有权语义

## 编译流程

```
Source (.aeth)
     ↓
Frontend: Lexer → Parser → Semantic
     ↓
Middle: IR Gen → Optimizer
     ↓
Backend: LLVM (主力) / C (备用)
     ↓
Native Binary
```

## 自举状态

| 模块 | 文件 | 状态 |
|------|------|------|
| Lexer | `src_aether/lexer.aeth` | ✅ 完成 |
| Parser | `src_aether/parser.aeth` | ✅ 完成 |
| Semantic | `src_aether/semantic.aeth` | ✅ 完成 |
| IR Gen | `src_aether/ir_gen.aeth` | ⚠️ 框架完成，需连接 AST |
| IR→LLVM | `src_aether/ir_to_llvm.aeth` | ⚠️ 框架完成，需连接 IR |
| Codegen | `src_aether/codegen.aeth` | ✅ 完成 |

## 待办事项 (优先级排序)

### P0: ✅ 已完成 - AST 到 IR 连接

- [x] `ir_gen.aeth` AST 连接函数
  - [x] `generate_block_from_ast` - 遍历 `StmtList`
  - [x] `generate_stmt_from_ast` - 处理各种语句
  - [x] `generate_expr_from_ast` - 递归处理表达式
  - [x] `Scope` 表 - 变量名 → 寄存器映射

- [x] `ir_to_llvm.aeth` IR 连接
  - [x] `ValueMap` - register id → LLVM value
  - [x] `BlockMap` - block id → LLVM basic block
  - [x] `set_value/get_value`, `set_block/get_block`

- [x] jxz 包管理器连接
  - [x] `cmd_build()` → `build_project()`
  - [x] `cmd_init()` → `init_project()`
  - [x] `cmd_run()` → `run_project()`

---

### P1: 🔧 Kernel 开发支持 (当前)

实现顺序：

- [x] **1. `#[repr(C)]` 属性** - C 兼容内存布局 ✅
  - [x] AST: 添加 `Attribute`, `AttributeList`, `ReprKind`, `AttrKind`
  - [x] AST: `StructDef`, `EnumDef`, `Function` 添加 `attrs` 字段
  - [x] Parser: 添加 `Hash`, `LBracket`, `RBracket` token
  - [x] Parser: 实现 `parse_attribute()` 和 `parse_attributes()`
  - [x] Parser: `parse_struct()` 和 `parse_enum()` 调用属性解析
  - [x] 测试: 编译 `tests/repr_c_test.aeth` 通过
  - [x] Codegen: `ir_to_llvm.aeth` 中根据 `repr(C)` 生成 packed struct

- [x] **2. `asm!` 宏** - 内联汇编 ✅
  - [x] IR: `Instruction::InlineAsm` 已存在 (`src/middle/ir.rs:232`)
  - [x] IR Gen: 已实现 (`src/middle/ir_gen.rs:1737`)
  - [x] C 后端: 已实现 (`src/backend/c/c_codegen.rs:672`)
  - [x] LLVM 后端: 已实现 (`src/backend/llvm/llvm_codegen.rs`)
  - [ ] 自举 Parser: `parse_asm!()` (延后，当前自举不需要)

- [x] **3. `#[naked]` 属性** - 裸函数 ✅
  - [x] Parser: 属性解析已完成 (复用 `parse_attributes()`)
  - [x] AST: `Function.attrs` 和 `has_naked()` 已实现
  - [x] IR: `IRFunction.naked: bool` 已存在
  - [x] Codegen (Rust): 设置 LLVM `naked` 函数属性
  - [x] Codegen (Rust): 同时实现 `interrupt` 属性
  - [ ] Codegen (自举): `ir_to_llvm.aeth` 添加 naked 支持 (延后)

- [x] **4. volatile 读写** ✅
  - [x] AST: `Type::Volatile` 已存在
  - [x] Token: `volatile` 关键字已存在
  - [x] IR: `IRFunction.volatile: bool` 已存在
  - [x] Codegen (LLVM): `LLVMSetVolatile()` 设置 load/store 为 volatile

- [x] **5. `#![no_std]` 模块属性** ✅ (已存在)
  - [x] AST: `Program.inner_attrs` 存储内部属性
  - [x] Parser: `parse_inner_attribute()` 解析 `#![...]`
  - [x] IR Gen: 设置 `module.no_std` 并跳过 C 库注册
  - [x] C 后端: 跳过运行时函数

---

### P1: ✅ Kernel 开发支持 - 全部完成!

### P2: 📐 高级数学计算

- [x] **1. Const Generics** - 编译时常量泛型 ✅
  - [x] AST: `GenericParam` 枚举 (Type/Const)
  - [x] AST: `GenericArg` 枚举 (Type/Const)
  - [x] AST: `StructDef.generic_params`, `EnumDef.generic_params`
  - [x] AST: `Type::GenericWithArgs` 变体 (支持混合类型和 const 参数)
  - [x] Parser: `parse_generic_params()` 支持 `<T, const N: usize>`
  - [x] Parser: `parse_generic_arg()` 支持 `Matrix<i32, 3, 3>`
  - [x] 类型系统: `eval_const_expr()` 常量表达式求值
  - [x] 类型系统: `ResolvedType::GenericWithConsts` 泛型实例化
  - [x] Semantic: `SymbolKind` 添加 `const_params` 字段
  - [x] Semantic: `collect_definition()` 提取 const params
  - [x] Codegen: `try_eval_const_expr()` 编译时求值
  - [x] Codegen: 单态化名称修饰 `Matrix<f32, 3, 3>` → `Matrix_f32_3_3`

- [x] **2. SIMD 类型** ✅
  - [x] `f32x4`, `f64x2`, `i32x8` 内建类型
  - [x] LLVM vector type 映射 (`LLVMVectorType`)
  - [x] `#[simd]` 函数标注 (AVX2/SSE4.2 + fast-math)
  - [x] 升级 llvm-sys 到 v211 (LLVM 21)

- [ ] **3. BLAS FFI**
  - [ ] OpenBLAS/MKL 绑定生成

---

### P3: 🤖 AI / GPU 计算

- [ ] **1. CUDA FFI**
  - [ ] `extern "CUDA"` 块
  - [ ] `#[kernel]` 函数标记

- [ ] **2. `Tensor<T, Shape>` 类型**
  - [ ] 形状推断
  - [ ] 广播规则

- [ ] **3. Autodiff**
  - [ ] 反向传播 IR 转换

---

### P4: jxz 包管理器增强

| 命令 | 状态 |
|------|------|
| `init` | ✅ 完成 |
| `build` | ✅ 完成 |
| `run` | ✅ 完成 |
| `test` | ❌ 空壳 |
| `install` | ✅ 完整 |
| `add/remove` | ❌ 空壳 |

- [ ] 解析 `Jxz.toml` 获取项目配置
- [ ] 实现依赖解析

## 关键文件

| 文件 | 用途 |
|------|------|
| `src_aether/ir_gen.aeth` | IR 生成器 (自举) |
| `src_aether/ir_to_llvm.aeth` | IR → LLVM 转换 |
| `src_aether/codegen.aeth` | LLVM 代码生成封装 |
| `src_aether/ast.c` | AST 结构定义 (C 生成) |
| `aethc-bootstrap` | 引导编译器 |
| `jxz/src/main.aeth` | 包管理器入口 |

## 已完成的修复 (2026-01-22)

Antigravity 挂掉前正在做的任务，已由 Claude 接手完成：

### ir_gen.aeth
- ✅ `generate_block_with_stmts()` - 块语句遍历
- ✅ `generate_stmt_kind()` - 语句类型分发
- ✅ `generate_match()` - 模式匹配 (比较链)
- ✅ `generate_loop()` - 无限循环
- ✅ `generate_for_loop()` - for-in 循环
- ✅ `generate_break()` / `generate_continue()` - 循环控制
- ✅ `generate_array_literal()` - 数组字面量
- ✅ `generate_closure()` - 闭包 (环境捕获)
- ✅ 新增字段: `loop_exit_block`, `loop_cond_block`

### ir_to_llvm.aeth
- ✅ `gen_while()` - while 循环
- ✅ `gen_loop()` - 无限循环
- ✅ `gen_for()` - for 循环
- ✅ `gen_match()` - match 表达式
- ✅ `gen_if()` - if 表达式
- ✅ `gen_closure()` - 闭包
- ✅ `gen_closure_call()` - 闭包调用
- ✅ `gen_array()` - 数组字面量
- ✅ `gen_array_index()` - 数组索引

## 注意事项

1. **目标后端是 LLVM**，不是 C 后端
2. **自举优先** - 修改 `src_aether/*.aeth`，不是 `src/*.rs`
3. 编译测试用 `./aethc-bootstrap <file>.aeth`
