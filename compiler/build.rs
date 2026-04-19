use std::env;
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");

    if env::var_os("CARGO_FEATURE_LLVM_BACKEND").is_none() {
        return;
    }

    if env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let prefix = env::var("LLVM_SYS_221_PREFIX")
        .unwrap_or_else(|_| String::from(r"C:\Program Files\LLVM"));
    let lib_dir = Path::new(&prefix).join("lib");

    if !lib_dir.exists() {
        println!(
            "cargo:warning=LLVM lib directory not found at '{}'. Set LLVM_SYS_221_PREFIX to your LLVM install root.",
            lib_dir.display()
        );
        return;
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=LLVM-C");
}
