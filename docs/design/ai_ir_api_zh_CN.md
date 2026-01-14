# AI-IR API 参考

> AI-Readable Intermediate Representation - 为 AI 设计的语义层

## 概述

AI-IR 是 AetherLang 的核心创新，位于 AST 和传统 IR 之间，专门为 AI 模型设计。

```
Source Code → AST → AI-IR → Traditional IR → Machine Code
                      ↑
               AI reads/writes here
```

## 核心类型

### NodeId / EdgeId
```rust
pub struct NodeId(pub usize);
pub struct EdgeId(pub usize);
```

### AIIRModule
```rust
pub struct AIIRModule {
    pub name: String,
    pub graph: SemanticGraph,      // 语义图
    pub constraints: Vec<Constraint>, // 约束
    pub hints: Vec<OptimizationHint>, // 优化提示
}
```

---

## SemanticGraph

语义图包含 **节点 (Nodes)** 和 **边 (Edges)**。

### NodeKind (节点类型)

| 类型 | 描述 | 字段 |
|------|------|------|
| `Function` | 函数定义 | params, return_type, effects, is_pure |
| `Type` | 类型定义 | type_kind, fields |
| `Variable` | 变量 | type_name, ownership, is_mutable |
| `Expression` | 表达式 | expr_kind, type_name |
| `Block` | 代码块 | stmt_count |

### EdgeKind (边类型)

| 类型 | 描述 |
|------|------|
| `Calls` | 函数调用关系 |
| `DataFlow` | 数据流 (ownership_transfer) |
| `ControlFlow` | 控制流 |
| `TypeOf` | 类型关系 |
| `DependsOn` | 依赖关系 |
| `Borrows` | 借用关系 (mutable) |

---

## Query API

### 关系查询

```rust
// 获取调用者
let callers = module.get_callers(func_id);
for caller in callers.callers {
    println!("Called by: {:?}", module.get_node(caller));
}

// 获取被调用者
let callees = module.get_callees(func_id);

// 数据流分析
let dataflow = module.get_dataflow(node_id);
// dataflow.sources - 数据来源
// dataflow.sinks   - 数据去向
```

### 类型查询

```rust
// 获取节点类型
let type_id = module.get_type_of(node_id);

// 获取某类型的所有实例
let instances = module.nodes_of_type("Point");
```

### 约束查询

```rust
// 获取前置条件
let preconditions = module.get_preconditions(func_id);

// 获取后置条件
let postconditions = module.get_postconditions(func_id);

// 获取所有约束
let constraints = module.get_constraints(node_id);
```

### 统计摘要

```rust
let summary = module.summary();
println!("Nodes: {}", summary.node_count);
println!("Edges: {}", summary.edge_count);
println!("Functions: {}", summary.function_count);
println!("Constraints: {}", summary.constraint_count);
```

---

## Intent Layer

高层意图标注，帮助 AI 理解代码目的。

### IntentKind

| 类别 | 意图 |
|------|------|
| **数据处理** | Sort, Filter, Map, Reduce, Search |
| **控制流** | ErrorHandling, Validation, Initialization, Cleanup, Retry |
| **性能** | Cache, LazyEval, Parallel, Batch |
| **安全** | BoundsCheck, NullCheck, OwnershipTransfer |
| **I/O** | Read, Write, Network |

```rust
let intent = Intent::new(IntentKind::Sort { ascending: true });
node.intent = Some(intent);
```

---

## Constraint Layer

约束层管理显式和推导的约束。

### ConstraintKind

| 类型 | 来源 | 示例 |
|------|------|------|
| `Precondition` | `requires` 子句 | `b != 0` |
| `Postcondition` | `ensures` 子句 | `result >= 0` |
| `Invariant` | 类型/循环不变量 | `len > 0` |
| `TypeBound` | 类型参数约束 | `T: Clone` |
| `Effect` | 效果约束 | `[io, alloc]` |

### VerificationStrategy

- `Static` - 编译时验证
- `Runtime` - 运行时断言
- `Hybrid` - 先尝试静态，回退到运行时
- `Documentation` - 仅文档，不验证

---

## 使用示例

### 转换 AST 到 AI-IR

```rust
use crate::ai_ir::AIIRConverter;

let converter = AIIRConverter::new("my_module".to_string());
let ai_ir = converter.convert(&program);

// 查询
println!("Functions: {:?}", ai_ir.graph.functions().len());
println!("Constraints: {:?}", ai_ir.constraints.len());
```

### 分析调用图

```rust
for func in ai_ir.graph.functions() {
    let callers = ai_ir.get_callers(func.id);
    let callees = ai_ir.get_callees(func.id);
    
    println!("{}: {} callers, {} callees", 
        func.name, 
        callers.callers.len(),
        callees.callees.len()
    );
}
```

---

## 设计原则

1. **AI-First** - 所有 API 设计优先考虑 AI 可理解性
2. **Explicit > Implicit** - 显式信息优于隐式推断
3. **Rich Metadata** - 携带丰富的语义元数据
4. **Query-Friendly** - 高效的查询接口
5. **Immutable by Default** - 默认不可变，mutation 需显式请求
