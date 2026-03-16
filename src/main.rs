#![allow(dead_code)]
#![allow(clippy::enum_variant_names, clippy::collapsible_if)]

mod codegen;
mod emu;
mod errors;
mod ir;
mod lexer;
mod parser;
mod types;

use std::process;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: kov <command> [args]");
        eprintln!();
        eprintln!("  build <file.kv> [-o output]   compile to binary");
        eprintln!("  run <file.kv> [-c cycles]     compile and execute");
        eprintln!("  check <file.kv>               type check only");
        eprintln!("  lex <file.kv>                 dump tokens");
        eprintln!();
        eprintln!("flags:");
        eprintln!("  --error-format=json           output errors as JSON");
        process::exit(1);
    }

    match args[1].as_str() {
        "lex" => cmd_lex(&args),
        "build" => cmd_build(&args),
        "run" => cmd_run(&args),
        "check" => cmd_check(&args),
        _ => {
            eprintln!("unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}

struct CompileResult {
    code: Vec<u8>,
    flash_base: u32,
    ram_base: u32,
    ram_top: u32,
    elapsed: std::time::Duration,
}

fn compile(source: &str) -> CompileResult {
    let start = Instant::now();

    let tokens = match lexer::Lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => die(&format!("lex error: {e}")),
    };

    let program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => die(&format!("{e}")),
    };

    match types::check::TypeChecker::new().check(&program) {
        Ok(warnings) => {
            for w in &warnings {
                eprint!(
                    "warning: {}\n",
                    errors::format_error(source, w.span, &w.message)
                );
            }
        }
        Err(type_errors) => {
            for e in &type_errors {
                eprint!("{}", errors::format_error(source, e.span, &e.message));
            }
            die(&format!("{} type error(s)", type_errors.len()));
        }
    }

    let board_name = program.items.iter().find_map(|item| {
        if let parser::ast::TopItem::Board(b) = item {
            Some(b.name.clone())
        } else {
            None
        }
    });

    let interrupts: Vec<(String, String)> = program
        .items
        .iter()
        .filter_map(|item| {
            if let parser::ast::TopItem::Interrupt(i) = item {
                Some((i.interrupt_name.clone(), i.fn_name.clone()))
            } else {
                None
            }
        })
        .collect();

    let board_config = board_name
        .as_deref()
        .and_then(codegen::startup::BoardConfig::from_name);

    let mut ir_result = ir::lower::Lowering::lower(&program);
    // optimize IR
    for func in &mut ir_result.functions {
        ir::opt::optimize(func);
    }
    let ram_base = board_config
        .as_ref()
        .map(|b| b.ram_start)
        .unwrap_or(0x2000_0000);
    let mut cg = codegen::CodeGen::new_with_globals(ram_base, &ir_result.globals);

    if let Some(ref board) = board_config {
        codegen::startup::emit_startup(&mut cg.emitter, board);
        for (_, fn_name) in &interrupts {
            codegen::startup::emit_interrupt_wrapper(&mut cg.emitter, fn_name);
        }
    }

    for func in &ir_result.functions {
        cg.gen_function(func);
    }

    let code = match cg.finish() {
        Ok(c) => c,
        Err(e) => die(&format!("codegen error: {e}")),
    };

    CompileResult {
        code,
        flash_base: board_config
            .as_ref()
            .map(|b| b.flash_start)
            .unwrap_or(0x0800_0000),
        ram_base: board_config
            .as_ref()
            .map(|b| b.ram_start)
            .unwrap_or(0x2000_0000),
        ram_top: board_config
            .as_ref()
            .map(|b| b.stack_top())
            .unwrap_or(0x2000_8000),
        elapsed: start.elapsed(),
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
    let output = find_flag(args, "-o").unwrap_or_else(|| input.replace(".kv", ".bin"));
    let source = read_file(input);
    let result = compile(&source);

    let binary = if output.ends_with(".elf") {
        codegen::elf::ElfWriter::new(result.flash_base, result.flash_base).write(&result.code)
    } else {
        result.code.clone()
    };

    if let Err(e) = std::fs::write(&output, &binary) {
        die(&format!("cannot write {output}: {e}"));
    }

    eprintln!(
        "  compiled: {} → {} ({} bytes)",
        input,
        output,
        binary.len()
    );
    eprintln!("  code:     {} bytes", result.code.len());
    eprintln!("  time:     {:.1}ms", result.elapsed.as_secs_f64() * 1000.0);
}

fn cmd_run(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov run <file.kv> [-c cycles]");
        process::exit(1);
    }

    let input = &args[2];
    let max_cycles: u64 = find_flag(args, "-c")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    let source = read_file(input);
    let result = compile(&source);

    eprintln!(
        "  compiled: {} bytes in {:.1}ms",
        result.code.len(),
        result.elapsed.as_secs_f64() * 1000.0
    );

    let mut cpu = emu::Cpu::with_memory(result.flash_base, result.flash_base, result.ram_base);
    cpu.mem.load_flash(&result.code);
    cpu.regs[2] = result.ram_top;

    let exec_start = Instant::now();
    cpu.run(max_cycles);
    let exec_time = exec_start.elapsed();

    eprintln!(
        "  executed: {} cycles in {:.1}ms",
        cpu.cycles,
        exec_time.as_secs_f64() * 1000.0
    );
    eprintln!("  halted:   {}", cpu.halted);

    // print MMIO activity
    let writes: Vec<_> = cpu.mem.mmio_log.iter().filter(|a| a.is_write).collect();
    if writes.is_empty() {
        eprintln!("  io:       (none)");
    } else {
        eprintln!("  io:       {} writes", writes.len());
        // group by address and show toggle pattern
        let mut last_addr = 0u32;
        let mut repeat_count = 0u32;
        for w in &writes {
            if w.address == last_addr {
                repeat_count += 1;
                if repeat_count <= 3 {
                    println!("            [{:#010X}] ← {:#X}", w.address, w.value);
                } else if repeat_count == 4 {
                    println!("            ... (repeating)");
                }
            } else {
                repeat_count = 0;
                last_addr = w.address;
                println!("            [{:#010X}] ← {:#X}", w.address, w.value);
            }
        }
    }

    // print register state
    eprintln!();
    eprintln!("  registers:");
    for i in (0..32).step_by(4) {
        eprintln!(
            "    x{:<2}={:#010X}  x{:<2}={:#010X}  x{:<2}={:#010X}  x{:<2}={:#010X}",
            i,
            cpu.regs[i],
            i + 1,
            cpu.regs[i + 1],
            i + 2,
            cpu.regs[i + 2],
            i + 3,
            cpu.regs[i + 3]
        );
    }
}

fn cmd_check(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov check <file.kv>");
        process::exit(1);
    }

    let input = &args[2];
    let json_mode = args.iter().any(|a| a == "--error-format=json");
    let source = read_file(input);

    let tokens = match lexer::Lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            if json_mode {
                println!(
                    "{}",
                    errors::format_error_json(
                        input,
                        &source,
                        lexer::token::Span::new(0, 0),
                        &format!("{e}"),
                        "error"
                    )
                );
            } else {
                eprintln!("error: {e}");
            }
            process::exit(1);
        }
    };

    let program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(e) => {
            if json_mode {
                println!(
                    "{}",
                    errors::format_error_json(
                        input,
                        &source,
                        lexer::token::Span::new(0, 0),
                        &format!("{e}"),
                        "error"
                    )
                );
            } else {
                eprintln!("error: {e}");
            }
            process::exit(1);
        }
    };

    match types::check::TypeChecker::new().check(&program) {
        Ok(warnings) => {
            for w in &warnings {
                if json_mode {
                    println!(
                        "{}",
                        errors::format_error_json(input, &source, w.span, &w.message, "warning")
                    );
                } else {
                    eprintln!("warning: {}", w.message);
                }
            }
            if !json_mode {
                eprintln!("  check: ok");
            }
        }
        Err(type_errors) => {
            for e in &type_errors {
                if json_mode {
                    println!(
                        "{}",
                        errors::format_error_json(input, &source, e.span, &e.message, "error")
                    );
                } else {
                    eprint!("{}", errors::format_error(&source, e.span, &e.message));
                }
            }
            process::exit(1);
        }
    }
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
