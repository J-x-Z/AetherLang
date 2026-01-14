//! Auto-FFI Generator
//!
//! Parses C header files and generates Aether Core `extern "C"` blocks.
//! This is a simplified prototype - full bindgen-like functionality
//! is planned for future releases.

use std::fs;
use std::path::Path;

/// Represents a parsed C function declaration
#[derive(Debug, Clone)]
pub struct CFunctionDecl {
    pub name: String,
    pub return_type: String,
    pub params: Vec<(String, String)>, // (name, type)
}

/// Parse a C header file and extract function declarations
/// This is a simplified parser - only handles basic patterns
pub fn parse_c_header(path: &Path) -> Result<Vec<CFunctionDecl>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read header: {}", e))?;
    
    let mut functions = Vec::new();
    
    // Simple regex-free parsing for function declarations
    // Pattern: <return_type> <name>(<params>);
    for line in content.lines() {
        let line = line.trim();
        
        // Skip preprocessor directives and empty lines
        if line.starts_with('#') || line.is_empty() || line.starts_with("//") {
            continue;
        }
        
        // Look for function declarations ending with );
        if line.ends_with(");") {
            if let Some(decl) = parse_function_decl(line) {
                functions.push(decl);
            }
        }
    }
    
    Ok(functions)
}

/// Parse a single function declaration line
fn parse_function_decl(line: &str) -> Option<CFunctionDecl> {
    // Pattern: TYPE NAME(PARAMS);
    let line = line.trim_end_matches(';');
    
    let paren_start = line.find('(')?;
    let paren_end = line.rfind(')')?;
    
    let before_paren = line[..paren_start].trim();
    let params_str = &line[paren_start + 1..paren_end];
    
    // Split return type and name
    let parts: Vec<&str> = before_paren.rsplitn(2, char::is_whitespace).collect();
    if parts.len() < 2 {
        return None;
    }
    
    let name = parts[0].trim_start_matches('*').to_string();
    let return_type = parts[1].to_string();
    
    // Parse parameters
    let params = parse_params(params_str);
    
    Some(CFunctionDecl {
        name,
        return_type,
        params,
    })
}

/// Parse function parameters
fn parse_params(params_str: &str) -> Vec<(String, String)> {
    if params_str.trim() == "void" || params_str.trim().is_empty() {
        return Vec::new();
    }
    
    params_str
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            let parts: Vec<&str> = p.rsplitn(2, char::is_whitespace).collect();
            if parts.len() >= 2 {
                let name = parts[0].trim_start_matches('*').to_string();
                let ty = parts[1].to_string();
                Some((name, ty))
            } else {
                None
            }
        })
        .collect()
}

/// Map C type to Aether Core type
fn map_c_type(c_type: &str) -> String {
    match c_type.trim() {
        "void" => "()".to_string(),
        "int" => "i32".to_string(),
        "long" => "i64".to_string(),
        "size_t" => "usize".to_string(),
        "char*" | "const char*" => "*u8".to_string(),
        "float" => "f32".to_string(),
        "double" => "f64".to_string(),
        "uint8_t" | "unsigned char" => "u8".to_string(),
        "uint32_t" | "unsigned int" => "u32".to_string(),
        "uint64_t" | "unsigned long" => "u64".to_string(),
        other if other.ends_with('*') => "*u8".to_string(), // Generic pointer
        other => other.to_string(),
    }
}

/// Generate Aether Core `extern "C"` block from parsed declarations
pub fn generate_extern_block(decls: &[CFunctionDecl]) -> String {
    let mut output = String::new();
    output.push_str("// Auto-generated FFI bindings\n");
    output.push_str("extern \"C\" {\n");
    
    for decl in decls {
        output.push_str(&format!("    fn {}(", decl.name));
        
        let params: Vec<String> = decl.params
            .iter()
            .map(|(name, ty)| format!("{}: {}", name, map_c_type(ty)))
            .collect();
        
        output.push_str(&params.join(", "));
        output.push(')');
        
        if decl.return_type != "void" {
            output.push_str(&format!(" -> {}", map_c_type(&decl.return_type)));
        }
        
        output.push_str(";\n");
    }
    
    output.push_str("}\n");
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_function_decl() {
        let decl = parse_function_decl("int printf(const char* format);").unwrap();
        assert_eq!(decl.name, "printf");
        assert_eq!(decl.return_type, "int");
    }

    #[test]
    fn test_generate_extern_block() {
        let decls = vec![
            CFunctionDecl {
                name: "exit".to_string(),
                return_type: "void".to_string(),
                params: vec![("code".to_string(), "int".to_string())],
            }
        ];
        let output = generate_extern_block(&decls);
        assert!(output.contains("fn exit(code: i32)"));
    }
}
