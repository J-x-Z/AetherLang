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
    /// Union fields overlap at offset zero; the type keeps maximum field size/alignment.
    pub is_union: bool,
}

/// IR Enum variant
#[derive(Debug, Clone)]
pub struct IRVariant {
    pub name: String,
    /// Fields of this variant (empty for unit variants)
    pub fields: Vec<IRType>,
}

/// IR Enum definition (sum type / tagged union)
#[derive(Debug, Clone)]
pub struct IREnum {
    pub name: String,
    pub variants: Vec<IRVariant>,
}

/// IR Module - contains all functions
#[derive(Debug, Clone)]
pub struct IRModule {
    pub pointer_width: crate::types::PointerWidth,
    pub name: String,
    pub functions: Vec<IRFunction>,
    pub structs: Vec<IRStruct>,
    pub enums: Vec<IREnum>,
    pub externs: Vec<IRExtern>,
    pub globals: Vec<IRGlobal>,
    /// #![no_std] - disable standard library
    pub no_std: bool,
    /// #![no_main] - no default main entry point
    pub no_main: bool,
}

/// Static storage is distinct from a function-local value. None denotes an
/// external declaration; Zero is a definition with zero-initialized storage.
#[derive(Debug, Clone)]
pub struct IRGlobal {
    pub name: String,
    pub ty: IRType,
    pub mutable: bool,
    pub initializer: Option<IRInitializer>,
}

#[derive(Debug, Clone)]
pub enum IRInitializer {
    Zero,
    Scalar(Constant),
    Aggregate(Vec<(usize, IRInitializer)>),
    Address { global: String, path: Vec<usize> },
    Layout { query: IRLayoutQuery, ty: IRType, field: Option<usize> },
}

#[derive(Debug, Clone, Copy)]
pub enum IRLayoutQuery { Size, Align, Offset }

/// External function declaration
#[derive(Debug, Clone)]
pub struct IRExtern {
    pub name: String,
    pub variadic: bool,
    pub params: Vec<(String, IRType)>,
    pub ret_type: IRType,
}

impl IRModule {
    pub fn new(name: &str) -> Self {
        Self {
            pointer_width: Default::default(),
            name: name.to_string(),
            functions: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            externs: Vec::new(),
            globals: Vec::new(),
            no_std: false,
            no_main: false,
        }
    }
    
    pub fn add_struct(&mut self, name: &str, fields: Vec<(String, IRType)>, repr: StructRepr) {
        self.structs.push(IRStruct {
            name: name.to_string(),
            fields,
            repr,
            is_union: false,
        });
    }
}


/// IR Function
#[derive(Debug, Clone)]
pub struct IRFunction {
    pub name: String,
    pub params: Vec<(String, IRType)>,
    pub ret_type: IRType,
    /// Explicit C ABI export (main or #[no_mangle]); ordinary calls use the language ABI.
    pub c_abi: bool,
    pub blocks: Vec<BasicBlock>,
    pub entry_block: BlockId,
    /// Canonical types assigned by lowering, shared by every backend.
    pub register_types: std::collections::HashMap<Register, IRType>,
    /// SIMD annotation - enable auto-vectorization hints
    pub simd: bool,
    /// Naked function - no prologue/epilogue (for asm)
    pub naked: bool,
    /// Interrupt handler function
    pub interrupt: bool,
    /// Volatile function - all memory accesses are volatile (for MMIO)
    pub volatile: bool,
}

impl IRFunction {
    pub fn new(name: &str, params: Vec<(String, IRType)>, ret_type: IRType) -> Self {
        Self {
            name: name.to_string(),
            params,
            ret_type,
            c_abi: name == "main",
            blocks: Vec::new(),
            entry_block: BlockId(0),
            register_types: std::collections::HashMap::new(),
            simd: false,
            naked: false,
            interrupt: false,
            volatile: false,
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
    /// Typed SIMD operation with lane order independent of the host instruction set.
    Platform { dest: Register, op: crate::intrinsics::PlatformOp },
    Vector { dest: Option<Register>, kind: crate::intrinsics::VectorKind, ty: IRType, args: Vec<Value> },
    /// Typed volatile/atomic operations, with checked compile-time ordering.
    Memory { dest: Option<Register>, kind: crate::intrinsics::MemoryKind,
        ty: IRType, args: Vec<Value>, order: crate::intrinsics::MemoryOrder,
        failure_order: crate::intrinsics::MemoryOrder },
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
    
    /// Initialize object representation bytes before assigning active fields.
    ZeroInitialize { ptr: Value, ty: IRType },

    /// store value, ptr
    Store { ptr: Value, value: Value },
    
    /// dest = gep ptr, index (with explicit element type for LLVM opaque pointers)
    GetElementPtr { dest: Register, ptr: Value, index: Value, elem_ty: IRType },
    
    /// Address an element within a fixed array value (two-index GEP).
    ArrayElementPtr { dest: Register, ptr: Value, index: Value, array_ty: IRType },

    /// Address a struct field; distinct from indexing an array of structs.
    GetFieldPtr { dest: Register, ptr: Value, field: usize, struct_ty: IRType },

    /// dest = phi [(val1, block1), (val2, block2), ...]
    Phi { dest: Register, incoming: Vec<(Value, BlockId)> },

    /// Target-dependent object layout, evaluated after the target is selected.
    Layout { dest: Register, query: IRLayoutQuery, ty: IRType, field: Option<usize> },

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
    
    /// Executable failure, preserved independently of debug assertions.
    Trap,

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

/// Aether asm placeholders use source operand order ($0, $1, ...). Native
/// constraints put outputs before inputs; both backends use this same mapping.
pub fn render_asm_template(template: &str, operands: &[IRAsmOperand], c_syntax: bool) -> std::result::Result<String, String> {
    let output_count = operands.iter().filter(|op| op.output.is_some()).count();
    let mut output_index = 0;
    let mut input_index = output_count;
    let mut mapping = Vec::new();
    for operand in operands {
        if operand.output.is_some() { mapping.push(output_index); output_index += 1; }
        else if matches!(operand.kind, IRAsmOperandKind::Input) { mapping.push(input_index); input_index += 1; }
    }
    let mut chars = template.chars().peekable();
    let mut result = String::new();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'$') { chars.next(); result.push_str(if c_syntax { "$" } else { "$$" }); continue; }
            let mut digits = String::new();
            while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) { digits.push(chars.next().unwrap()); }
            if digits.is_empty() { return Err("asm literal '$' must be escaped as '$$'".into()); }
            let index: usize = digits.parse().map_err(|_| "invalid asm operand index")?;
            let mapped = mapping.get(index).ok_or_else(|| format!("asm operand ${index} does not exist"))?;
            result.push(if c_syntax { '%' } else { '$' }); result.push_str(&mapped.to_string());
        } else if ch == '%' && c_syntax { result.push_str("%%"); }
        else { result.push(ch); }
    }
    Ok(result)
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
    /// No ABI result; never a stored language value.
    Void,
    /// The language unit value: zero bytes with alignment one.
    Unit,
    Bool,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
    Ptr(Box<IRType>),
    /// Volatile storage qualifier; loads produce the unqualified value type.
    Volatile(Box<IRType>),
    Array(Box<IRType>, usize),
    Struct(String),
    Function { params: Vec<IRType>, ret: Box<IRType> },
    /// SIMD vector type: Vector(element_type, lane_count)
    /// e.g., Vector(F32, 4) = f32x4, Vector(F64, 2) = f64x2
    Vector(Box<IRType>, usize),
}

impl IRType {
    pub fn unqualified(&self) -> &Self { match self { Self::Volatile(inner) => inner.unqualified(), other => other } }

    pub fn integer_bits(&self) -> Option<u32> {
        match self {
            Self::Volatile(inner) => inner.integer_bits(),
            Self::I8 | Self::U8 => Some(8), Self::I16 | Self::U16 => Some(16),
            Self::I32 | Self::U32 => Some(32), Self::I64 | Self::U64 => Some(64),
            _ => None,
        }
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(self.unqualified(), Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    pub fn size_bytes(&self) -> usize {
        match self {
            IRType::Void | IRType::Unit => 0,
            IRType::Bool | IRType::I8 | IRType::U8 => 1,
            IRType::I16 | IRType::U16 => 2,
            IRType::I32 | IRType::U32 | IRType::F32 => 4,
            IRType::I64 | IRType::U64 | IRType::F64 | IRType::Ptr(_) => 8,
            IRType::Volatile(inner) => inner.size_bytes(),
            IRType::Array(elem, count) => elem.size_bytes() * count,
            IRType::Struct(_) => 8, // Placeholder
            IRType::Function { .. } => 8,
            IRType::Vector(elem, lanes) => elem.size_bytes() * lanes,
        }
    }
}
