//! Source location tracking
#![allow(dead_code)]

/// A span represents a range in the source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Start byte offset
    pub start: usize,
    /// End byte offset (exclusive)
    pub end: usize,
    /// File ID
    pub file_id: usize,
}

impl Span {
    /// Create a new span
    pub fn new(start: usize, end: usize, file_id: usize) -> Self {
        Self { start, end, file_id }
    }
    
    /// Create a dummy span (for testing)
    pub fn dummy() -> Self {
        Self { start: 0, end: 0, file_id: 0 }
    }
    
    /// Merge two spans
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            file_id: self.file_id,
        }
    }
    
    /// Get the length of the span
    pub fn len(&self) -> usize {
        self.end - self.start
    }
    
    /// Check if the span is empty
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::dummy()
    }
}
