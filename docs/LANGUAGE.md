# AetherLang 语言规范

> 语言设计与词法规则。各能力的实际实现状态见 [主线任务](../MAINLINE_TASKS.md)；1.4节转义规则及下文C ABI声明与定义规则已通过端到端验证，其他章节不能据此视为全部实现。

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

### 1.4 字符串与字符转义（2026-11-15实现）

`.aeth`和`.ath`的双引号字符串使用同一转义规则，存储UTF-8字节。`.aeth`的单引号字符表示一个Unicode scalar，类型`char`为32位；`'a`/`'static`等无结束引号的名字仍是生命周期记号。自举Core支持下表字符串子集，不新增字符字面量语法。

| 写法 | 值 |
| --- | --- |
| `\n`、`\r`、`\t` | LF、CR、TAB |
| `\\`、`\"`、`\'` | 反斜杠、双引号、单引号 |
| `\0` | 单个NUL；不继续吞入后面的数字 |
| `\xHH` | 恰好两位十六进制，值为ASCII `00..7F` |
| `\u{H...}` | 1至6位十六进制Unicode scalar；不允许下划线，最大`10FFFF`，拒绝`D800..DFFF`代理值 |

未知转义、缺失数字/括号、不完整输入和范围错误均产生编译诊断，不再丢弃反斜杠。十六进制数字可大小写混用。例如`"\x1b"`包含一个ESC字节；`"\x41F"`包含`41 46`两个字节；`"\07"`包含`00 37`；`"\u{4e2d}"`包含UTF-8的`E4 B8 AD`，而`'\u{4e2d}'`的标量值为20013。

`\u{...}`的值先解码为scalar，再编码为UTF-8；`\x80..\xFF`不是任意字节后门，应使用数值`u8`数组/缓冲区表达二进制数据。原始Unicode源字符与其对应Unicode转义具有相同值。字符串可以包含NUL，但调用C字符串接口时仍受该接口的终止规则约束；写二进制数据应显式传长度。

字符可用`as`显式转为整数：值为Unicode码点，窄化遵循整数转换的截断规则；常量与运行时一致。整数不能通过未经校验的`as char`构造字符，也不会隐式当作char使用。

真实验证入口为`tests/literal_escapes.py`：比较C/LLVM的O0/O2/O3实际输出字节、字符值和Script输出，并检查非法转义的源文件位置；`--core`可比较自举Core字符串子集。

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

// 函数值和闭包由当前基础编译器在检查阶段报 E2403。
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

### 3.1 值、借用和资源

数组、元组和具名结构按值传递。字段中的裸指针保留地址，复制含裸指针的记录可能产生资源别名。引用的初始化、投影及逃逸受到检查。

`own Record` 为显式选择的资源建立移动语义。由手工管理值接管所有权，以及把 owner 交回手工 API，需要 `unsafe`。确切的可组合范围、析构策略与迁移例见[迁移说明](MIGRATION.md)。`shared` 尚无引用计数实现。

### 3.3 基本类型

| 类型 | 大小 | 描述 |
|------|------|------|
| `i8` / `i16` / `i32` / `i64` | 1-8 | 有符号整数 |
| `u8` / `u16` / `u32` / `u64` | 1-8 | 无符号整数 |
| `isize` / `usize` | 4 或 8 | 随目标指针宽度变化 |
| `f32` / `f64` | 4 / 8 | 无类型上下文的浮点字面量默认为 `f64`。赋值、参数、返回值或另一运算数可指定 `f32`，每步运算均按该精度舍入。 |
| `bool` | 1 | 布尔值 |
| `char` | 4 | Unicode 字符 |
| `()` | 0 | Unit 类型 |
| `!` | 0 | Never 类型 |

---

## 4. AI-Native 特性

### 4.1 效果与契约

```aether
fn increment(n:i32)->i32 [requires n>0, ensures result==n+1] pure { n+1 }
```

`pure` 要求没有可观察副作用，`effect[...]` 约束推断效果的上界。未标注的普通函数按函数体与实际调用推断效果。契约谓词必须纯且返回 bool。入口检查前置条件，每个正常出口检查后置条件，返回表达式只求值一次。失败触发 trap。函数上的 `invariant` 会在检查阶段被拒绝。

### 4.2 错误值、容器与分配

从 `option` 导入 `Option`，从 `result` 导入 `Result`。使用模式匹配或 `option_or`、`result_or`。返回 `Result` 的函数可用 `?` 传播相同错误类型。泛型向量使用 `Vec<T>`，通过 `vec_new`、`vec_push`、`vec_free` 管理。独立的 arena/stack API 见[分配器源码](../stdlib/alloc.aeth)。

## C ABI 声明与定义（2026-11-15实现）

主编译器允许多个模块声明同一 `extern "C"` 函数，并由整个程序中唯一一个
`#[no_mangle]` 非泛型函数提供实现。两者共享链接符号；普通函数仍使用模块私有的链接名。
这可以用于模块之间的回调接口，不需要为了导入实现而制造循环模块依赖。

```aether
extern "C" { fn native_callback(input: i32) -> i32; }
#[no_mangle]
pub fn native_callback(value: i32) -> i32 { value + 1 }
fn main() -> i32 { native_callback(1) - 2 }
```

声明与定义的参数个数、解析后的类型、返回类型和变参属性必须完全一致；参数名不参与比较。
类型别名与其目标等价，名义结构体即使布局相同仍是不同类型。省略返回类型和 `-> ()`
均表示 C 的 `void`。当前只支持 C ABI，Aether 函数体不能定义变参函数。
不相容声明、函数/静态变量重名、重复函数体及泛型 `#[no_mangle]` 导出会被拒绝。
已知函数体的调用效果由真实实现推断，extern 注解不能遮盖实现的效果。

此能力属于 Rust 实现的主编译器；最小自举 Core 的支持子集见 [BOOTSTRAP.md](BOOTSTRAP.md)，
不因主编译器支持而自动扩大。C ABI 目标覆盖与已知限制仍以主线 T07 为准。

普通私有函数、私有静态存储具有对象本地链接。要由其他对象访问稳定名称的存储，写 `#[no_mangle] static mut NAME:T=...;` 并使用相容的 C 声明。原有 `pub static` 保留模块修饰名。多个模块可用 `usize` 与目标同宽无符号类型声明同一个 C 符号。语义检查保留每处实际导入的类型，普通 Aether 函数仍区分两种类型。

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

### 本地包入口、泛型检查与元组

`--module name=/absolute/path/lib.aeth` 把名称绑定到一个具体源文件。CLI 与 LSP 使用同一解析器；入口内部的相对导入从真实文件所在目录查找。本地模块与显式包入口重名且指向不同源时，编译器报告歧义。

泛型函数按使用到的具体类型实例化，函数体类型错误在实例化时报告。未调用模板的函数体仍须通过语法解析。相同类型参数的递归可以复用原实例；无限增长的类型和递归别名触发有源位置的诊断。

`(x,)` 是单元素元组，`(x)` 是括号表达式。元素从左到右各求值一次，`p.0` 与 `p.1.0` 访问对应位置。赋值复制值，引用保留借用约束。普通 Aether 函数可以传递和返回元组；C 互操作请使用具名且布局兼容的结构体。捕获闭包和函数值当前在语义检查阶段报 E2403。

### 可释放的库资源

导入 `owned_vec` 后，`own Vec<T>` 通过 `owned_vec_new` 创建，可借用传给 `owned_vec_push/get/len`。每个实际使用的 T 都有唯一的析构函数，作用域结束时释放存储。裸 `Vec<T>` 继续通过 `vec_free` 释放。泛型 `@drop` 接受包含全部类型或常量参数的直接结构体实例，每个结构体族可注册一个通用析构。常量参数的种类、位宽和顺序须匹配。与具体析构重合的实例在检查阶段报 E2405。

`owned_tensor` 和 `owned_quantum` 分别提供 `own TensorHandle` 和 `own LeaseHandle`。创建空句柄后，通过可变借用取得资源。重新申请失败会保留现有资源；提前返回或使用 `?` 返回错误会清理所有已取得的资源。借用所有者时，通过显式 `unsafe` 才能取得原始指针或复制含指针的原始字段。原始 ABI 和内存接口仍由调用者保证生存期。当前支持将 owner 作为参数或返回值，嵌入聚合的 owner（例如 `Result<own T, E>`）仍报 E2405。
