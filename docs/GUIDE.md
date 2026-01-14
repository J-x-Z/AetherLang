# AetherLang 使用指南

> 从安装到编写第一个 AI-Native 程序

## 快速开始

### 1. 安装

```bash
git clone https://github.com/J-x-Z/AetherLang.git
cd AetherLang
cargo build --release
```

### 2. 你的第一个程序

创建 `hello.aeth`:
```aether
fn main() {
    println("Hello, AetherLang!")
}
```

编译运行:
```bash
cargo run -- hello.aeth --emit-c
gcc hello.c -o hello
./hello
```

---

## 基础语法

### 变量

```aether
let x = 42          // 不可变
let mut y = 10      // 可变
y = 20              // OK
```

### 函数

```aether
fn add(a: i32, b: i32) -> i32 {
    a + b  // 最后一个表达式自动返回
}

fn greet(name: str) {
    println("Hello, " + name)
}
```

### 控制流

```aether
// 条件
if x > 0 {
    println("positive")
} else {
    println("non-positive")
}

// 循环
for i in 0..10 {
    println_i64(i)
}

while x > 0 {
    x = x - 1
}
```

### 结构体

```aether
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p = Point { x: 10, y: 20 }
    println_i64(p.x)
}
```

---

## AI-Native 特性

### 契约编程

```aether
// 前置条件: 调用者必须保证 b != 0
fn divide(a: i32, b: i32) -> i32 [requires b != 0] {
    a / b
}

// 多个契约
fn binary_search(arr: i32, target: i32) -> i32 
    [requires arr != 0, requires target >= 0]
{
    // ...  
    return 0
}
```

### 纯函数

```aether
// 标记为 pure: 无副作用
fn add(a: i32, b: i32) -> i32 pure {
    a + b
}

// 编译器会检查: pure 函数不能调用 println 等副作用函数
```

### 效果标注

```aether
// 显式声明副作用
fn log(msg: str) effect[io] {
    println(msg)
}

fn allocate(size: i32) effect[alloc] {
    // 内存分配
}
```

### 所有权

```aether
// own: 转移所有权
fn consume(data: own String) {
    // data 在这里被消费
}

// ref: 借用 (只读)
fn read(data: ref String) {
    println(data)
}

// mut: 可变借用
fn modify(data: mut String) {
    data.push('!')
}

// shared: 共享所有权 (引用计数)
fn share(data: shared Handle) {
    // 多处共享
}
```

---

## 编译器选项

```bash
# 编译到 C 代码
cargo run -- input.aeth --emit-c

# 输出 IR
cargo run -- input.aeth --emit-ir

# 指定输出文件
cargo run -- input.aeth -o output.c

# 优化级别
cargo run -- input.aeth -O2
```

---

## 示例文件

查看 `examples/` 目录:

| 文件 | 描述 |
|------|------|
| `hello.aeth` | Hello World |
| `struct_test.aeth` | 结构体使用 |
| `ai_native_test.aeth` | AI-Native 特性演示 |
| `effect_test.aeth` | 效果系统演示 |

---

## 下一步

- 📖 [语言规范](docs/LANGUAGE.md) - 完整语法和类型系统
- 🤖 [AI-IR API](docs/design/ai-ir-api.md) - 为 AI 开发者

## 常见问题

**Q: 为什么要有 `pure` 标记?**
A: 帮助 AI 理解哪些函数无副作用，可以安全地重排序、缓存或并行化。

**Q: `requires` 和普通 `if` 检查有什么区别?**
A: `requires` 是编译器/AI 可理解的契约，可以被静态分析或自动验证。

**Q: 什么时候用 `shared` vs `ref`?**
A: `ref` 是临时借用，函数结束后失效；`shared` 是引用计数，可以长期持有。
