//! Type System for AetherLang
#![allow(dead_code)]

/// Primitive types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    I8, I16, I32, I64, Isize,
    U8, U16, U32, U64, Usize,
    F32, F64,
    Bool,
    Char,
    Unit,
    Never,
}

impl PrimitiveType {
    /// Get the size in bytes
    pub fn size_of(&self) -> usize {
        match self {
            Self::I8 | Self::U8 | Self::Bool => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 | Self::Char => 4,
            Self::I64 | Self::U64 | Self::F64 | Self::Isize | Self::Usize => 8,
            Self::Unit | Self::Never => 0,
        }
    }
    
    /// Get the alignment
    pub fn align_of(&self) -> usize {
        self.size_of().max(1)
    }
    
    /// Check if this is a signed integer type
    pub fn is_signed(&self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Isize)
    }
    
    /// Check if this is an integer type
    pub fn is_integer(&self) -> bool {
        matches!(self, 
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::Isize |
            Self::U8 | Self::U16 | Self::U32 | Self::U64 | Self::Usize
        )
    }
    
    /// Check if this is a floating-point type
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

/// Resolved type (after type checking)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedType {
    Primitive(PrimitiveType),
    Pointer(Box<ResolvedType>),
    Reference { mutable: bool, inner: Box<ResolvedType> },
    Array { elem: Box<ResolvedType>, size: usize },
    Slice(Box<ResolvedType>),
    Tuple(Vec<ResolvedType>),
    Struct { name: String, fields: Vec<(String, ResolvedType)> },
    Enum { name: String },
    Function { params: Vec<ResolvedType>, ret: Box<ResolvedType> },
    /// Generic type instantiation (e.g. Option<i32>)
    Generic(String, Vec<ResolvedType>),
    /// Generic Type Parameter (e.g. T in struct Foo<T>)
    GenericParam(String),
    /// String type (pointer to char array)
    String,
    Unknown,
}

impl ResolvedType {
    pub fn unit() -> Self {
        Self::Primitive(PrimitiveType::Unit)
    }
    
    pub fn never() -> Self {
        Self::Primitive(PrimitiveType::Never)
    }
    
    pub fn i32() -> Self {
        Self::Primitive(PrimitiveType::I32)
    }
    
    pub fn bool() -> Self {
        Self::Primitive(PrimitiveType::Bool)
    }
    
    // Constants for common types
    pub const UNIT: Self = Self::Primitive(PrimitiveType::Unit);
    pub const NEVER: Self = Self::Primitive(PrimitiveType::Never);
    pub const BOOL: Self = Self::Primitive(PrimitiveType::Bool);
    pub const I8: Self = Self::Primitive(PrimitiveType::I8);
    pub const I16: Self = Self::Primitive(PrimitiveType::I16);
    pub const I32: Self = Self::Primitive(PrimitiveType::I32);
    pub const I64: Self = Self::Primitive(PrimitiveType::I64);
    pub const U8: Self = Self::Primitive(PrimitiveType::U8);
    pub const U16: Self = Self::Primitive(PrimitiveType::U16);
    pub const U32: Self = Self::Primitive(PrimitiveType::U32);
    pub const U64: Self = Self::Primitive(PrimitiveType::U64);
    pub const F32: Self = Self::Primitive(PrimitiveType::F32);
    pub const F64: Self = Self::Primitive(PrimitiveType::F64);
    
    /// Create a pointer type
    pub fn ptr(inner: Box<ResolvedType>) -> Self {
        Self::Pointer(inner)
    }
}
