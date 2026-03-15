mod lexer;
mod parser;
mod ir;
mod codegen;

use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: kov <command> [args]");
        eprintln!();
        eprintln!("commands:");
        eprintln!("  build <file.kv>    compile to RISC-V binary");
        eprintln!("  lex <file.kv>      dump token stream (debug)");
        process::exit(1);
    }

    match args[1].as_str() {
        "lex" => cmd_lex(&args),
        "build" => cmd_build(&args),
        _ => {
            eprintln!("unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}

fn cmd_lex(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov lex <file.kv>");
        process::exit(1);
    }

    let source = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", args[2]);
            process::exit(1);
        }
    };

    match lexer::Lexer::tokenize(&source) {
        Ok(tokens) => {
            for tok in &tokens {
                println!("{:>4}..{:<4}  {:?}", tok.span.start, tok.span.end, tok.kind);
            }
            eprintln!("\n{} tokens", tokens.len());
        }
        Err(e) => {
            eprintln!("lex error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_build(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov build <file.kv>");
        process::exit(1);
    }

    let source = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", args[2]);
            process::exit(1);
        }
    };

    let tokens = match lexer::Lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            process::exit(1);
        }
    };

    eprintln!("lexed: {} tokens", tokens.len());
    eprintln!("parser not yet implemented");
    process::exit(1);
}
