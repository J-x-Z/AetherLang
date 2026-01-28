# AetherLang 语言规范

> 📌 **主文档** - 这是 AetherLang 的**唯一完整规范**

## 目录

1. [词法规范](#1-词法规范)
2. [语法规范](#2-语法规范-ai-native)
3. [类型系统](#3-类型系统)
4. [AI-Native 特性](#4-ai-native-特性)

---

## 1. 词法规范

### 1.1 关键字

#### 核心关键字
| 关键字 | 用途 |
|--------|------|
| `fn` | 函数定义 |
| `let` | 变量绑定 |
| `mut` | 可变性 |
| `if` / `else` | 条件分支 |
| `loop` / `while` / `for` / `in` | 循环 |
| `return` | 返回 |
| `match` | 模式匹配 |
| `struct` / `enum` | 类型定义 |
| `impl` | 方法实现 |
| `interface` | 接口定义 |
| `const` | 常量 |
| `unsafe` | 不安全块 |
| `break` / `continue` | 控制流 |
| `true` / `false` | 布尔字面量 |
| `asm` | 内联汇编 |

#### AI-Native 新关键字 🆕
| 关键字 | 用途 |
|--------|------|
| `pub` | 公开可见性 |
| `type` / `trait` / `where` | 类型系统 |
| `own` / `ref` / `shared` | 所有权 |
| `pure` / `effect` | 效果系统 |
| `requires` / `ensures` / `invariant` | 契约编程 |

### 1.2 运算符

```
算术: + - * / %
比较: == != < <= > >=
逻辑: && || !
位运算: & | ^ ~ << >>
赋值: = += -= *= /=
其他: -> => .. :: @ ?
```

### 1.3 分隔符

```
( ) { } [ ] < >
, . : ; @
```

---

## 2. 语法规范 (AI-Native)

### 2.1 程序结构

```bnf
<program> ::= <item>*

<item> ::= <function>
         | <struct>
         | <enum>
         | <impl>
         | <interface>
         | <const>
```

### 2.2 函数定义

```bnf
<function> ::= <visibility>? "fn" <ident> "(" <params>? ")" 
               ("->" <type>)? 
               <contracts>?     # [requires ..., ensures ...]
               <effect>?        # pure | effect[...]
               <block>

<visibility> ::= "pub"
<contracts> ::= "[" <contract> ("," <contract>)* "]"
<contract> ::= ("requires" | "ensures" | "invariant") <expr>
<effect> ::= "pure" | "effect" "[" <effect_name> ("," <effect_name>)* "]"
<effect_name> ::= "io" | "alloc" | "read" | "write" | "panic"
```

**示例:**
```aether
// 带契约的纯函数
fn divide(a: i32, b: i32) -> i32 [requires b != 0] pure {
    a / b
}

// 带副作用标注
fn log(msg: str) effect[io] {
    println(msg)
}

// 公开函数
pub fn main() {}
```

### 2.3 类型定义

```bnf
<struct> ::= <visibility>? "struct" <ident> "{" <field>* "}"
<field> ::= <ident> ":" <type> ","?

<enum> ::= "enum" <ident> "{" <variant>* "}"
<variant> ::= <ident> ("(" <type> ("," <type>)* ")")?
```

### 2.4 类型语法

```bnf
<type> ::= <ident>                    # 命名类型 (i32, String)
         | ("own" | "shared") <type>  # 所有权类型 🆕
         | "*" <type>                 # 指针
         | "&" "mut"? <type>          # 引用
         | "[" <type> ";" <expr> "]"  # 数组
         | "[" <type> "]"             # 切片
         | "(" <type> ("," <type>)* ")"  # 元组
         | "()"                       # Unit
         | "!"                        # Never
```

### 2.5 语句

```bnf
<stmt> ::= "let" "mut"? <ident> ":" <type> ("=" <expr>)?   # P5.1: 类型标注必须
         | <expr>
         | "return" <expr>?
         | "break"
         | "continue"
```

> ⚠️ **P5.1 强制类型标注**: AetherLang 要求所有 `let` 语句必须显式标注类型。
> 这是为了减少 AI 幻觉，确保代码语义完全显式。

```aether
// ❌ 禁止 - 类型推断
let x = 10;

// ✅ 必须 - 显式类型
let x: i32 = 10;
let name: *u8 = "hello\0" as *u8;

// ❌ 禁止 - 闭包参数无类型
let f = |x| x + 1;

// ✅ 必须 - 闭包参数显式类型
let f: fn(i32) -> i32 = |x: i32| -> i32 { x + 1 };
```

### 2.6 表达式

```bnf
<expr> ::= <literal>
         | <ident>
         | <expr> <binop> <expr>
         | <unop> <expr>
         | <expr> "(" <args>? ")"      # 调用
         | <expr> "." <ident>          # 字段访问
         | <expr> "[" <expr> "]"       # 索引
         | "if" <expr> <block> ("else" <block>)?
         | "match" <expr> "{" <arm>* "}"
         | "loop" <block>
         | "while" <expr> <block>
         | "for" <ident> "in" <expr> <block>
         | "{" <stmt>* "}"             # 块
```

---

## 3. 类型系统

### 3.1 所有权模式

| 模式 | 关键字 | 语义 |
|------|--------|------|
| 所有权 | `own` | 值的所有权转移 |
| 不可变借用 | `ref`, `&` | 只读访问 |
| 可变借用 | `mut`, `&mut` | 读写访问 |
| 共享所有权 🆕 | `shared` | 引用计数共享 |

### 3.2 所有权规则

1. 每个值有且只有一个所有者
2. 所有者离开作用域时值被释放
3. 不可变借用可以有多个
4. 可变借用同时只能有一个
5. `shared` 使用引用计数

### 3.3 基本类型

| 类型 | 大小 | 描述 |
|------|------|------|
| `i8` / `i16` / `i32` / `i64` | 1-8 | 有符号整数 |
| `u8` / `u16` / `u32` / `u64` | 1-8 | 无符号整数 |
| `isize` / `usize` | 8 | 指针大小整数 |
| `f32` / `f64` | 4 / 8 | 浮点数 |
| `bool` | 1 | 布尔值 |
| `char` | 4 | Unicode 字符 |
| `()` | 0 | Unit 类型 |
| `!` | 0 | Never 类型 |

---

## 4. AI-Native 特性

### 4.1 契约编程

```aether
fn binary_search(arr: [i32], target: i32) -> i32 
    [requires arr.len() > 0, requires target >= 0]
{
    // 编译器验证前置条件
}
```

| 子句 | 用途 |
|------|------|
| `requires` | 前置条件 (调用者必须满足) |
| `ensures` | 后置条件 (函数保证) |
| `invariant` | 不变量 (始终成立) |

### 4.2 效果系统

```aether
// 纯函数 - 无副作用
fn add(a: i32, b: i32) -> i32 pure {
    a + b
}

// 显式副作用
fn write_file(path: str) effect[io, alloc] {
    // ...
}
```

> ⚠️ **P5.2 效果系统强制执行**: 调用有副作用的函数时，调用者必须声明相同或更大的效果集。
> 违反此规则会导致编译错误（硬错误），而非警告。

```aether
// ❌ 编译错误 - 调用 effect[io] 但未声明
fn bad_caller() {
    println("hello");  // Error: calling effect[io] without declaring it
}

// ✅ 正确 - 声明了 effect[io]
fn good_caller() effect[io] {
    println("hello");
}
```

**规则**: 纯函数不能调用产生副作用的函数。

| 效果 | 描述 |
|------|------|
| `io` | 输入/输出 |
| `alloc` | 内存分配 |
| `read` | 读取全局状态 |
| `write` | 写入全局状态 |
| `panic` | 可能 panic |

### 4.3 Result/Option 类型 (P5.3)

AetherLang 的 `Option<T>` 和 `Result<T, E>` 类型**故意不提供 `unwrap()` 方法**。
这是为了强制安全的错误处理，减少运行时 panic。

```aether
use result::{Option, Result}

// ❌ 不存在 - unwrap() 方法未定义
let value: i32 = maybe_value.unwrap();

// ✅ 使用 match
let value: i32 = match maybe_value {
    Option::Some(v) => v,
    Option::None => 0,  // 提供默认值
};

// ✅ 使用 unwrap_or
let value: i32 = maybe_value.unwrap_or(0);

// ✅ 使用 ? 操作符 (在返回 Result 的函数中)
let value: i32 = maybe_value?;
```

### 4.4 显式分配器 (P5.4)

AetherLang 容器类型使用显式分配器泛型参数：

```aether
use alloc::{Allocator, GlobalAllocator, ArenaAllocator}

// Vec 需要分配器参数
pub struct Vec<T, A: Allocator> {
    data: *T,
    len: u64,
    capacity: u64,
    alloc: A,
}

// 使用全局分配器
let v: Vec<i32, GlobalAllocator> = Vec::new_in(GlobalAllocator::new());

// 使用 Arena 分配器 (批量释放)
let arena: ArenaAllocator = ArenaAllocator::new(1024);
let v: Vec<i32, ArenaAllocator> = Vec::new_in(arena);
```

### 4.5 渐进式严格性

```aether
@prototype  // 宽松模式 - 允许警告
fn test() {}

@production // 严格模式 - 警告变错误
fn critical() {}
```

---

## 实现参考

| 模块 | 文件 |
|------|------|
| 词法分析 | `src/frontend/lexer.rs`, `src/frontend/token.rs` |
| 语法分析 | `src/frontend/parser.rs`, `src/frontend/ast.rs` |
| 语义分析 | `src/frontend/semantic.rs` |
| 类型系统 | `src/types/type_system.rs` |
| AI-IR | `src/ai_ir/` |

---

## 5. 相关设计文档

- [AI-IR 设计](design/ai_ir_design.md)
- [AI-IR API](design/ai_ir_api.md)
- [架构概览](design/architecture_overview.md)
- [上下文参数模式](context-pattern.md) (P5.5)
- [双层架构](design/DUAL_LAYER_ARCHITECTURE.md)

---

## ⚠️ 旧文档 (已废弃)

以下文档为旧版本，仅供参考：
- `词法规范.md` - 缺少 AI-Native 关键字
- `语法规范.md` - v1 语法，无契约/效果
- `类型系统.md` - 缺少 `shared` 所有权
- `语法规范v2.md` - 已合并到本文档
