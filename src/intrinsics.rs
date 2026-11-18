//! One inventory of low-level memory operations shared by checking and lowering.
use crate::types::{PrimitiveType as P, ResolvedType as T};
#[path = "host_builtins.rs"]
mod host;
pub use host::{host_builtin, host_builtin_names, host_c_signature, HostBuiltin, NumberConversion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    VolatileRead,
    VolatileWrite,
    AtomicLoad,
    AtomicStore,
    FetchAdd,
    FetchSub,
    CompareExchange,
    Fence,
}
#[derive(Debug, Clone)]
pub struct MemorySpec {
    pub kind: MemoryKind,
    pub element: P,
    pub explicit_order: bool,
}
impl MemorySpec {
    pub fn params(&self) -> Vec<T> {
        let element = T::Primitive(self.element.clone());
        let mut params = if self.kind == MemoryKind::Fence {
            vec![]
        } else {
            vec![T::Pointer(Box::new(element.clone()))]
        };
        match self.kind {
            MemoryKind::VolatileWrite
            | MemoryKind::AtomicStore
            | MemoryKind::FetchAdd
            | MemoryKind::FetchSub => params.push(element.clone()),
            MemoryKind::CompareExchange => {
                params.push(element.clone());
                params.push(element);
            }
            _ => {}
        }
        if self.explicit_order {
            params.push(T::I32);
            if self.kind == MemoryKind::CompareExchange {
                params.push(T::I32);
            }
        }
        params
    }
    pub fn result(&self) -> T {
        match self.kind {
            MemoryKind::VolatileWrite | MemoryKind::AtomicStore | MemoryKind::Fence => T::UNIT,
            MemoryKind::CompareExchange => T::BOOL,
            _ => T::Primitive(self.element.clone()),
        }
    }
}
pub fn memory(name: &str) -> Option<MemorySpec> {
    if name == "__atomic_thread_fence" || name == "__atomic_thread_fence_explicit" {
        return Some(MemorySpec {
            kind: MemoryKind::Fence,
            element: P::U8,
            explicit_order: name.ends_with("_explicit"),
        });
    }
    let (explicit_order, name) = if let Some(name) = name.strip_suffix("_explicit") {
        (true, name)
    } else {
        (false, name)
    };
    for (suffix, element) in [
        ("u8", P::U8),
        ("u16", P::U16),
        ("u32", P::U32),
        ("u64", P::U64),
    ] {
        let Some(prefix) = name.strip_suffix(&format!("_{}", suffix)) else {
            continue;
        };
        let kind = match prefix {
            "__volatile_read" if !explicit_order => MemoryKind::VolatileRead,
            "__volatile_write" if !explicit_order => MemoryKind::VolatileWrite,
            "__atomic_load" => MemoryKind::AtomicLoad,
            "__atomic_store" => MemoryKind::AtomicStore,
            "__atomic_fetch_add" => MemoryKind::FetchAdd,
            "__atomic_fetch_sub" => MemoryKind::FetchSub,
            "__atomic_compare_exchange" => MemoryKind::CompareExchange,
            _ => return None,
        };
        return Some(MemorySpec {
            kind,
            element,
            explicit_order,
        });
    }
    None
}
pub fn memory_names() -> Vec<String> {
    let mut names = vec![
        "__atomic_thread_fence".into(),
        "__atomic_thread_fence_explicit".into(),
    ];
    for suffix in ["u8", "u16", "u32", "u64"] {
        for prefix in [
            "__volatile_read",
            "__volatile_write",
            "__atomic_load",
            "__atomic_store",
            "__atomic_fetch_add",
            "__atomic_fetch_sub",
            "__atomic_compare_exchange",
        ] {
            let name = format!("{}_{}", prefix, suffix);
            names.push(name.clone());
            if prefix.starts_with("__atomic_") {
                names.push(format!("{}_explicit", name));
            }
        }
    }
    names
}

/// Public constants: relaxed=0, acquire=1, release=2, acq_rel=3, seq_cst=4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOrder {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}
impl MemoryOrder {
    pub fn from_i64(value: i64) -> Option<Self> {
        Some(match value {
            0 => Self::Relaxed,
            1 => Self::Acquire,
            2 => Self::Release,
            3 => Self::AcqRel,
            4 => Self::SeqCst,
            _ => return None,
        })
    }
    pub fn c_name(self) -> &'static str {
        match self {
            Self::Relaxed => "__ATOMIC_RELAXED",
            Self::Acquire => "__ATOMIC_ACQUIRE",
            Self::Release => "__ATOMIC_RELEASE",
            Self::AcqRel => "__ATOMIC_ACQ_REL",
            Self::SeqCst => "__ATOMIC_SEQ_CST",
        }
    }
}
pub fn validate_orders(
    kind: MemoryKind,
    success: MemoryOrder,
    failure: MemoryOrder,
) -> Result<(), String> {
    use MemoryKind as K;
    use MemoryOrder as O;
    if (kind == K::AtomicLoad && matches!(success, O::Release | O::AcqRel))
        || (kind == K::AtomicStore && matches!(success, O::Acquire | O::AcqRel))
        || (kind == K::Fence && success == O::Relaxed)
    {
        return Err("memory order is not valid for this operation".into());
    }
    if kind == K::CompareExchange {
        let allowed = match success {
            O::Relaxed | O::Release => failure == O::Relaxed,
            O::Acquire | O::AcqRel => matches!(failure, O::Relaxed | O::Acquire),
            O::SeqCst => matches!(failure, O::Relaxed | O::Acquire | O::SeqCst),
        };
        if !allowed {
            return Err(
                "compare_exchange failure order cannot release or be stronger than success".into(),
            );
        }
    }
    Ok(())
}

/// SIMD operations share one signature inventory with both lowering backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorKind { New, Splat, Add, Sub, Mul, Div, Sum, Extract, Load, Store, Eq, Ne, Lt, Le, Gt, Ge, All, Any }
#[derive(Debug, Clone)]
pub struct VectorSpec { pub kind: VectorKind, pub element: P, pub lanes: usize }
impl VectorSpec {
    pub fn vector(&self) -> T { T::Vector(Box::new(T::Primitive(self.element.clone())), self.lanes) }
    pub fn params(&self, width: crate::types::PointerWidth) -> Vec<T> {
        use VectorKind::*;
        let scalar = T::Primitive(self.element.clone()); let vector = self.vector();
        match self.kind {
            New => vec![scalar; self.lanes], Splat => vec![scalar],
            Load => vec![T::Pointer(Box::new(scalar))], Store => vec![T::Pointer(Box::new(scalar)), vector],
            Extract => vec![vector, T::Primitive(P::Usize(width))],
            Sum | All | Any => vec![vector], _ => vec![vector.clone(), vector],
        }
    }
    pub fn result(&self) -> T {
        use VectorKind::*;
        match self.kind {
            Sum | Extract => T::Primitive(self.element.clone()), Store => T::UNIT,
            All | Any => T::BOOL,
            Eq | Ne | Lt | Le | Gt | Ge => T::Vector(Box::new(T::Primitive(if matches!(self.element, P::F64 | P::I64) {P::I64} else {P::I32})),self.lanes),
            _ => self.vector(),
        }
    }
}
pub fn vector(name: &str) -> Option<VectorSpec> {
    let name = name.strip_prefix("__simd_").unwrap_or(name);
    let (shape, operation) = name.rsplit_once('_')?;
    let (element,lanes) = match shape { "f32x4"=>(P::F32,4), "f64x2"=>(P::F64,2), "i32x4"=>(P::I32,4), "i64x2"=>(P::I64,2), _=>return None };
    use VectorKind::*;
    let kind=match operation {"new"=>New,"splat"=>Splat,"add"=>Add,"sub"=>Sub,"mul"=>Mul,"div" if matches!(element,P::F32|P::F64)=>Div,"sum"=>Sum,"extract"=>Extract,"load"=>Load,"store"=>Store,"eq"=>Eq,"ne"=>Ne,"lt"=>Lt,"le"=>Le,"gt"=>Gt,"ge"=>Ge,"all" if matches!(element,P::I32|P::I64)=>All,"any" if matches!(element,P::I32|P::I64)=>Any,_=>return None};
    Some(VectorSpec{kind,element,lanes})
}
pub fn vector_names() -> Vec<String> {
    let mut names=vec![];
    for shape in ["f32x4","f64x2","i32x4","i64x2"] {
        for operation in ["new","splat","add","sub","mul","div","sum","extract","load","store","eq","ne","lt","le","gt","ge","all","any"] {
            let name=format!("{shape}_{operation}");
            if vector(&name).is_some() { names.push(format!("__simd_{name}")); names.push(name); }
        }
    }
    names
}

/// Target-libc operations whose spelling/layout must not leak into source libraries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformOp { ErrnoGet }
impl PlatformOp {
    pub fn params(self) -> Vec<crate::types::ResolvedType> { vec![] }
    pub fn result(self) -> crate::types::ResolvedType { crate::types::ResolvedType::I32 }
    pub fn libc_symbol(self, target: &str) -> Result<&'static str, String> {
        let native;
        let target = if target == "native" {
            native = target_lexicon::HOST.to_string();
            &native
        } else { target };
        match self {
            Self::ErrnoGet if target.contains("apple") => Ok("__error"),
            Self::ErrnoGet if target.contains("android") => Ok("__errno"),
            Self::ErrnoGet if target.contains("linux") => Ok("__errno_location"),
            Self::ErrnoGet if target.contains("windows") => Ok("_errno"),
            Self::ErrnoGet if target.contains("freebsd") => Ok("__error"),
            _ => Err(format!("errno access is not implemented for target '{target}'")),
        }
    }
}
pub fn platform(name: &str) -> Option<PlatformOp> {
    match name { "__errno_get" => Some(PlatformOp::ErrnoGet), _ => None }
}
pub fn platform_names() -> [&'static str; 1] { ["__errno_get"] }
pub fn is_reserved(name: &str) -> bool {
    memory(name).is_some() || (name.starts_with("__simd_") && vector(name).is_some()) || platform(name).is_some()
}
