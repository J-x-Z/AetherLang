//! Built-in Functions Registry
//!
//! Defines all built-in functions available in AetherLang.
#![allow(dead_code)]

use std::collections::HashMap;
use crate::types::type_system::ResolvedType;

/// Built-in function signature
#[derive(Debug, Clone)]
pub struct BuiltinFunc {
    pub name: String,
    pub params: Vec<(String, ResolvedType)>,
    pub ret_type: ResolvedType,
    /// C function name to generate
    pub c_name: String,
    /// Whether this function is variadic (like printf)
    pub variadic: bool,
}

/// Registry of all built-in functions
pub struct BuiltinRegistry {
    functions: HashMap<String, BuiltinFunc>,
}

impl BuiltinRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            functions: HashMap::new(),
        };
        registry.register_all();
        registry
    }

    fn register_all(&mut self) {
        // I/O Functions
        self.register(BuiltinFunc {
            name: "print".to_string(),
            params: vec![("s".to_string(), ResolvedType::String)],
            ret_type: ResolvedType::UNIT,
            c_name: "aether_print".to_string(),
            variadic: false,
        });

        self.register(BuiltinFunc {
            name: "println".to_string(),
            params: vec![("s".to_string(), ResolvedType::String)],
            ret_type: ResolvedType::UNIT,
            c_name: "aether_println".to_string(),
            variadic: false,
        });

        self.register(BuiltinFunc {
            name: "print_i64".to_string(),
            params: vec![("n".to_string(), ResolvedType::I64)],
            ret_type: ResolvedType::UNIT,
            c_name: "aether_print_i64".to_string(),
            variadic: false,
        });

        self.register(BuiltinFunc {
            name: "println_i64".to_string(),
            params: vec![("n".to_string(), ResolvedType::I64)],
            ret_type: ResolvedType::UNIT,
            c_name: "aether_println_i64".to_string(),
            variadic: false,
        });

        // Memory functions
        self.register(BuiltinFunc {
            name: "alloc".to_string(),
            params: vec![("size".to_string(), ResolvedType::U64)],
            ret_type: ResolvedType::ptr(Box::new(ResolvedType::U8)),
            c_name: "malloc".to_string(),
            variadic: false,
        });

        self.register(BuiltinFunc {
            name: "free".to_string(),
            params: vec![("ptr".to_string(), ResolvedType::ptr(Box::new(ResolvedType::U8)))],
            ret_type: ResolvedType::UNIT,
            c_name: "free".to_string(),
            variadic: false,
        });

        // Process control
        self.register(BuiltinFunc {
            name: "exit".to_string(),
            params: vec![("code".to_string(), ResolvedType::I32)],
            ret_type: ResolvedType::NEVER,
            c_name: "exit".to_string(),
            variadic: false,
        });

        // Debug
        self.register(BuiltinFunc {
            name: "assert".to_string(),
            params: vec![("cond".to_string(), ResolvedType::BOOL)],
            ret_type: ResolvedType::UNIT,
            c_name: "aether_assert".to_string(),
            variadic: false,
        });
    }

    fn register(&mut self, func: BuiltinFunc) {
        self.functions.insert(func.name.clone(), func);
    }

    /// Check if a function is a built-in
    pub fn is_builtin(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Get a built-in function by name
    pub fn get(&self, name: &str) -> Option<&BuiltinFunc> {
        self.functions.get(name)
    }

    /// Get all built-in functions
    pub fn all(&self) -> impl Iterator<Item = &BuiltinFunc> {
        self.functions.values()
    }
}

impl Default for BuiltinRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate C runtime support code
pub fn generate_c_runtime() -> String {
    r#"
/* AetherLang Runtime Support */

static void aether_print(const char* s) {
    printf("%s", s);
}

static void aether_println(const char* s) {
    printf("%s\n", s);
}

static void aether_print_i64(int64_t n) {
    printf("%lld", (long long)n);
}

static void aether_println_i64(int64_t n) {
    printf("%lld\n", (long long)n);
}

static void aether_assert(bool cond) {
    if (!cond) {
        fprintf(stderr, "Assertion failed\n");
        exit(1);
    }
}

"#.to_string()
}
