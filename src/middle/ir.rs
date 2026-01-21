//! Aether IR definitions
//!
//! Three-address code style IR with SSA support.
#![allow(dead_code)]

use std::fmt;

/// Struct representation/layout specification
#[derive(Debug, Clone, PartialEq)]
pub enum StructRepr {
    /// Default (Rust-like) layout
    Default,
    /// C-compatible layout
    C,
    /// Packed (no padding)
    Packed,
    /// Transparent (single-field wrapper)
    Transparent,
}

impl Default for StructRepr {
    fn default() -> Self {
        StructRepr::Default
    }
}

/// IR Struct definition
#[derive(Debug, Clone)]
pub struct IRStruct {
    pub name: String,
    pub fields: Vec<(String, IRType)>,
    pub repr: StructRepr,
}

/// IR Module - contains all functions
#[derive(Debug, Clone)]
pub struct IRModule {
    pub name: String,
    pub functions: Vec<IRFunction>,
    pub structs: Vec<IRStruct>,
    pub externs: Vec<IRExtern>,
}

/// External function declaration
#[derive(Debug, Clone)]
pub struct IRExtern {
    pub name: String,
    pub params: Vec<(String, IRType)>,
    pub ret_type: IRType,
}

impl IRModule {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            functions: Vec::new(),
            structs: Vec::new(),
            externs: Vec::new(),
        }
    }
    
    pub fn add_struct(&mut self, name: &str, fields: Vec<(String, IRType)>, repr: StructRepr) {
        self.structs.push(IRStruct {
            name: name.to_string(),
            fields,
            repr,
        });
    }
}


/// IR Function
#[derive(Debug, Clone)]
pub struct IRFunction {
    pub name: String,
    pub params: Vec<(String, IRType)>,
    pub ret_type: IRType,
    /// Original struct return type for sret functions (None if not sret)
    pub sret_type: Option<IRType>,
    pub blocks: Vec<BasicBlock>,
    pub entry_block: BlockId,
    /// Contract assertions for runtime checking
    pub contracts: IRContracts,
    /// SIMD annotation - enable auto-vectorization hints
    pub simd: bool,
    /// Naked function - no prologue/epilogue (for asm)
    pub naked: bool,
    /// Interrupt handler function
    pub interrupt: bool,
}

/// Contract expressions for runtime assertion generation
#[derive(Debug, Clone, Default)]
pub struct IRContracts {
    /// Preconditions (checked at function entry)
    pub requires: Vec<String>,
    /// Postconditions (checked before return)
    pub ensures: Vec<String>,
    /// Effect annotations (for documentation/verification)
    pub effects: Vec<String>,
}

impl IRFunction {
    pub fn new(name: &str, params: Vec<(String, IRType)>, ret_type: IRType) -> Self {
        Self {
            name: name.to_string(),
            params,
            ret_type,
            sret_type: None,
            blocks: Vec::new(),
            entry_block: BlockId(0),
            contracts: IRContracts::default(),
            simd: false,
            naked: false,
            interrupt: false,
        }
    }

    pub fn add_block(&mut self, label: &str) -> BlockId {
        let id = BlockId(self.blocks.len());
        self.blocks.push(BasicBlock::new(id, label));
        id
    }

    pub fn get_block_mut(&mut self, id: BlockId) -> Option<&mut BasicBlock> {
        self.blocks.get_mut(id.0)
    }
}

/// Basic Block - a sequence of instructions with single entry/exit
#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub label: String,
    pub instructions: Vec<Instruction>,
    pub terminator: Option<Terminator>,
}

impl BasicBlock {
    pub fn new(id: BlockId, label: &str) -> Self {
        Self {
            id,
            label: label.to_string(),
            instructions: Vec::new(),
            terminator: None,
        }
    }

    pub fn push(&mut self, inst: Instruction) {
        self.instructions.push(inst);
    }

    pub fn set_terminator(&mut self, term: Terminator) {
        self.terminator = Some(term);
    }
}

/// Block identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

/// Virtual register
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register(pub usize);

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// IR Instruction (non-terminating)
#[derive(Debug, Clone)]
pub enum Instruction {
    /// dest = value
    Assign { dest: Register, value: Value },
    
    /// dest = left op right
    BinOp { dest: Register, op: BinOp, left: Value, right: Value },
    
    /// dest = op value
    UnaryOp { dest: Register, op: UnaryOp, value: Value },
    
    /// dest = func(args...)
    Call { dest: Option<Register>, func: String, args: Vec<Value> },
    
    /// dest = alloca type
    Alloca { dest: Register, ty: IRType },
    
    /// dest = load ptr (with explicit element type for LLVM opaque pointers)
    Load { dest: Register, ptr: Value, ty: IRType },
    
    /// store value, ptr
    Store { ptr: Value, value: Value },
    
    /// dest = gep ptr, index (with explicit element type for LLVM opaque pointers)
    GetElementPtr { dest: Register, ptr: Value, index: Value, elem_ty: IRType },
    
    /// dest = phi [(val1, block1), (val2, block2), ...]
    Phi { dest: Register, incoming: Vec<(Value, BlockId)> },

    /// dest = cast value to ty
    Cast { dest: Register, value: Value, ty: IRType },

    /// Inline Assembly (Phase 11)
    InlineAsm {
        template: String,
        operands: Vec<IRAsmOperand>,
    },
}

#[derive(Debug, Clone)]
pub struct IRAsmOperand {
    pub kind: IRAsmOperandKind,
    pub constraint: String,
    pub input: Option<Value>,
    pub output: Option<Register>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IRAsmOperandKind {
    Input,
    Output,
    InOut,
    Clobber,
}

/// Block terminator
#[derive(Debug, Clone)]
pub enum Terminator {
    /// return value
    Return { value: Option<Value> },
    
    /// br target
    Jump { target: BlockId },
    
    /// br cond, then_target, else_target
    Branch { cond: Value, then_target: BlockId, else_target: BlockId },
    
    /// unreachable
    Unreachable,
}

/// IR Value
#[derive(Debug, Clone)]
pub enum Value {
    Register(Register),
    Constant(Constant),
    Parameter(usize),
    Global(String),
    Unit,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Register(r) => write!(f, "{}", r),
            Value::Constant(c) => write!(f, "{}", c),
            Value::Parameter(i) => write!(f, "arg{}", i),
            Value::Global(name) => write!(f, "@{}", name),
            Value::Unit => write!(f, "()"),
        }
    }
}

/// Constant value
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Null,
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constant::Int(n) => write!(f, "{}", n),
            Constant::Float(n) => write!(f, "{}", n),
            Constant::Bool(b) => write!(f, "{}", b),
            Constant::String(s) => write!(f, "\"{}\"", s),
            Constant::Null => write!(f, "null"),
        }
    }
}

/// Binary operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add, Sub, Mul, Div, Mod,
    // Comparison
    Eq, Ne, Lt, Le, Gt, Ge,
    // Bitwise
    And, Or, Xor, Shl, Shr,
}

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::Div => "div",
            BinOp::Mod => "mod",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::Lt => "lt",
            BinOp::Le => "le",
            BinOp::Gt => "gt",
            BinOp::Ge => "ge",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::Shl => "shl",
            BinOp::Shr => "shr",
        };
        write!(f, "{}", s)
    }
}

/// Unary operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

/// IR Type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IRType {
    Void,
    Bool,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
    Ptr(Box<IRType>),
    Array(Box<IRType>, usize),
    Struct(String),
    Function { params: Vec<IRType>, ret: Box<IRType> },
    /// SIMD vector type: Vector(element_type, lane_count)
    /// e.g., Vector(F32, 4) = f32x4, Vector(F64, 2) = f64x2
    Vector(Box<IRType>, usize),
}

impl IRType {
    pub fn size_bytes(&self) -> usize {
        match self {
            IRType::Void => 0,
            IRType::Bool | IRType::I8 | IRType::U8 => 1,
            IRType::I16 | IRType::U16 => 2,
            IRType::I32 | IRType::U32 | IRType::F32 => 4,
            IRType::I64 | IRType::U64 | IRType::F64 | IRType::Ptr(_) => 8,
            IRType::Array(elem, count) => elem.size_bytes() * count,
            IRType::Struct(_) => 8, // Placeholder
            IRType::Function { .. } => 8,
            IRType::Vector(elem, lanes) => elem.size_bytes() * lanes,
        }
    }
}
