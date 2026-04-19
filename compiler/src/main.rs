#![allow(dead_code)]

mod ast;
mod codegen;
mod diagnostics;
mod driver;
mod lexer;
mod mir;
mod parser;
mod sema;
mod source;

fn main() {
    if let Err(err) = driver::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
