# AetherLang Context Parameter Pattern

> Best practices for explicit context passing in AetherLang

## Overview

AetherLang encourages **explicit context passing** over hidden global state. This makes code more predictable, testable, and easier for AI to analyze.

## The Context Pattern

### Define a Context Struct

```aether
use alloc::{Allocator, GlobalAllocator}

/// Application context containing shared resources
pub struct Context<A: Allocator> {
    /// Memory allocator for this context
    pub allocator: A,
    /// Configuration settings
    pub config: Config,
    /// Logging level
    pub log_level: i32,
}

impl<A: Allocator> Context<A> {
    pub fn new(allocator: A, config: Config) -> Context<A> {
        Context {
            allocator: allocator,
            config: config,
            log_level: 1,  // INFO
        }
    }
}

/// Default context using global allocator
pub type GlobalContext = Context<GlobalAllocator>

pub fn default_context() -> GlobalContext {
    Context::new(GlobalAllocator::new(), Config::default())
}
```

### Pass Context Explicitly

```aether
// ✅ GOOD: Context passed explicitly
pub fn process_data(data: &[u8], ctx: &mut Context<impl Allocator>) -> Result<Output, Error> effect[alloc] {
    let buffer: Vec<u8, _> = Vec::new_in(&mut ctx.allocator);

    if ctx.log_level >= 2 {
        log_debug("Processing data...");
    }

    // Process using context resources
    do_work(data, &mut buffer, ctx)?;

    Ok(Output::from_buffer(buffer))
}

// ❌ BAD: Hidden global state
pub fn process_data_bad(data: &[u8]) -> Result<Output, Error> {
    let buffer: Vec<u8> = Vec::new();  // Where does memory come from?
    log_debug("Processing...");         // What logger? What level?
    // ...
}
```

### Benefits

1. **Explicit Dependencies** - All resources are visible in function signatures
2. **Testability** - Easy to inject mock allocators/loggers for testing
3. **AI Readability** - AI can trace resource usage through the call graph
4. **No Hidden State** - No surprises from global mutations

## Common Context Components

### Allocator Context

```aether
pub struct AllocContext<A: Allocator> {
    pub alloc: A,
}

// Functions that allocate take AllocContext
pub fn create_buffer<A: Allocator>(size: u64, ctx: &mut AllocContext<A>) -> *u8 effect[alloc] {
    ctx.alloc.allocate(size, 8)
}
```

### IO Context

```aether
pub struct IOContext {
    pub stdin: FileHandle,
    pub stdout: FileHandle,
    pub stderr: FileHandle,
}

// Functions that do IO take IOContext
pub fn print_line(msg: &str, ctx: &mut IOContext) effect[io] {
    write(ctx.stdout, msg);
    write(ctx.stdout, "\n");
}
```

### Combined Context

```aether
pub struct AppContext<A: Allocator> {
    pub alloc: A,
    pub io: IOContext,
    pub config: Config,
    pub metrics: Metrics,
}
```

## Anti-Patterns to Avoid

### ❌ Global Singletons

```aether
// BAD: Hidden global state
static mut GLOBAL_CONFIG: Config = Config::default();

fn get_config() -> &Config {
    unsafe { &GLOBAL_CONFIG }
}
```

### ❌ Thread-Local Storage (when avoidable)

```aether
// BAD: Hidden per-thread state
thread_local! {
    static CONTEXT: Context = Context::new();
}
```

### ❌ Implicit Allocators

```aether
// BAD: Allocation source unclear
fn create_list() -> List<i32> {
    List::new()  // Where does memory come from?
}

// GOOD: Allocation source explicit
fn create_list<A: Allocator>(alloc: &mut A) -> List<i32, A> effect[alloc] {
    List::new_in(alloc)
}
```

## Migration Guide

### From Implicit to Explicit

1. **Identify all resource usage** in your function
2. **Create a context struct** with those resources
3. **Add context parameter** to function signature
4. **Update callers** to pass context

### Before

```aether
fn process_items(items: &[Item]) -> Vec<Result> {
    let results: Vec<Result> = Vec::new();
    for item in items {
        let r: Result = compute(item);
        log_info("Processed item");
        results.push(r);
    }
    results
}
```

### After

```aether
fn process_items<A: Allocator>(
    items: &[Item],
    ctx: &mut Context<A>
) -> Vec<Result, A> effect[alloc, io] {
    let results: Vec<Result, A> = Vec::new_in(&mut ctx.allocator);
    for item in items {
        let r: Result = compute(item);
        if ctx.log_level >= 1 {
            log_info("Processed item", &mut ctx.io);
        }
        results.push(r);
    }
    results
}
```

## Summary

| Principle | Description |
|-----------|-------------|
| **Explicit > Implicit** | Pass resources as parameters, not globals |
| **Context Structs** | Group related resources into context types |
| **Effect Annotations** | Mark functions with their effects |
| **Allocator Parameters** | Use `A: Allocator` for memory-allocating code |
| **Testability** | Design for easy mocking and testing |
