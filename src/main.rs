mod lexer;
mod parser;
mod ir;
mod codegen;

use std::process;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: kov <command> [args]");
        eprintln!();
        eprintln!("  build <file.kv> [-o output]   compile to RISC-V binary");
        eprintln!("  lex <file.kv>                 dump token stream");
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

    let source = read_file(&args[2]);
    match lexer::Lexer::tokenize(&source) {
        Ok(tokens) => {
            for tok in &tokens {
                println!("{:>4}..{:<4}  {:?}", tok.span.start, tok.span.end, tok.kind);
            }
            eprintln!("{} tokens", tokens.len());
        }
        Err(e) => die(&format!("lex error: {e}")),
    }
}

fn cmd_build(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov build <file.kv> [-o output]");
        process::exit(1);
    }

    let input = &args[2];
    let output = find_flag(&args, "-o").unwrap_or_else(|| {
        input.replace(".kv", ".bin")
    });

    let start = Instant::now();
    let source = read_file(input);

    let tokens = match lexer::Lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => die(&format!("lex error: {e}")),
    };

    let program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => die(&format!("{e}")),
    };

    let ir = ir::lower::Lowering::lower(&program);

    let mut cg = codegen::CodeGen::new();
    for func in &ir.functions {
        cg.gen_function(func);
    }

    let code = match cg.finish() {
        Ok(c) => c,
        Err(e) => die(&format!("codegen error: {e}")),
    };

    let elapsed = start.elapsed();

    let binary = if output.ends_with(".elf") {
        // default to GD32VF103 flash base for now
        let text_base = 0x0800_0000u32;
        codegen::elf::ElfWriter::new(text_base, text_base).write(&code)
    } else {
        code.clone()
    };

    if let Err(e) = std::fs::write(&output, &binary) {
        die(&format!("cannot write {output}: {e}"));
    }

    eprintln!("  compiled: {} → {} ({} bytes)", input, output, binary.len());
    eprintln!("  code:     {} bytes", code.len());
    eprintln!("  time:     {:.1}ms", elapsed.as_secs_f64() * 1000.0);
}

fn read_file(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => die(&format!("cannot read {path}: {e}")),
    }
}

fn find_flag(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1);
}
