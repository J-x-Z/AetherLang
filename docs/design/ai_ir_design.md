# AI-IR Design Document

> **Version**: 1.0-draft
> **Status**: Design Phase
> **Core Innovation**: Intermediate Representation designed for AI Readability

---

## Overview

AI-IR (AI-Readable Intermediate Representation) is a specialized intermediate layer in the AetherLang compiler, sitting between the AST and traditional IR. It captures not just the structure of the code, but also rich semantics, intent, and constraints, enabling AI to:

1. **Understand** - Query semantic relationships in the code.
2. **Modify** - Optimize code within safe boundaries.
3. **Verify** - Check the correctness of modifications.

---

## Architectural Position

```
Source Code (.aeth)
       ↓
   [Lexer] → Tokens
       ↓
   [Parser] → AST (Abstract Syntax Tree)
       ↓
   [Semantic Analyzer] → Typed AST + Contracts + Effects
       ↓
   [AI-IR Generator] → AI-IR  ◀━━ AI Read/Write Layer
       ↓
   [Lowering Pass] → Traditional IR (SSA)
       ↓
   [Backend] → C / LLVM / Native
```

---

## Data Structure Design

### 1. Top-Level Module

```rust
/// AI-IR Module: AI-understandable representation of a compilation unit
pub struct AIIRModule {
    /// Module name
    pub name: String,
    
    /// Semantic Graph: Core data structure
    pub semantic_graph: SemanticGraph,
    
    /// All function definitions
    pub functions: Vec<AIIRFunction>,
    
    /// All type definitions
    pub types: Vec<AIIRType>,
    
    /// Global constraints
    pub global_constraints: Vec<Constraint>,
    
    /// Metadata
    pub metadata: ModuleMetadata,
}

pub struct ModuleMetadata {
    /// Strictness level: prototype | production
    pub strictness_level: StrictnessLevel,
    
    /// Version number (for iteration tracking)
    pub version: u64,
    
    /// Identifier of the AI model that last modified this
    pub last_modified_by: Option<String>,
}
```

### 2. Semantic Graph

The Semantic Graph is the core of AI-IR, representing all entities and their relationships in the code.

```rust
/// Semantic Graph: Collection of nodes and edges
pub struct SemanticGraph {
    /// All nodes
    pub nodes: Vec<SemanticNode>,
    
    /// All edges
    pub edges: Vec<SemanticEdge>,
    
    /// Node index (fast lookup)
    node_index: HashMap<NodeId, usize>,
}

/// Semantic Node: An entity in the code
pub struct SemanticNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Span,
    pub attributes: NodeAttributes,
}

pub enum NodeKind {
    /// Function Node
    Function {
        name: String,
        signature: FunctionSignature,
        effects: EffectSet,
        contracts: Contracts,
    },
    
    /// Type Node
    Type {
        name: String,
        kind: TypeKind,  // struct, enum, alias
        invariants: Vec<Constraint>,
    },
    
    /// Variable Node
    Variable {
        name: String,
        ty: TypeRef,
        ownership: Ownership,
        lifetime: LifetimeRef,
    },
    
    /// Expression Node
    Expression {
        kind: ExprKind,
        ty: TypeRef,
        value_range: Option<ValueRange>,  // Possible value range
    },
    
    /// Block Node
    Block {
        intent: Option<Intent>,  // High-level intent
        optimization_hints: Vec<OptHint>,
    },
}

/// Semantic Edge: Relationship between entities
pub struct SemanticEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

pub enum EdgeKind {
    /// Call relationship
    Calls,
    
    /// Data flow
    DataFlow {
        ownership_transfer: bool,
    },
    
    /// Control flow
    ControlFlow,
    
    /// Type relationship
    TypeOf,
    
    /// Dependency relationship
    DependsOn,
    
    /// Implementation relationship
    Implements,
    
    /// Constraint source
    ConstrainedBy,
}
```

### 3. Intent Layer

The Intent Layer captures the high-level purpose of the code, helping AI understand "why" rather than just "what".

```rust
/// Code Intent
pub struct Intent {
    /// Intent type
    pub kind: IntentKind,
    
    /// Natural language description (optional)
    pub description: Option<String>,
    
    /// Confidence (0.0 - 1.0)
    pub confidence: f64,
}

pub enum IntentKind {
    // Data Processing Intents
    Sort { ascending: bool },
    Filter { predicate_desc: String },
    Map { transform_desc: String },
    Reduce { operation_desc: String },
    
    // Control Flow Intents
    ErrorHandling,
    Validation,
    Initialization,
    Cleanup,
    
    // Performance Intents
    CacheComputation,
    LazyEvaluation,
    Parallelizable,
    
    // Safety Intents
    BoundsCheck,
    NullCheck,
    OwnershipTransfer,
    
    // Custom Intent
    Custom(String),
}
```

### 4. Constraint Layer

The Constraint Layer explicitly represents all compile-time and runtime constraints.

```rust
/// Constraint
pub struct Constraint {
    pub id: ConstraintId,
    pub kind: ConstraintKind,
    pub source: ConstraintSource,
    pub verification: VerificationStrategy,
}

pub enum ConstraintKind {
    /// Precondition
    Precondition(Expr),
    
    /// Postcondition
    Postcondition(Expr),
    
    /// Invariant
    Invariant(Expr),
    
    /// Type Bound
    TypeBound {
        type_param: String,
        bounds: Vec<TraitRef>,
    },
    
    /// Lifetime Constraint
    Lifetime {
        short: LifetimeRef,
        outlives: LifetimeRef,
    },
    
    /// Effect Constraint
    Effect {
        allowed: EffectSet,
    },
    
    /// Value Range Constraint
    ValueRange {
        variable: NodeId,
        min: Option<i64>,
        max: Option<i64>,
    },
}

pub enum ConstraintSource {
    /// Explicitly declared by user
    Explicit { span: Span },
    
    /// Inferred by compiler
    Inferred { reason: String },
    
    /// Propagated from call
    Propagated { from: NodeId },
}

pub enum VerificationStrategy {
    /// Static Verification
    Static,
    
    /// Runtime Assertion
    Runtime,
    
    /// Hybrid: Static if possible, otherwise Runtime
    Hybrid,
    
    /// Documentation only, no verification
    Documentation,
}
```

### 5. Optimization Hints Layer

```rust
/// Optimization Hint
pub struct OptHint {
    pub kind: OptHintKind,
    pub target: NodeId,
    pub priority: u8,  // 0-255
}

pub enum OptHintKind {
    /// Hotspot Code
    Hotspot { estimated_calls: u64 },
    
    /// Performance Bottleneck
    Bottleneck { issue: String },
    
    /// Inlinable
    Inlinable,
    
    /// Parallelizable
    Parallelizable { data_deps: Vec<NodeId> },
    
    /// Loop Optimization Opportunity
    LoopOptimization { kind: LoopOptKind },
    
    /// Memory Optimization Opportunity
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

## AI Interaction Interface

### 1. Query API

```rust
impl AIIRModule {
    // === Basic Queries ===
    
    /// Get all callers of a function
    pub fn get_callers(&self, func: NodeId) -> Vec<NodeId>;
    
    /// Get all called functions
    pub fn get_callees(&self, func: NodeId) -> Vec<NodeId>;
    
    /// Get data flow for a variable
    pub fn get_dataflow(&self, var: NodeId) -> DataflowInfo;
    
    /// Get lifetime of a variable
    pub fn get_lifetime(&self, var: NodeId) -> LifetimeInfo;
    
    // === Constraint Queries ===
    
    /// Get all constraints for a node
    pub fn get_constraints(&self, node: NodeId) -> Vec<&Constraint>;
    
    /// Check if a constraint is satisfied
    pub fn check_constraint(&self, constraint: &Constraint) -> ConstraintResult;
    
    /// Find constraint conflicts
    pub fn find_constraint_conflicts(&self) -> Vec<ConstraintConflict>;
    
    // === Type Queries ===
    
    /// Get all methods of a type
    pub fn get_methods(&self, ty: TypeRef) -> Vec<NodeId>;
    
    /// Get all traits implemented by a type
    pub fn get_implemented_traits(&self, ty: TypeRef) -> Vec<TraitRef>;
    
    /// Query available operations for a type (API Discovery)
    pub fn get_available_operations(&self, ty: TypeRef) -> Vec<Operation>;
    
    // === Effect Queries ===
    
    /// Get effects of a function
    pub fn get_effects(&self, func: NodeId) -> EffectSet;
    
    /// Check effect compatibility
    pub fn check_effect_compatibility(&self, caller: NodeId, callee: NodeId) -> bool;
}
```

### 2. Mutation API

```rust
impl AIIRModule {
    // === Basic Mutations ===
    
    /// Replace expression
    pub fn replace_expression(
        &mut self, 
        target: NodeId, 
        replacement: AIIRExpr
    ) -> MutationResult;
    
    /// Inline function call
    pub fn inline_call(&mut self, call_site: NodeId) -> MutationResult;
    
    /// Extract expression to variable
    pub fn extract_to_variable(
        &mut self, 
        expr: NodeId, 
        var_name: &str
    ) -> MutationResult;
    
    // === Refactoring Operations ===
    
    /// Rename symbol
    pub fn rename(&mut self, node: NodeId, new_name: &str) -> MutationResult;
    
    /// Extract function
    pub fn extract_function(
        &mut self, 
        block: NodeId, 
        func_name: &str
    ) -> MutationResult;
    
    /// Move code
    pub fn move_code(
        &mut self, 
        source: NodeId, 
        target_location: Location
    ) -> MutationResult;
    
    // === Optimization Operations ===
    
    /// Apply optimization hint
    pub fn apply_optimization(&mut self, hint: &OptHint) -> MutationResult;
    
    /// Unroll loop
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

### 3. Validation API

```rust
impl AIIRModule {
    /// Full validation
    pub fn validate(&self) -> ValidationResult;
    
    /// Type check
    pub fn check_types(&self) -> Vec<TypeError>;
    
    /// Constraint check
    pub fn check_constraints(&self) -> Vec<ConstraintViolation>;
    
    /// Borrow check
    pub fn check_borrows(&self) -> Vec<BorrowError>;
    
    /// Effect check
    pub fn check_effects(&self) -> Vec<EffectError>;
    
    /// Incremental validation (check only changed parts)
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

## Serialization Format

AI-IR supports multiple serialization formats for AI model processing:

### JSON Format (for LLM)

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
        "description": "Integer division"
      }
    }
  ],
  "semantic_graph": {
    "nodes": [...],
    "edges": [...]
  }
}
```

### Protocol Buffer Format (for Efficient Transport)

```protobuf
message AIIRModule {
  string name = 1;
  uint64 version = 2;
  repeated AIIRFunction functions = 3;
  SemanticGraph semantic_graph = 4;
}
```

---

## Integration with Iteration Engine

```rust
/// Iteration Engine uses AI-IR for safe optimization
pub struct IterationEngine {
    /// Current AI-IR
    current_module: AIIRModule,
    
    /// History (for rollback)
    history: Vec<AIIRModule>,
    
    /// Sandbox environment
    sandbox: Sandbox,
    
    /// Audit log
    audit_log: AuditLog,
}

impl IterationEngine {
    /// Execute one iteration
    pub fn iterate(&mut self, mutation: Mutation) -> IterationResult {
        // 1. Apply mutation in sandbox
        let sandbox_module = self.sandbox.apply(mutation);
        
        // 2. Validate modification
        let validation = sandbox_module.validate();
        if !validation.is_valid {
            return IterationResult::Rejected(validation.errors);
        }
        
        // 3. Run tests
        let test_result = self.sandbox.run_tests();
        if !test_result.passed {
            return IterationResult::TestFailed(test_result);
        }
        
        // 4. Record audit log
        self.audit_log.record(mutation, validation, test_result);
        
        // 5. Save history
        self.history.push(self.current_module.clone());
        
        // 6. Apply change
        self.current_module = sandbox_module;
        
        IterationResult::Accepted
    }
    
    /// Rollback to previous version
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

## Implementation Roadmap

### Phase 1: Core Data Structures
- [ ] Implement `SemanticNode` and `SemanticEdge`
- [ ] Implement `SemanticGraph` and index
- [ ] Implement Constraint System

### Phase 2: AST to AI-IR Conversion
- [ ] Function definition conversion
- [ ] Type definition conversion
- [ ] Expression and statement conversion
- [ ] Constraint collection and propagation

### Phase 3: Query Interface
- [ ] Basic query implementation
- [ ] Constraint query implementation
- [ ] Type and effect query

### Phase 4: Mutation Interface
- [ ] Basic mutation operations
- [ ] Refactoring operations
- [ ] Incremental validation

### Phase 5: Iteration Engine Integration
- [ ] Sandbox environment
- [ ] Audit log
- [ ] Version control

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 1.0-draft | 2026-01-13 | Initial Design |
