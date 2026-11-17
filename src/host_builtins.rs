//! Hosted language functions. Explicit declarations may shadow these names.
use crate::frontend::ast::{Effect, EffectSet};
use crate::types::{PointerWidth, PrimitiveType as P, ResolvedType as T};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostBuiltin {
    Print,
    Println,
    PrintI64,
    PrintlnI64,
    Puts,
    Alloc,
    Malloc,
    Free,
    Atof,
    Strcmp,
    Exit,
    Assert,
}

pub fn host_builtin_names() -> &'static [&'static str] {
    &[
        "print",
        "println",
        "print_i64",
        "println_i64",
        "puts",
        "alloc",
        "malloc",
        "free",
        "atof",
        "strcmp",
        "exit",
        "assert",
    ]
}

pub fn host_builtin(name: &str) -> Option<HostBuiltin> {
    use HostBuiltin::*;
    Some(match name {
        "print" => Print,
        "println" => Println,
        "print_i64" => PrintI64,
        "println_i64" => PrintlnI64,
        "puts" => Puts,
        "alloc" => Alloc,
        "malloc" => Malloc,
        "free" => Free,
        "atof" => Atof,
        "strcmp" => Strcmp,
        "exit" => Exit,
        "assert" => Assert,
        _ => return None,
    })
}

impl HostBuiltin {
    pub fn params(self, width: PointerWidth) -> Vec<T> {
        let bytes = T::Pointer(Box::new(T::U8));
        use HostBuiltin::*;
        match self {
            Print | Println | Puts | Free | Atof => vec![bytes],
            PrintI64 | PrintlnI64 => vec![T::I64],
            // Preserve existing u64 calls on 64-bit hosts; never narrow a
            // u64 argument silently when compiling for a 32-bit target.
            Alloc | Malloc => vec![T::Primitive(match width {
                PointerWidth::W32 => P::U32,
                PointerWidth::W64 => P::U64,
            })],
            Strcmp => vec![bytes.clone(), bytes],
            Exit => vec![T::I32],
            Assert => vec![T::BOOL],
        }
    }

    pub fn result(self) -> T {
        use HostBuiltin::*;
        match self {
            Alloc | Malloc => T::Pointer(Box::new(T::U8)),
            Puts | Strcmp => T::I32,
            Atof => T::F64,
            Exit => T::NEVER,
            _ => T::UNIT,
        }
    }

    pub fn effects(self) -> EffectSet {
        use HostBuiltin::*;
        let effects = match self {
            Print | Println | PrintI64 | PrintlnI64 | Puts => vec![Effect::IO],
            Alloc | Malloc | Free => vec![Effect::Alloc],
            // Reads through raw pointers are effects even without mutation.
            Atof | Strcmp => vec![Effect::Read],
            Exit | Assert => vec![Effect::Panic],
        };
        EffectSet {
            is_pure: false,
            effects,
        }
    }
}

/// Fixed platform C names are ABI contracts in hosted programs. Language
/// aliases such as `print` and `alloc` deliberately do not appear here.
pub fn host_c_signature(name: &str, width: PointerWidth) -> Option<(Vec<T>, T, bool)> {
    let bytes = T::Pointer(Box::new(T::U8));
    let size = T::Primitive(match width {
        PointerWidth::W32 => P::U32,
        PointerWidth::W64 => P::U64,
    });
    Some(match name {
        "printf" => (vec![bytes], T::I32, true),
        "puts" => (vec![bytes], T::I32, false),
        "malloc" => (vec![size], bytes, false),
        "realloc" => (vec![bytes.clone(), size], bytes, false),
        "memcpy" => (vec![bytes.clone(), bytes.clone(), size], bytes, false),
        "strlen" => (vec![bytes], size, false),
        "free" => (vec![bytes], T::UNIT, false),
        "exit" => (vec![T::I32], T::UNIT, false),
        "strcmp" => (vec![bytes.clone(), bytes], T::I32, false),
        "atof" => (vec![bytes], T::F64, false),
        _ => return None,
    })
}
