//! Iteration Engine for AI Self-Optimization
//!
//! Manages the lifecycle of AI-driven code optimization:
//! - Sandbox isolation for safe iteration
//! - Resource limits to prevent runaway optimization
//! - Audit logging for all decisions
//! - Version control and rollback

use serde::{Serialize, Deserialize};
use std::time::{Duration, SystemTime};

// ==================== Iteration Engine ====================

/// The iteration engine manages AI optimization cycles
#[derive(Debug)]
pub struct IterationEngine {
    /// Configuration
    config: IterationConfig,
    
    /// Current iteration state
    state: IterationState,
    
    /// Audit log
    audit_log: Vec<AuditEntry>,
    
    /// Version history (for rollback)
    versions: Vec<VersionSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationConfig {
    /// Maximum iterations per session
    pub max_iterations: u32,
    
    /// Maximum time per iteration
    pub max_time_per_iteration: Duration,
    
    /// Maximum total time
    pub max_total_time: Duration,
    
    /// Maximum memory usage (bytes)
    pub max_memory_bytes: usize,
    
    /// Allow unsafe transformations
    pub allow_unsafe: bool,
    
    /// Require all tests to pass
    pub require_tests_pass: bool,
}

#[derive(Debug, Clone)]
pub struct IterationState {
    /// Current iteration number
    pub iteration: u32,
    
    /// Start time of current session
    pub start_time: SystemTime,
    
    /// Status
    pub status: IterationStatus,
    
    /// Last result
    pub last_result: Option<IterationResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IterationStatus {
    /// Ready to start
    Idle,
    /// Running an iteration
    Running,
    /// Paused (awaiting approval)
    Paused,
    /// Completed successfully
    Completed,
    /// Failed
    Failed(String),
    /// Aborted (timeout or resource limit)
    Aborted(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationResult {
    /// Did the iteration improve the code?
    pub improved: bool,
    
    /// Performance metrics change
    pub performance_delta: Option<f64>,
    
    /// Code size change
    pub size_delta: Option<i64>,
    
    /// Transformations applied
    pub transformations: Vec<String>,
    
    /// Warnings generated
    pub warnings: Vec<String>,
}

// ==================== Audit Log ====================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp
    pub timestamp: SystemTime,
    
    /// Entry type
    pub entry_type: AuditEntryType,
    
    /// Description
    pub description: String,
    
    /// AI model identifier (if applicable)
    pub ai_model: Option<String>,
    
    /// Additional context
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEntryType {
    /// Session started
    SessionStart,
    /// Iteration started
    IterationStart,
    /// Transformation applied
    TransformationApplied,
    /// Validation passed
    ValidationPassed,
    /// Validation failed
    ValidationFailed,
    /// Rollback performed
    Rollback,
    /// Session completed
    SessionComplete,
    /// Error occurred
    Error,
}

// ==================== Version Snapshot ====================

#[derive(Debug, Clone)]
pub struct VersionSnapshot {
    /// Version number
    pub version: u32,
    
    /// Timestamp
    pub timestamp: SystemTime,
    
    /// Description of changes
    pub description: String,
    
    /// Snapshot of AI-IR (serialized)
    pub ai_ir_snapshot: String,
}

// ==================== Implementation ====================

impl Default for IterationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            max_time_per_iteration: Duration::from_secs(30),
            max_total_time: Duration::from_secs(300),
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            allow_unsafe: false,
            require_tests_pass: true,
        }
    }
}

impl IterationEngine {
    /// Create a new iteration engine
    pub fn new(config: IterationConfig) -> Self {
        Self {
            config,
            state: IterationState {
                iteration: 0,
                start_time: SystemTime::now(),
                status: IterationStatus::Idle,
                last_result: None,
            },
            audit_log: Vec::new(),
            versions: Vec::new(),
        }
    }
    
    /// Start a new iteration session
    pub fn start_session(&mut self) {
        self.state.iteration = 0;
        self.state.start_time = SystemTime::now();
        self.state.status = IterationStatus::Running;
        
        self.log(AuditEntryType::SessionStart, "Iteration session started", None);
    }
    
    /// Check if we can continue iterating
    pub fn can_continue(&self) -> bool {
        match &self.state.status {
            IterationStatus::Running => {
                // Check iteration limit
                if self.state.iteration >= self.config.max_iterations {
                    return false;
                }
                
                // Check time limit
                if let Ok(elapsed) = self.state.start_time.elapsed() {
                    if elapsed > self.config.max_total_time {
                        return false;
                    }
                }
                
                true
            }
            _ => false,
        }
    }
    
    /// Record a transformation
    pub fn record_transformation(&mut self, description: &str, ai_model: Option<&str>) {
        self.log(
            AuditEntryType::TransformationApplied,
            description,
            ai_model,
        );
    }
    
    /// Complete an iteration
    pub fn complete_iteration(&mut self, result: IterationResult) {
        self.state.iteration += 1;
        self.state.last_result = Some(result);
        
        self.log(
            AuditEntryType::IterationStart,
            &format!("Iteration {} completed", self.state.iteration),
            None,
        );
    }
    
    /// Create a version snapshot for rollback
    pub fn create_snapshot(&mut self, description: &str, ai_ir_json: &str) {
        let version = self.versions.len() as u32 + 1;
        self.versions.push(VersionSnapshot {
            version,
            timestamp: SystemTime::now(),
            description: description.to_string(),
            ai_ir_snapshot: ai_ir_json.to_string(),
        });
    }
    
    /// Rollback to a previous version
    pub fn rollback(&mut self, version: u32) -> Option<&VersionSnapshot> {
        self.log(
            AuditEntryType::Rollback,
            &format!("Rolling back to version {}", version),
            None,
        );
        
        self.versions.get(version as usize - 1)
    }
    
    /// End the session
    pub fn end_session(&mut self, success: bool) {
        self.state.status = if success {
            IterationStatus::Completed
        } else {
            IterationStatus::Failed("Session ended".to_string())
        };
        
        self.log(
            AuditEntryType::SessionComplete,
            &format!("Session completed: {} iterations, success={}", 
                     self.state.iteration, success),
            None,
        );
    }
    
    /// Get the audit log
    pub fn get_audit_log(&self) -> &[AuditEntry] {
        &self.audit_log
    }
    
    /// Export audit log as JSON
    pub fn export_audit_log_json(&self) -> String {
        serde_json::to_string_pretty(&self.audit_log)
            .unwrap_or_else(|_| "[]".to_string())
    }
    
    /// Internal logging
    fn log(&mut self, entry_type: AuditEntryType, description: &str, ai_model: Option<&str>) {
        self.audit_log.push(AuditEntry {
            timestamp: SystemTime::now(),
            entry_type,
            description: description.to_string(),
            ai_model: ai_model.map(|s| s.to_string()),
            context: None,
        });
    }
}
