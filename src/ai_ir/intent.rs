//! Intent Layer: High-level intent annotations for AI understanding
//!
//! The intent layer captures what code does at a high level,
//! helping AI understand purpose, not just structure.


/// High-level intent annotation
#[derive(Debug, Clone)]
pub struct Intent {
    /// What this code intends to do
    pub kind: IntentKind,
    
    /// Natural language description (optional)
    pub description: Option<String>,
    
    /// Confidence level (0.0 - 1.0)
    pub confidence: f64,
}

/// Categories of code intent
#[derive(Debug, Clone)]
pub enum IntentKind {
    // === Data Processing ===
    /// Sorting data
    Sort { ascending: bool },
    /// Filtering data
    Filter,
    /// Mapping/transforming data
    Map,
    /// Reducing/aggregating data
    Reduce,
    /// Searching for elements
    Search,
    
    // === Control Flow ===
    /// Error handling
    ErrorHandling,
    /// Input validation
    Validation,
    /// Initialization
    Initialization,
    /// Cleanup/finalization
    Cleanup,
    /// Retry logic
    Retry { max_attempts: Option<u32> },
    
    // === Performance ===
    /// Caching computation
    Cache,
    /// Lazy evaluation
    LazyEval,
    /// Parallelizable operation
    Parallel,
    /// Batching operations
    Batch,
    
    // === Safety ===
    /// Bounds checking
    BoundsCheck,
    /// Null/None checking
    NullCheck,
    /// Ownership transfer
    OwnershipTransfer,
    /// Resource management
    ResourceManagement,
    
    // === I/O ===
    /// Reading input
    Read,
    /// Writing output
    Write,
    /// Network communication
    Network,
    
    // === Custom ===
    /// Custom intent with description
    Custom(String),
}

impl Intent {
    /// Create a new intent with high confidence
    pub fn new(kind: IntentKind) -> Self {
        Self {
            kind,
            description: None,
            confidence: 1.0,
        }
    }
    
    /// Create an intent with description
    pub fn with_description(kind: IntentKind, desc: &str) -> Self {
        Self {
            kind,
            description: Some(desc.to_string()),
            confidence: 1.0,
        }
    }
    
    /// Create an inferred intent with confidence
    pub fn inferred(kind: IntentKind, confidence: f64) -> Self {
        Self {
            kind,
            description: None,
            confidence,
        }
    }
}

impl Default for Intent {
    fn default() -> Self {
        Self {
            kind: IntentKind::Custom("unknown".to_string()),
            description: None,
            confidence: 0.0,
        }
    }
}
