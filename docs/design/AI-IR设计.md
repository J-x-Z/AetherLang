# AI-IR 设计文档

> **版本**: 1.0-draft
> **状态**: 设计阶段
> **核心创新**: 为 AI 可读写而设计的中间表示层

---

## 概述

AI-IR (AI-Readable Intermediate Representation) 是 AetherLang 编译器中位于 AST 和传统 IR 之间的一个专门为 AI 设计的中间层。它不仅包含代码的结构信息，还包含丰富的语义、意图和约束信息，使 AI 能够：

1. **理解** - 查询代码的语义关系
2. **修改** - 在安全边界内优化代码
3. **验证** - 检查修改后的正确性

---

## 架构位置

```
Source Code (.aeth)
       ↓
   [Lexer] → Tokens
       ↓
   [Parser] → AST (Abstract Syntax Tree)
       ↓
   [Semantic Analyzer] → Typed AST + Contracts + Effects
       ↓
   [AI-IR Generator] → AI-IR  ◀━━ AI 可读写此层
       ↓
   [Lowering Pass] → Traditional IR (SSA)
       ↓
   [Backend] → C / LLVM / Native
```

---

## 数据结构设计

### 1. 顶层模块

```rust
/// AI-IR 模块：包含整个编译单元的 AI 可理解表示
pub struct AIIRModule {
    /// 模块名称
    pub name: String,
    
    /// 语义图：核心数据结构
    pub semantic_graph: SemanticGraph,
    
    /// 所有函数定义
    pub functions: Vec<AIIRFunction>,
    
    /// 所有类型定义
    pub types: Vec<AIIRType>,
    
    /// 全局约束
    pub global_constraints: Vec<Constraint>,
    
    /// 元数据
    pub metadata: ModuleMetadata,
}

pub struct ModuleMetadata {
    /// 严格性级别: prototype | production
    pub strictness_level: StrictnessLevel,
    
    /// 版本号 (用于迭代追踪)
    pub version: u64,
    
    /// 上次修改的 AI 模型标识
    pub last_modified_by: Option<String>,
}
```

### 2. 语义图 (Semantic Graph)

语义图是 AI-IR 的核心，表示代码中所有实体及其关系。

```rust
/// 语义图：节点和边的集合
pub struct SemanticGraph {
    /// 所有节点
    pub nodes: Vec<SemanticNode>,
    
    /// 所有边
    pub edges: Vec<SemanticEdge>,
    
    /// 节点索引 (快速查找)
    node_index: HashMap<NodeId, usize>,
}

/// 语义节点：代码中的一个实体
pub struct SemanticNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Span,
    pub attributes: NodeAttributes,
}

pub enum NodeKind {
    /// 函数节点
    Function {
        name: String,
        signature: FunctionSignature,
        effects: EffectSet,
        contracts: Contracts,
    },
    
    /// 类型节点
    Type {
        name: String,
        kind: TypeKind,  // struct, enum, alias
        invariants: Vec<Constraint>,
    },
    
    /// 变量节点
    Variable {
        name: String,
        ty: TypeRef,
        ownership: Ownership,
        lifetime: LifetimeRef,
    },
    
    /// 表达式节点
    Expression {
        kind: ExprKind,
        ty: TypeRef,
        value_range: Option<ValueRange>,  // 可能的值范围
    },
    
    /// 代码块节点
    Block {
        intent: Option<Intent>,  // 高层意图
        optimization_hints: Vec<OptHint>,
    },
}

/// 语义边：实体间的关系
pub struct SemanticEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

pub enum EdgeKind {
    /// 调用关系
    Calls,
    
    /// 数据流
    DataFlow {
        ownership_transfer: bool,
    },
    
    /// 控制流
    ControlFlow,
    
    /// 类型关系
    TypeOf,
    
    /// 依赖关系
    DependsOn,
    
    /// 实现关系
    Implements,
    
    /// 约束来源
    ConstrainedBy,
}
```

### 3. 意图层 (Intent Layer)

意图层捕获代码的高层目的，帮助 AI 理解"为什么"而非仅仅"是什么"。

```rust
/// 代码意图
pub struct Intent {
    /// 意图类型
    pub kind: IntentKind,
    
    /// 自然语言描述 (可选)
    pub description: Option<String>,
    
    /// 置信度 (0.0 - 1.0)
    pub confidence: f64,
}

pub enum IntentKind {
    // 数据处理意图
    Sort { ascending: bool },
    Filter { predicate_desc: String },
    Map { transform_desc: String },
    Reduce { operation_desc: String },
    
    // 控制流意图
    ErrorHandling,
    Validation,
    Initialization,
    Cleanup,
    
    // 性能相关意图
    CacheComputation,
    LazyEvaluation,
    Parallelizable,
    
    // 安全相关意图
    BoundsCheck,
    NullCheck,
    OwnershipTransfer,
    
    // 自定义意图
    Custom(String),
}
```

### 4. 约束层 (Constraint Layer)

约束层显式表示所有的编译期和运行期约束。

```rust
/// 约束
pub struct Constraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub source: ConstraintSource,
    pub verification: VerificationStrategy,
}

pub enum ConstraintKind {
    /// 前置条件
    Precondition(Expr),
    
    /// 后置条件
    Postcondition(Expr),
    
    /// 不变量
    Invariant(Expr),
    
    /// 类型约束
    TypeBound {
        type_param: String,
        bounds: Vec<TraitRef>,
    },
    
    /// 生命周期约束
    Lifetime {
        short: LifetimeRef,
        outlives: LifetimeRef,
    },
    
    /// 效果约束
    Effect {
        allowed: EffectSet,
    },
    
    /// 值范围约束
    ValueRange {
        variable: NodeId,
        min: Option<i64>,
        max: Option<i64>,
    },
}

pub enum ConstraintSource {
    /// 用户显式声明
    Explicit { span: Span },
    
    /// 编译器推导
    Inferred { reason: String },
    
    /// 从调用传播
    Propagated { from: NodeId },
}

pub enum VerificationStrategy {
    /// 静态验证
    Static,
    
    /// 运行时断言
    Runtime,
    
    /// 混合：尽可能静态，否则运行时
    Hybrid,
    
    /// 仅文档，不验证
    Documentation,
}
```

### 5. 优化提示层 (Optimization Hints Layer)

```rust
/// 优化提示
pub struct OptHint {
    pub kind: OptHintKind,
    pub target: NodeId,
    pub priority: u8,  // 0-255
}

pub enum OptHintKind {
    /// 热点代码
    Hotspot { estimated_calls: u64 },
    
    /// 性能瓶颈
    Bottleneck { issue: String },
    
    /// 可内联
    Inlinable,
    
    /// 可并行化
    Parallelizable { data_deps: Vec<NodeId> },
    
    /// 循环优化机会
    LoopOptimization { kind: LoopOptKind },
    
    /// 内存优化机会
    MemoryOptimization { kind: MemOptKind },
}

pub enum LoopOptKind {
    Unrollable { factor: usize },
    Vectorizable,
    LoopInvariantMotion,
    StrengthReduction,
}

pub enum MemOptKind {
    StackAllocatable,
    PoolAllocatable,
    CacheLineFriendly,
}
```

---

## AI 交互接口

### 1. 查询接口 (Query API)

```rust
impl AIIRModule {
    // === 基础查询 ===
    
    /// 获取函数的所有调用者
    pub fn get_callers(&self, func: NodeId) -> Vec<NodeId>;
    
    /// 获取函数的所有被调用函数
    pub fn get_callees(&self, func: NodeId) -> Vec<NodeId>;
    
    /// 获取变量的数据流
    pub fn get_dataflow(&self, var: NodeId) -> DataflowInfo;
    
    /// 获取变量的生命周期
    pub fn get_lifetime(&self, var: NodeId) -> LifetimeInfo;
    
    // === 约束查询 ===
    
    /// 获取某节点的所有约束
    pub fn get_constraints(&self, node: NodeId) -> Vec<&Constraint>;
    
    /// 检查约束是否满足
    pub fn check_constraint(&self, constraint: &Constraint) -> ConstraintResult;
    
    /// 获取约束冲突
    pub fn find_constraint_conflicts(&self) -> Vec<ConstraintConflict>;
    
    // === 类型查询 ===
    
    /// 获取类型的所有方法
    pub fn get_methods(&self, ty: TypeRef) -> Vec<NodeId>;
    
    /// 获取类型实现的所有 trait
    pub fn get_implemented_traits(&self, ty: TypeRef) -> Vec<TraitRef>;
    
    /// 查询类型的所有合法操作 (API 可发现性)
    pub fn get_available_operations(&self, ty: TypeRef) -> Vec<Operation>;
    
    // === 效果查询 ===
    
    /// 获取函数的效果
    pub fn get_effects(&self, func: NodeId) -> EffectSet;
    
    /// 检查效果兼容性
    pub fn check_effect_compatibility(&self, caller: NodeId, callee: NodeId) -> bool;
}
```

### 2. 修改接口 (Mutation API)

```rust
impl AIIRModule {
    // === 基础修改 ===
    
    /// 替换表达式
    pub fn replace_expression(
        &mut self, 
        target: NodeId, 
        replacement: AIIRExpr
    ) -> MutationResult;
    
    /// 内联函数调用
    pub fn inline_call(&mut self, call_site: NodeId) -> MutationResult;
    
    /// 提取表达式到变量
    pub fn extract_to_variable(
        &mut self, 
        expr: NodeId, 
        var_name: &str
    ) -> MutationResult;
    
    // === 重构操作 ===
    
    /// 重命名符号
    pub fn rename(&mut self, node: NodeId, new_name: &str) -> MutationResult;
    
    /// 提取函数
    pub fn extract_function(
        &mut self, 
        block: NodeId, 
        func_name: &str
    ) -> MutationResult;
    
    /// 移动代码
    pub fn move_code(
        &mut self, 
        source: NodeId, 
        target_location: Location
    ) -> MutationResult;
    
    // === 优化操作 ===
    
    /// 应用优化提示
    pub fn apply_optimization(&mut self, hint: &OptHint) -> MutationResult;
    
    /// 展开循环
    pub fn unroll_loop(&mut self, loop_node: NodeId, factor: usize) -> MutationResult;
}

pub struct MutationResult {
    pub success: bool,
    pub new_nodes: Vec<NodeId>,
    pub removed_nodes: Vec<NodeId>,
    pub constraint_violations: Vec<ConstraintViolation>,
    pub warnings: Vec<Warning>,
}
```

### 3. 验证接口 (Validation API)

```rust
impl AIIRModule {
    /// 完整验证
    pub fn validate(&self) -> ValidationResult;
    
    /// 类型检查
    pub fn check_types(&self) -> Vec<TypeError>;
    
    /// 约束检查
    pub fn check_constraints(&self) -> Vec<ConstraintViolation>;
    
    /// 借用检查
    pub fn check_borrows(&self) -> Vec<BorrowError>;
    
    /// 效果检查
    pub fn check_effects(&self) -> Vec<EffectError>;
    
    /// 增量验证 (仅检查修改部分)
    pub fn validate_incremental(&self, changed_nodes: &[NodeId]) -> ValidationResult;
}

pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub suggestions: Vec<FixSuggestion>,
}
```

---

## 序列化格式

AI-IR 支持多种序列化格式以便 AI 模型处理：

### JSON 格式 (用于 LLM)

```json
{
  "module": "example",
  "version": 1,
  "functions": [
    {
      "id": "fn_divide",
      "name": "divide",
      "signature": "fn -> i64 (numerator: i64, divisor: i64)",
      "effects": ["pure"],
      "contracts": {
        "requires": ["divisor != 0"],
        "ensures": ["result * divisor <= numerator"]
      },
      "intent": {
        "kind": "arithmetic",
        "description": "整数除法"
      }
    }
  ],
  "semantic_graph": {
    "nodes": [...],
    "edges": [...]
  }
}
```

### Protocol Buffer 格式 (用于高效传输)

```protobuf
message AIIRModule {
  string name = 1;
  uint64 version = 2;
  repeated AIIRFunction functions = 3;
  SemanticGraph semantic_graph = 4;
}
```

---

## 与迭代引擎的集成

```rust
/// 迭代引擎使用 AI-IR 进行安全优化
pub struct IterationEngine {
    /// 当前 AI-IR
    current_module: AIIRModule,
    
    /// 历史版本 (用于回滚)
    history: Vec<AIIRModule>,
    
    /// 沙箱环境
    sandbox: Sandbox,
    
    /// 审计日志
    audit_log: AuditLog,
}

impl IterationEngine {
    /// 执行一次迭代
    pub fn iterate(&mut self, mutation: Mutation) -> IterationResult {
        // 1. 在沙箱中应用修改
        let sandbox_module = self.sandbox.apply(mutation);
        
        // 2. 验证修改
        let validation = sandbox_module.validate();
        if !validation.is_valid {
            return IterationResult::Rejected(validation.errors);
        }
        
        // 3. 运行测试
        let test_result = self.sandbox.run_tests();
        if !test_result.passed {
            return IterationResult::TestFailed(test_result);
        }
        
        // 4. 记录审计日志
        self.audit_log.record(mutation, validation, test_result);
        
        // 5. 保存历史版本
        self.history.push(self.current_module.clone());
        
        // 6. 应用修改
        self.current_module = sandbox_module;
        
        IterationResult::Accepted
    }
    
    /// 回滚到上一个版本
    pub fn rollback(&mut self) -> bool {
        if let Some(prev) = self.history.pop() {
            self.current_module = prev;
            true
        } else {
            false
        }
    }
}
```

---

## 实现路线

### Phase 1: 核心数据结构
- [ ] 实现 `SemanticNode` 和 `SemanticEdge`
- [ ] 实现 `SemanticGraph` 及其索引
- [ ] 实现约束系统

### Phase 2: AST 到 AI-IR 转换
- [ ] 函数定义转换
- [ ] 类型定义转换
- [ ] 表达式和语句转换
- [ ] 约束收集和传播

### Phase 3: 查询接口
- [ ] 基础查询实现
- [ ] 约束查询实现
- [ ] 类型和效果查询

### Phase 4: 修改接口
- [ ] 基础修改操作
- [ ] 重构操作
- [ ] 增量验证

### Phase 5: 迭代引擎集成
- [ ] 沙箱环境
- [ ] 审计日志
- [ ] 版本控制

---

## 版本历史

| 版本 | 日期 | 变更 |
|------|------|------|
| 1.0-draft | 2026-01-13 | 初始设计 |
