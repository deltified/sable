use core::ffi::c_int;

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllTargets() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllTargetInfos() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllAsmParsers() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllAsmPrinters() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllDisassemblers() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeAllTargetMCs() {}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeNativeTarget() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeNativeAsmPrinter() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeNativeAsmParser() -> c_int {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn LLVM_InitializeNativeDisassembler() -> c_int {
    0
}
