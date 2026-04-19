#[cfg(not(feature = "llvm-backend"))]
use anyhow::{Result, bail};

#[cfg(not(feature = "llvm-backend"))]
use crate::mir::MirProgram;

#[cfg(feature = "llvm-backend")]
mod llvm_backend;

#[cfg(all(feature = "llvm-backend", target_os = "windows"))]
mod llvm_windows_shims;

#[cfg(feature = "llvm-backend")]
pub use llvm_backend::emit_llvm_ir;

#[cfg(not(feature = "llvm-backend"))]
pub fn emit_llvm_ir(_program: &MirProgram, _module_name: &str) -> Result<String> {
    bail!(
        "LLVM backend is disabled in this build. Rebuild with '--features llvm-backend' to use the 'ir' command."
    )
}
