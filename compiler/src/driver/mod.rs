use std::path::Path;

use anyhow::{Result, bail};

use crate::codegen;
use crate::diagnostics::Diagnostics;
use crate::lexer;
use crate::mir;
use crate::parser;
use crate::runtime;
use crate::sema;
use crate::source::SourceDb;

pub fn run() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        print_usage();
        bail!("missing required arguments");
    }

    let command = &args[1];
    let input = &args[2];

    let mut source_db = SourceDb::new();
    let file_id = source_db.add_file(Path::new(input))?;
    let source = source_db.source(file_id);

    let (tokens, mut diagnostics) = lexer::lex(file_id, source);
    if command == "tokens" {
        diagnostics.sort_deterministically();
        for token in &tokens {
            println!(
                "{:?} '{}' @ {}..{}",
                token.kind, token.text, token.span.start, token.span.end
            );
        }
        emit_and_fail_if_errors(diagnostics, &source_db)?;
        return Ok(());
    }

    let (module, parse_diags) = parser::parse(tokens);
    diagnostics.extend(parse_diags);

    match command.as_str() {
        "ast" => {
            diagnostics.sort_deterministically();
            emit_and_fail_if_errors(diagnostics, &source_db)?;
            println!("{module:#?}");
            Ok(())
        }
        "check" => {
            let (checked, sema_diags) = sema::check(&module);
            diagnostics.extend(sema_diags);
            diagnostics.sort_deterministically();
            emit_and_fail_if_errors(diagnostics, &source_db)?;

            let mut mir_program = mir::lower(&module, &checked)?;
            mir::optimize(&mut mir_program);

            println!("check succeeded");
            Ok(())
        }
        "mir" => {
            let (checked, sema_diags) = sema::check(&module);
            diagnostics.extend(sema_diags);
            diagnostics.sort_deterministically();
            emit_and_fail_if_errors(diagnostics, &source_db)?;

            let mut mir_program = mir::lower(&module, &checked)?;
            mir::optimize(&mut mir_program);
            println!("{mir_program:#?}");
            Ok(())
        }
        "ir" => {
            let (checked, sema_diags) = sema::check(&module);
            diagnostics.extend(sema_diags);
            diagnostics.sort_deterministically();
            emit_and_fail_if_errors(diagnostics, &source_db)?;

            let mut mir_program = mir::lower(&module, &checked)?;
            mir::optimize(&mut mir_program);

            let ir = codegen::emit_llvm_ir(&mir_program, "sable")?;
            println!("{ir}");
            Ok(())
        }
        "run" => {
            let (checked, sema_diags) = sema::check(&module);
            diagnostics.extend(sema_diags);
            diagnostics.sort_deterministically();
            emit_and_fail_if_errors(diagnostics, &source_db)?;

            let mut mir_program = mir::lower(&module, &checked)?;
            mir::optimize(&mut mir_program);

            let result = runtime::run_main(&mir_program)?;
            if let Some(value) = result {
                println!("program returned: {}", runtime::format_value(&value));
            } else {
                println!("program returned: void");
            }
            Ok(())
        }
        _ => {
            print_usage();
            bail!("unknown command: {}", command)
        }
    }
}

fn emit_and_fail_if_errors(mut diagnostics: Diagnostics, source_db: &SourceDb) -> Result<()> {
    diagnostics.sort_deterministically();
    if diagnostics.is_empty() {
        return Ok(());
    }

    eprint!("{}", diagnostics.render(source_db));
    if diagnostics.has_errors() {
        bail!("compilation failed")
    }

    Ok(())
}

fn print_usage() {
    eprintln!("usage: compiler <tokens|ast|check|mir|ir|run> <input.sable>");
}
