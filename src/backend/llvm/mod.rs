//! LLVM Backend
#![allow(unused_imports, dead_code)]

mod llvm_codegen;

pub use llvm_codegen::LLVMCodeGen;

#[test]
fn linked_llvm_matches_selected_feature() {
    let mut major = 0;
    let mut minor = 0;
    unsafe { llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, std::ptr::null_mut()) };
    let expected_major = if cfg!(feature = "llvm23") { 23 } else { 21 };
    assert_eq!((major, minor), (expected_major, 1));
}
