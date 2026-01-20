//! Module System for AetherLang
//!
//! Provides dynamic module loading and symbol resolution for use statements.

use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

use crate::frontend::ast::Item;
use crate::frontend::lexer::Lexer;
use crate::frontend::parser::Parser;
use crate::utils::{Result, Error};

/// Represents a parsed module with its exported symbols
#[derive(Debug, Clone)]
pub struct ParsedModule {
    /// Path to the module file
    pub path: PathBuf,
    /// All items in the module
    pub items: Vec<Item>,
    /// Module name (derived from filename)
    pub name: String,
}

impl ParsedModule {
    /// Get all public items from this module
    pub fn public_items(&self) -> Vec<&Item> {
        self.items.iter().filter(|item| self.is_public(item)).collect()
    }
    
    /// Check if an item is public
    fn is_public(&self, item: &Item) -> bool {
        match item {
            Item::Function(f) => f.is_pub,
            Item::Struct(s) => s.is_pub,
            Item::Enum(_) => true, // Enums are public by default for now
            Item::Interface(i) => i.is_pub,
            Item::Trait(t) => t.is_pub,
            Item::TypeAlias(t) => t.is_pub,
            Item::Const(_) => true, // Constants are public by default
            _ => false,
        }
    }
    
    /// Find a public item by name
    pub fn find_public_item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|item| {
            self.is_public(item) && self.item_name(item) == Some(name)
        })
    }
    
    /// Get the name of an item
    fn item_name<'a>(&self, item: &'a Item) -> Option<&'a str> {
        match item {
            Item::Function(f) => Some(&f.name.name),
            Item::Struct(s) => Some(&s.name.name),
            Item::Enum(e) => Some(&e.name.name),
            Item::Interface(i) => Some(&i.name.name),
            Item::Trait(t) => Some(&t.name.name),
            Item::TypeAlias(t) => Some(&t.name.name),
            Item::Const(c) => Some(&c.name.name),
            _ => None,
        }
    }
}

/// Module loader for dynamic module resolution
pub struct ModuleLoader {
    /// Search paths for module files
    search_paths: Vec<PathBuf>,
    /// Cache of parsed modules by module name
    parsed_modules: HashMap<String, ParsedModule>,
    /// Modules currently being parsed (for circular dependency detection)
    parsing_stack: Vec<String>,
}

impl ModuleLoader {
    /// Create a new module loader with default search paths
    pub fn new() -> Self {
        Self {
            search_paths: vec![
                PathBuf::from("."),
                PathBuf::from("src_aether"),
                PathBuf::from("stdlib"),
            ],
            parsed_modules: HashMap::new(),
            parsing_stack: Vec::new(),
        }
    }
    
    /// Add a search path
    pub fn add_search_path(&mut self, path: PathBuf) {
        if !self.search_paths.contains(&path) {
            self.search_paths.push(path);
        }
    }
    
    /// Find a module file by name
    pub fn find_module_file(&self, module_name: &str) -> Option<PathBuf> {
        for search_path in &self.search_paths {
            let module_path = search_path.join(format!("{}.aeth", module_name));
            if module_path.exists() {
                return Some(module_path);
            }
        }
        None
    }
    
    /// Load and parse a module by name
    pub fn load_module(&mut self, module_name: &str) -> Result<&ParsedModule> {
        // Check cache first
        if self.parsed_modules.contains_key(module_name) {
            return Ok(self.parsed_modules.get(module_name).unwrap());
        }
        
        // Check for circular dependency
        if self.parsing_stack.contains(&module_name.to_string()) {
            return Err(Error::ModuleError(format!(
                "Circular module dependency detected: {} -> {}",
                self.parsing_stack.join(" -> "),
                module_name
            )));
        }
        
        // Find module file
        let module_path = self.find_module_file(module_name)
            .ok_or_else(|| Error::ModuleError(format!(
                "Module not found: {}. Searched in: {:?}",
                module_name,
                self.search_paths
            )))?;
        
        // Parse the module
        self.parsing_stack.push(module_name.to_string());
        let parsed = self.parse_module_file(&module_path, module_name)?;
        self.parsing_stack.pop();
        
        // Cache the result
        self.parsed_modules.insert(module_name.to_string(), parsed);
        
        Ok(self.parsed_modules.get(module_name).unwrap())
    }
    
    /// Parse a module file
    fn parse_module_file(&self, path: &PathBuf, module_name: &str) -> Result<ParsedModule> {
        // Read file contents
        let source = fs::read_to_string(path).map_err(|e| {
            Error::ModuleError(format!("Failed to read module file {:?}: {}", path, e))
        })?;
        
        // Lex and Parse
        let lexer = Lexer::new(&source, 0);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program()?;
        
        Ok(ParsedModule {
            path: path.clone(),
            items: program.items,
            name: module_name.to_string(),
        })
    }
    
    /// Get a cached module if available
    pub fn get_cached_module(&self, module_name: &str) -> Option<&ParsedModule> {
        self.parsed_modules.get(module_name)
    }
    
    /// Check if a module is cached
    pub fn is_cached(&self, module_name: &str) -> bool {
        self.parsed_modules.contains_key(module_name)
    }
}

impl Default for ModuleLoader {
    fn default() -> Self {
        Self::new()
    }
}
