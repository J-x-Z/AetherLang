//! Pointer-sized language integers follow the compilation target, including x32 ABIs.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PointerWidth { W32, W64 }

impl Default for PointerWidth {
    fn default() -> Self {
        if usize::BITS == 32 { Self::W32 } else { Self::W64 }
    }
}

impl PointerWidth {
    pub fn for_target(target: &str) -> Result<Self, String> {
        if target == "native" { return Ok(Self::default()); }
        let triple: target_lexicon::Triple = target.parse()
            .map_err(|error| format!("invalid target '{target}': {error}"))?;
        match triple.pointer_width() {
            Ok(target_lexicon::PointerWidth::U32) => Ok(Self::W32),
            Ok(target_lexicon::PointerWidth::U64) => Ok(Self::W64),
            _ => Err(format!("target '{target}' needs a supported 32- or 64-bit pointer layout")),
        }
    }
    pub fn bytes(self) -> usize {
        match self { Self::W32 => 4, Self::W64 => 8 }
    }
}
