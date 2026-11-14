//! One inventory of low-level memory operations shared by checking and lowering.
use crate::types::{PrimitiveType as P, ResolvedType as T};

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
