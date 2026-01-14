//! Structured Feedback Module
//!
//! Provides machine-readable output for AI consumption:
//! - JSON error reports with fix suggestions
//! - Compilation statistics
//! - Performance metrics
#![allow(dead_code, unused_variables)]

pub mod iteration;

use serde::{Serialize, Deserialize};
use crate::utils::Error;

// ==================== Structured Error Report ====================

/// A structured error report for AI consumption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorReport {
    /// Error code (e.g., "E0001")
    pub code: String,
    
    /// Error severity
    pub severity: Severity,
    
    /// Human-readable message
    pub message: String,
    
    /// Location information
    pub location: Option<Location>,
    
    /// Suggested fixes
    pub suggestions: Vec<Suggestion>,
    
    /// Related information
    pub related: Vec<RelatedInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Description of the fix
    pub message: String,
    
    /// The replacement text
    pub replacement: Option<String>,
    
    /// Location to apply the fix
    pub location: Option<Location>,
    
    /// Confidence in this suggestion (0.0 - 1.0)
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedInfo {
    pub message: String,
    pub location: Option<Location>,
}

// ==================== Compilation Feedback ====================

/// Complete compilation feedback for AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationFeedback {
    /// Compilation status
    pub success: bool,
    
    /// Source file
    pub source_file: String,
    
    /// All errors and warnings
    pub diagnostics: Vec<ErrorReport>,
    
    /// Compilation statistics
    pub stats: CompilationStats,
    
    /// AI-IR summary (if generated)
    pub ai_ir_summary: Option<AIIRSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationStats {
    /// Parse time in milliseconds
    pub parse_time_ms: u64,
    
    /// Semantic analysis time
    pub semantic_time_ms: u64,
    
    /// IR generation time
    pub ir_gen_time_ms: u64,
    
    /// Total time
    pub total_time_ms: u64,
    
    /// Number of functions
    pub function_count: usize,
    
    /// Number of types
    pub type_count: usize,
    
    /// Lines of code
    pub loc: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIIRSummary {
    pub node_count: usize,
    pub edge_count: usize,
    pub constraint_count: usize,
    pub pure_function_count: usize,
    pub effect_function_count: usize,
}

// ==================== Error Conversion ====================

impl ErrorReport {
    /// Create an error report from a compiler error
    /// Enhanced with multiple suggestions for AI error recovery
    pub fn from_error(error: &Error, file_name: &str) -> Self {
        let (code, message, suggestions) = generate_error_info(error);
        
        let location = error.span().map(|s| Location {
            file: file_name.to_string(),
            line: s.start as u32,
            column: 0,
            end_line: Some(s.end as u32),
            end_column: None,
        });
        
        Self {
            code,
            severity: Severity::Error,
            message,
            location,
            suggestions,
            related: vec![],
        }
    }
    
    /// Create a warning report
    pub fn warning(code: &str, message: &str, location: Option<Location>) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Warning,
            message: message.to_string(),
            location,
            suggestions: vec![],
            related: vec![],
        }
    }
    
    /// Add a suggestion to this report
    pub fn add_suggestion(&mut self, message: &str, replacement: Option<String>, confidence: f64) {
        self.suggestions.push(Suggestion {
            message: message.to_string(),
            replacement,
            location: self.location.clone(),
            confidence,
        });
    }
    
    /// Sort suggestions by confidence (highest first)
    pub fn sort_suggestions(&mut self) {
        self.suggestions.sort_by(|a, b| {
            b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
}

/// Generate error info with multiple suggestions
/// This is the core smart error recovery logic
fn generate_error_info(error: &Error) -> (String, String, Vec<Suggestion>) {
    match error {
        // ========== Type Errors ==========
        Error::TypeMismatch { expected, got, span } => {
            let mut suggestions = vec![
                Suggestion {
                    message: format!("Cast the value to {}", expected),
                    replacement: Some(format!("({} as {})", got, expected)),
                    location: None,
                    confidence: 0.7,
                },
                Suggestion {
                    message: format!("Use a variable of type {}", expected),
                    replacement: None,
                    location: None,
                    confidence: 0.5,
                },
            ];
            
            // AI error pattern: often confuses i32 and i64
            if (got == "i32" && expected == "i64") || (got == "i64" && expected == "i32") {
                suggestions.insert(0, Suggestion {
                    message: "Common AI error: integer size mismatch. Consider using explicit type annotations".to_string(),
                    replacement: Some(format!("value as {}", expected)),
                    location: None,
                    confidence: 0.9,
                });
            }
            
            (
                "E0001".to_string(),
                format!("Type mismatch: expected {}, got {}", expected, got),
                suggestions,
            )
        }
        
        // ========== Undefined Variable ==========
        Error::UndefinedVariable { name, span } => {
            let mut suggestions = vec![
                Suggestion {
                    message: format!("Define '{}' before using it", name),
                    replacement: Some(format!("let {} = /* value */;\n", name)),
                    location: None,
                    confidence: 0.8,
                },
            ];
            
            // AI error pattern: typos in common variable names
            let common_typos = get_common_typos(name);
            for (typo, correct) in common_typos {
                suggestions.push(Suggestion {
                    message: format!("Did you mean '{}'?", correct),
                    replacement: Some(correct.clone()),
                    location: None,
                    confidence: 0.75,
                });
            }
            
            // AI error pattern: using variable before assignment
            suggestions.push(Suggestion {
                message: "Check if the variable was declared in an earlier scope".to_string(),
                replacement: None,
                location: None,
                confidence: 0.4,
            });
            
            (
                "E0002".to_string(),
                format!("Undefined variable: {}", name),
                suggestions,
            )
        }
        
        // ========== Effect Violation ==========
        Error::EffectViolation { message, span } => {
            (
                "E0010".to_string(),
                format!("Effect violation: {}", message),
                vec![
                    Suggestion {
                        message: "Remove 'pure' annotation from the function".to_string(),
                        replacement: Some("/* effect[io] */".to_string()),
                        location: None,
                        confidence: 0.8,
                    },
                    Suggestion {
                        message: "Use a wrapper function that handles the impure operation".to_string(),
                        replacement: None,
                        location: None,
                        confidence: 0.6,
                    },
                    Suggestion {
                        message: "If the call is necessary, mark this function with appropriate effects".to_string(),
                        replacement: None,
                        location: None,
                        confidence: 0.5,
                    },
                ],
            )
        }
        
        // ========== Argument Count Mismatch ==========
        Error::ArgCountMismatch { expected, got, span } => {
            let mut suggestions = vec![];
            
            if *got < *expected {
                suggestions.push(Suggestion {
                    message: format!("Add {} more argument(s)", expected - got),
                    replacement: None,
                    location: None,
                    confidence: 0.9,
                });
            } else {
                suggestions.push(Suggestion {
                    message: format!("Remove {} extra argument(s)", got - expected),
                    replacement: None,
                    location: None,
                    confidence: 0.9,
                });
            }
            
            // AI error pattern: forgetting self parameter
            if *expected > 0 && *got == expected - 1 {
                suggestions.push(Suggestion {
                    message: "For methods, 'self' might be implicit but receiver is required".to_string(),
                    replacement: None,
                    location: None,
                    confidence: 0.7,
                });
            }
            
            (
                "E0003".to_string(),
                format!("Argument count mismatch: expected {}, got {}", expected, got),
                suggestions,
            )
        }
        
        // ========== Default Case ==========
        _ => (
            "E9999".to_string(),
            format!("{}", error),
            vec![Suggestion {
                message: "Check the error message for details".to_string(),
                replacement: None,
                location: None,
                confidence: 0.1,
            }],
        ),
    }
}

/// Get common typos for variable names (AI error pattern recognition)
fn get_common_typos(name: &str) -> Vec<(String, String)> {
    let mut typos = Vec::new();
    
    // Common patterns AI often confuses
    let patterns = [
        ("resut", "result"),
        ("reuslt", "result"),
        ("reslut", "result"),
        ("lenght", "length"),
        ("lenth", "length"),
        ("indx", "index"),
        ("idx", "index"),
        ("cnt", "count"),
        ("coutn", "count"),
        ("val", "value"),
        ("valu", "value"),
        ("tmp", "temp"),
        ("i", "index"),  // Often AI uses 'i' but declares 'index'
    ];
    
    for (wrong, correct) in patterns {
        if name == wrong {
            typos.push((wrong.to_string(), correct.to_string()));
        }
        if name == correct {
            // Also suggest the abbreviation might be intended
            typos.push((correct.to_string(), wrong.to_string()));
        }
    }
    
    typos
}

impl CompilationFeedback {
    /// Create a successful feedback
    pub fn success(source_file: String, stats: CompilationStats) -> Self {
        Self {
            success: true,
            source_file,
            diagnostics: vec![],
            stats,
            ai_ir_summary: None,
        }
    }
    
    /// Create a failed feedback
    pub fn failure(source_file: String, errors: Vec<ErrorReport>, stats: CompilationStats) -> Self {
        Self {
            success: false,
            source_file,
            diagnostics: errors,
            stats,
            ai_ir_summary: None,
        }
    }
    
    /// Output as JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }
    
    /// Output as compact JSON (for programmatic use)
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for CompilationStats {
    fn default() -> Self {
        Self {
            parse_time_ms: 0,
            semantic_time_ms: 0,
            ir_gen_time_ms: 0,
            total_time_ms: 0,
            function_count: 0,
            type_count: 0,
            loc: 0,
        }
    }
}
