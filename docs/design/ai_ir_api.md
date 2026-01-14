# AI-IR API Reference

> AI-Readable Intermediate Representation - Semantic Layer Designed for AI

## Overview

AI-IR is the core innovation of AetherLang, sitting between AST and traditional IR, specifically designed for AI models.

```
Source Code → AST → AI-IR → Traditional IR → Machine Code
                      ↑
               AI reads/writes here
```

## Core Types

### NodeId / EdgeId
```rust
pub struct NodeId(pub usize);
pub struct EdgeId(pub usize);
```

### AIIRModule
```rust
pub struct AIIRModule {
    pub name: String,
    pub graph: SemanticGraph,      // Semantic Graph
    pub constraints: Vec<Constraint>, // Constraints
    pub hints: Vec<OptimizationHint>, // Optimization Hints
}
```

---

## SemanticGraph

The Semantic Graph contains **Nodes** and **Edges**.

### NodeKind

| Kind | Description | Fields |
|------|-------------|--------|
| `Function` | Function definition | params, return_type, effects, is_pure |
| `Type` | Type definition | type_kind, fields |
| `Variable` | Variable | type_name, ownership, is_mutable |
| `Expression` | Expression | expr_kind, type_name |
| `Block` | Code Block | stmt_count |

### EdgeKind

| Kind | Description |
|------|-------------|
| `Calls` | Function call relationship |
| `DataFlow` | Data flow (ownership_transfer) |
| `ControlFlow` | Control flow |
| `TypeOf` | Type relationship |
| `DependsOn` | Dependency relationship |
| `Borrows` | Borrow relationship (mutable) |

---

## Query API

### Relationship Queries

```rust
// Get callers
let callers = module.get_callers(func_id);
for caller in callers.callers {
    println!("Called by: {:?}", module.get_node(caller));
}

// Get callees
let callees = module.get_callees(func_id);

// Dataflow analysis
let dataflow = module.get_dataflow(node_id);
// dataflow.sources - data sources
// dataflow.sinks   - data sinks
```

### Type Queries

```rust
// Get node type
let type_id = module.get_type_of(node_id);

// Get all instances of a type
let instances = module.nodes_of_type("Point");
```

### Constraint Queries

```rust
// Get preconditions
let preconditions = module.get_preconditions(func_id);

// Get postconditions
let postconditions = module.get_postconditions(func_id);

// Get all constraints
let constraints = module.get_constraints(node_id);
```

### Statistics Summary

```rust
let summary = module.summary();
println!("Nodes: {}", summary.node_count);
println!("Edges: {}", summary.edge_count);
println!("Functions: {}", summary.function_count);
println!("Constraints: {}", summary.constraint_count);
```

---

## Intent Layer

High-level purpose annotations to help AI understand code intent.

### IntentKind

| Category | Intent |
|----------|--------|
| **Data Processing** | Sort, Filter, Map, Reduce, Search |
| **Control Flow** | ErrorHandling, Validation, Initialization, Cleanup, Retry |
| **Performance** | Cache, LazyEval, Parallel, Batch |
| **Safety** | BoundsCheck, NullCheck, OwnershipTransfer |
| **I/O** | Read, Write, Network |

```rust
let intent = Intent::new(IntentKind::Sort { ascending: true });
node.intent = Some(intent);
```

---

## Constraint Layer

Manages explicit and inferred constraints.

### ConstraintKind

| Kind | Source | Example |
|------|--------|---------|
| `Precondition` | `requires` clause | `b != 0` |
| `Postcondition` | `ensures` clause | `result >= 0` |
| `Invariant` | Type/Loop invariant | `len > 0` |
| `TypeBound` | Type parameter bound | `T: Clone` |
| `Effect` | Effect constraint | `[io, alloc]` |

### VerificationStrategy

- `Static` - Verify at compile time
- `Runtime` - Runtime assertion
- `Hybrid` - Try static, fallback to runtime
- `Documentation` - Documentation only, no verification

---

## Usage Examples

### Convert AST to AI-IR

```rust
use crate::ai_ir::AIIRConverter;

let converter = AIIRConverter::new("my_module".to_string());
let ai_ir = converter.convert(&program);

// Query
println!("Functions: {:?}", ai_ir.graph.functions().len());
println!("Constraints: {:?}", ai_ir.constraints.len());
```

### Analyze Call Graph

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

## Design Principles

1. **AI-First** - API designed for AI comprehension first
2. **Explicit > Implicit** - Explicit info is better than implicit inference
3. **Rich Metadata** - Carry rich semantic metadata
4. **Query-Friendly** - Efficient query interfaces
5. **Immutable by Default** - Mutations must be explicit
