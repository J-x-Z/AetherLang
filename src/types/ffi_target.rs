//! Target families with verified LLVM lowering for by-value foreign types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForeignAbiTarget { DarwinAarch64, SysvX86_64, WindowsX86_64, Other }

impl ForeignAbiTarget {
    pub fn for_triple(target: &str) -> Self {
        let native;
        let triple = if target == "native" {
            native = target_lexicon::HOST.to_string();
            &native
        } else { target };
        let arch = triple.split('-').next().unwrap_or("");
        if matches!(arch, "aarch64" | "arm64") && triple.contains("-apple-") {
            Self::DarwinAarch64
        } else if arch == "x86_64" && triple.split('-').any(|part| part == "windows") {
            Self::WindowsX86_64
        } else if arch == "x86_64" && (triple.contains("-linux-") || triple.contains("-apple-")) &&
            !triple.contains("x32") {
            Self::SysvX86_64
        } else {
            Self::Other
        }
    }

    pub fn supports_aggregates(self) -> bool { self != Self::Other }
    pub fn supports_vector128(self) -> bool { matches!(self, Self::DarwinAarch64) }
}

pub const VECTOR_ABI_ERROR: &str = "foreign vector ABI requires a direct supported 128-bit vector on Apple arm64; use a scalar-pointer interface for nested or unsupported vectors";
pub const AGGREGATE_ABI_ERROR: &str = "aggregate C ABI lowering is currently implemented for Apple arm64, x86_64 SysV and Windows x64; use a pointer interface or the C backend on this target";
pub const WIN64_LAYOUT_ERROR: &str = "Windows x64 by-value C interfaces require a structure or union with nonzero-size fields; use a pointer for bare arrays or empty layouts";
