# Aether Script Language Specification (`.ath`)

> **"Syntactic Simplicity, System-Level Truth"**

Aether Script is the high-level, "Layer 1" language of the Aether ecosystem. It is designed to be **approachable** (using indentation-based syntax) but **truthful** (transpiles to explicit, zero-magic Aether Core code).

## 1. Core Philosophy

1.  **Zero Abstraction Leaks**: Every Script construct maps to a visible, deterministic sequences of Core code. No hidden runtimes.
2.  **Radical Explicitness**: The generated Core code is "AI-Readable" and brutally explicit (fully qualified types, explicit error handling).
3.  **Context-Aware**: The transpiler is aware of the "System Context" (e.g., Default Allocator, Target Backend) but expects the user to be explicit about *intent*.

## 2. Syntax Overview

Aether Script uses indentation for block structure.

```python
# Function Definition
def calculate_area(radius: float) -> float:
    if radius < 0.0:
        return 0.0
        
    # Standard math usage
    return 3.14159 * radius * radius

# Comptime Execution (Metaprogramming)
@comptime
def generate_lookup_table():
    pass
```

### 2.1 Basic Types Mapping

| Script Type | Core Type (Layer 0) | Rule |
| :--- | :--- | :--- |
| `int` | `i64` | Signed 64-bit integer |
| `float` | `f64` | 64-bit float |
| `bool` | `bool` | - |
| `str` | `String` | **Always Owned** (Heap). String literals auto-convert to `String` unless typed `&str`. |
| `&str` | `&str` | **Zero-Copy Slice**. explicit annotation required. |
| `List[T]` | `Vec<T>` | Dynamic array (Heap) |
| `None` | `Option::None` | - |

### 2.2 Mutability & ownership
Script variables are **mutable by default** to simplify algorithm prototyping.
```python
x = 10       # Transpiles to: let mut x: i64 = 10;
x = 20
```

## 3. The "Anti-Leak" System

### 3.1 The Implicit Context (`ctx`)
To avoid global state, every Script function silently accepts a `ctx` argument in the generated Core code (unless marked `@pure` or `@static`).
```python
# Script
def process():
    data = [] 

# Core
fn process(ctx: &mut ScriptContext) {
    let mut data = Vec::new_in(ctx.allocator);
}
```

### 3.2 Modules & Imports
Aether Script uses `import` which maps to `use` in Core, but with resolution logic.
```python
import std.io
from math import sin

# C-Interop (Auto-FFI)
extern "C":
    def malloc(size: int) -> ptr
```

### 3.3 Error Handling (Result vs Exceptions)

**Explicit (With Block):**
```python
# Script
with region_allocator as alloc:
    temp_data = [x * 2 for x in data]

# Core (Transpiled)
let temp_data = Vec::new_in(alloc);
...
```

### 3.4 Error Handling (Result vs Exceptions)
Aether Script does NOT have Exceptions. It has syntax sugar for `Result`.

```python
# Script: '?' operator propagates errors
def read_file(path: str) -> Result[str, Error]:
    f = File.open(path)?
    return f.read_all()

# Core (Transpiled)
fn read_file(path: String) -> Result<String, Error> {
    let f = match File::open(path) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    return f.read_all();
}
```

## 4. Interop & Classes

### 4.1 Structs (Data Classes)
```python
# Script
class User:
    id: int
    name: str

# Core
struct User {
    id: i64,
    name: String,
}
```

### 4.2 Methods
```python
# Script
impl User:
    def greet(self):
        print("Hello " + self.name)
```

## 5. Transpilation Rules (The "Contract")

1.  **Name Mangling**: Script names are preserved 1:1 where possible to ensure generated code remains readable.
2.  **No Implicit Libraries**: `print()` transpiles to `std::io::print()` ONLY if `std` is imported. Otherwise, it might error or define a "prelude".
    *   *Decision*: Aether Script includes a minimal "Core Prelude" that maps common non-allocating functions.
3.  **Source Maps**: Every generated Core line is tagged with `#line` pointing back to `.ath`.

## 6. Project Structure

Aether projects can mix `.aeth` and `.ath` files.

- `src/main.ath` -> Transpiled to `build/gen/main.aeth` -> Compiled.
- `src/lib.aeth` -> Compiled directly.
- The **Linker** sees only object files from both.
