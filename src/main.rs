#![allow(dead_code)]
#![allow(
    clippy::enum_variant_names,
    clippy::collapsible_if,
    clippy::type_complexity,
    clippy::ptr_arg
)]

mod build;
mod codegen;
mod emu;
mod errors;
mod ir;
mod lexer;
mod lsp;
mod parser;
mod pkg;
mod testing;
mod types;

use std::process;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("usage: kov <command> [args]");
        eprintln!();
        eprintln!("  build <file.kov> [-o output]   compile to binary");
        eprintln!("  run <file.kov> [-c cycles]     compile and execute");
        eprintln!("  asm <file.kov>                 show generated assembly");
        eprintln!("  trace <file.kov> [-c cycles]   compile, execute, output JSON trace");
        eprintln!("  wcet <file.kov>                worst-case execution time analysis");
        eprintln!("  flash <file.kov> [--chip X]    compile and flash to hardware");
        eprintln!("  test <file.kov>                run #[test] functions");
        eprintln!("  init <name> [--board X]       create new project");
        eprintln!("  add <package> [--git URL]     add a dependency");
        eprintln!("  boards                        list supported boards");
        eprintln!("  svd <file.svd> [--name X]     generate board def from SVD");
        eprintln!("  check <file.kov>               type check only");
        eprintln!("  lsp                           start language server");
        eprintln!("  lex <file.kov>                 dump tokens");
        eprintln!();
        eprintln!("flags:");
        eprintln!("  --error-format=json           output errors as JSON");
        process::exit(1);
    }

    match args[1].as_str() {
        "lex" => cmd_lex(&args),
        "build" => cmd_build(&args),
        "asm" => cmd_asm(&args),
        "run" => cmd_run(&args),
        "trace" => cmd_trace(&args),
        "wcet" => cmd_wcet(&args),
        "init" => {
            if args.len() < 3 {
                eprintln!("usage: kov init <name> [--board <board>]");
                process::exit(1);
            }
            let name = &args[2];
            let board = find_flag(&args, "--board").unwrap_or_else(|| "esp32c3".into());
            match pkg::init_project(name, &board) {
                Ok(()) => eprintln!("  created project: {}/", name),
                Err(e) => die(&e),
            }
        }
        "add" => {
            if args.len() < 3 {
                eprintln!("usage: kov add <package> [--git <url>]");
                process::exit(1);
            }
            let dep_name = &args[2];
            let git_url = find_flag(&args, "--git");

            let toml_path = "kov.toml";
            let content = std::fs::read_to_string(toml_path)
                .unwrap_or_else(|_| die("no kov.toml found in current directory"));
            let mut pkg = pkg::Package::from_toml(&content);
            pkg.deps.insert(
                dep_name.clone(),
                pkg::DepSpec {
                    git: git_url,
                    version: None,
                    path: None,
                },
            );
            if let Err(e) = std::fs::write(toml_path, pkg.to_toml()) {
                die(&format!("cannot write kov.toml: {e}"));
            }
            eprintln!("  added dependency: {}", dep_name);
        }
        "test" => {
            if args.len() < 3 {
                eprintln!("usage: kov test <file.kov>");
                process::exit(1);
            }
            let source = read_file(&args[2]);
            let results = testing::run_tests(&source);
            eprint!("{}", testing::format_results(&results));
            if results.iter().any(|r| !r.passed) {
                process::exit(1);
            }
        }
        "flash" => cmd_flash(&args),
        "boards" => {
            eprintln!("supported boards:");
            eprintln!("  esp32c3     ESP32-C3 (RISC-V, 400KB RAM, 160MHz)");
            eprintln!("  ch32v003    WCH CH32V003 (RISC-V, 2KB RAM, 48MHz, ~$0.10)");
            eprintln!("  gd32vf103   GigaDevice GD32VF103 (RISC-V, 32KB RAM, 108MHz)");
            eprintln!("  fe310       SiFive FE310 (RISC-V, 16KB RAM, 320MHz)");
            eprintln!("  stm32f4     STM32F4 (ARM Cortex-M4, 128KB RAM, 168MHz)");
            eprintln!("  nrf52840    Nordic nRF52840 (ARM Cortex-M4F, 256KB RAM, 64MHz)");
            eprintln!("  rp2040      Raspberry Pi Pico (ARM Cortex-M0+, 264KB RAM, 133MHz)");
        }
        "svd" => {
            if args.len() < 3 {
                eprintln!("usage: kov svd <file.svd> [--name <board>]");
                process::exit(1);
            }
            let xml = read_file(&args[2]);
            let name = find_flag(&args, "--name").unwrap_or_else(|| "myboard".into());
            let peripherals = codegen::svd::parse_svd(&xml);
            println!("{}", codegen::svd::generate_kov(&peripherals, &name));
        }
        "lsp" => lsp::run_lsp(),
        "check" => cmd_check(&args),
        _ => {
            eprintln!("unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}

struct CompileResult {
    code: Vec<u8>,
    compressed: Vec<u8>,
    labels: std::collections::HashMap<String, usize>,
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

    let mut program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(errors) => {
            for e in &errors {
                eprint!("{}", errors::format_error(source, e.span, &e.message));
            }
            die(&format!("{} parse error(s)", errors.len()));
        }
    };
    parser::monomorph::monomorphize(&mut program);

    match types::check::TypeChecker::new().check(&program) {
        Ok(warnings) => {
            for w in &warnings {
                eprintln!(
                    "warning: {}",
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

    // interrupt safety check
    let isr_check = types::interrupt::InterruptSafety::check(&program);
    for e in &isr_check.errors {
        eprintln!("warning: {e}");
    }

    // static assertions — evaluate const expressions at compile time
    for item in &program.items {
        if let parser::ast::TopItem::ConstAssert(expr, span) = item {
            if let parser::ast::Expr::BoolLit(false, _) = expr {
                die(&format!(
                    "static_assert failed at {}..{}",
                    span.start, span.end
                ));
            }
            // for complex expressions, try const eval via a wrapper function
            if let parser::ast::Expr::Binary(_, _, _, _) = expr {
                let wrapper = format!(
                    "fn __assert__() bool {{ return {}; }}",
                    source[span.start as usize..]
                        .split(')')
                        .next()
                        .unwrap_or("false")
                        .trim_start_matches("static_assert(")
                );
                if let Ok(tokens) = lexer::Lexer::tokenize(&wrapper) {
                    if let Ok(prog) = parser::Parser::new(tokens).parse() {
                        let mut ir = ir::lower::Lowering::lower(&prog);
                        for func in &mut ir.functions {
                            ir::opt::optimize(func);
                        }
                        if let Some(func) = ir.functions.first() {
                            if let Some(result) = ir::consteval::eval(func, &[]) {
                                if result == 0 {
                                    die("static_assert failed");
                                }
                            }
                        }
                    }
                }
            }
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
    // optimize IR: inline first, then per-function opts
    ir::opt::inline_functions(&mut ir_result.functions);
    for func in &mut ir_result.functions {
        ir::opt::optimize(func);
    }
    let ram_base = board_config
        .as_ref()
        .map(|b| b.ram_start)
        .unwrap_or(0x2000_0000);
    let mut cg = codegen::CodeGen::new_with_globals(ram_base, &ir_result.globals);

    // extract clock from board definition
    let clock_hz: u32 = program
        .items
        .iter()
        .find_map(|item| {
            if let parser::ast::TopItem::Board(b) = item {
                b.fields.iter().find_map(|f| {
                    if f.name == "clock" {
                        if let Some(parser::ast::Expr::IntLit(v, _)) = &f.address {
                            Some(*v as u32)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .unwrap_or(160_000_000);

    if let Some(ref board) = board_config {
        codegen::startup::emit_startup(&mut cg.emitter, board);
        codegen::builtins::emit_builtins(&mut cg.emitter, clock_hz);
        for (_, fn_name) in &interrupts {
            codegen::startup::emit_interrupt_wrapper(&mut cg.emitter, fn_name);
        }
    }

    for func in &ir_result.functions {
        cg.gen_function(func);
    }

    // enforce #[stack(N)] and #[max_cycles(N)] attributes
    for item in &program.items {
        if let parser::ast::TopItem::Function(f) = item {
            for attr in &f.attrs {
                if attr.name == "stack" {
                    if let Some(parser::ast::Expr::IntLit(limit, _)) = attr.args.first() {
                        let result = codegen::stack::analyze(
                            &ir_result.functions,
                            &f.name,
                            Some(*limit as u32),
                        );
                        if result.exceeded {
                            die(&format!(
                                "#[stack({})] on {}: worst-case stack depth is {} bytes (chain: {})",
                                limit,
                                f.name,
                                result.max_depth,
                                result.call_chain.join(" -> ")
                            ));
                        }
                    }
                }
                if attr.name == "max_cycles" {
                    if let Some(parser::ast::Expr::IntLit(limit, _)) = attr.args.first() {
                        let ir_func = ir_result.functions.iter().find(|func| func.name == f.name);
                        if let Some(ir_func) = ir_func {
                            let result = codegen::wcet::analyze(ir_func, Some(*limit as u32));
                            if result.exceeded {
                                die(&format!(
                                    "#[max_cycles({})] on {}: worst-case is {} cycles",
                                    limit, f.name, result.total_cycles
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    let labels = cg.emitter.labels.clone();
    let code = match cg.finish() {
        Ok(c) => c,
        Err(e) => die(&format!("codegen error: {e}")),
    };
    let compressed = codegen::compress::compress(&code);

    CompileResult {
        code,
        compressed,
        labels,
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
        eprintln!("usage: kov lex <file.kov>");
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
        eprintln!("usage: kov build <file.kov> [-o output]");
        process::exit(1);
    }

    let input = &args[2];
    let output = find_flag(args, "-o").unwrap_or_else(|| input.replace(".kov", ".bin"));
    let source = read_file(input);
    let result = compile(&source);

    let binary = if output.ends_with(".elf") {
        codegen::elf::ElfWriter::new(result.flash_base, result.flash_base).write(&result.compressed)
    } else {
        result.compressed.clone()
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
    eprintln!(
        "  code:     {} bytes ({} uncompressed)",
        result.compressed.len(),
        result.code.len()
    );
    eprintln!("  time:     {:.1}ms", result.elapsed.as_secs_f64() * 1000.0);
}

fn cmd_run(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov run <file.kov> [-c cycles]");
        process::exit(1);
    }

    let input = &args[2];
    let max_cycles: u64 = find_flag(args, "-c")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);

    let source = read_file(input);
    let result = compile(&source);

    eprintln!(
        "  compiled: {} bytes ({} compressed) in {:.1}ms",
        result.code.len(),
        result.compressed.len(),
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

fn cmd_asm(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov asm <file.kov>");
        process::exit(1);
    }
    let source = read_file(&args[2]);
    let result = compile(&source);
    println!(
        "{}",
        codegen::disasm::disassemble(&result.code, result.flash_base, &result.labels)
    );
}

fn cmd_trace(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov trace <file.kov> [-c cycles]");
        process::exit(1);
    }

    let input = &args[2];
    let max_cycles: u64 = find_flag(args, "-c")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000);

    let source = read_file(input);
    let result = compile(&source);

    let mut cpu = emu::Cpu::with_memory(result.flash_base, result.flash_base, result.ram_base);
    cpu.mem.load_flash(&result.code);
    cpu.regs[2] = result.ram_top;
    cpu.run_traced(max_cycles);

    if let Some(trace) = &cpu.trace {
        println!("{}", trace.to_json());
    }
}

fn cmd_wcet(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov wcet <file.kov>");
        process::exit(1);
    }

    let source = read_file(&args[2]);
    let result = compile(&source);

    let mut ir = ir::lower::Lowering::lower(
        &parser::Parser::new(lexer::Lexer::tokenize(&source).unwrap())
            .parse()
            .unwrap(),
    );
    for func in &mut ir.functions {
        ir::opt::optimize(func);
    }

    let results: Vec<_> = ir
        .functions
        .iter()
        .map(|f| codegen::wcet::analyze(f, None))
        .collect();

    eprintln!("  wcet analysis ({} functions):", results.len());
    eprint!("{}", codegen::wcet::format_report(&results));

    let stack_results: Vec<_> = ir
        .functions
        .iter()
        .map(|f| codegen::stack::analyze(&ir.functions, &f.name, None))
        .collect();

    eprintln!("  stack analysis:");
    eprint!("{}", codegen::stack::format_report(&stack_results));
    let _ = result;
}

fn cmd_flash(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov flash <file.kov> [--chip <name>]");
        process::exit(1);
    }

    let input = &args[2];
    let source = read_file(input);
    let result = compile(&source);

    // detect chip from board definition or --chip flag
    let chip = find_flag(args, "--chip").unwrap_or_else(|| {
        // try to extract from board name in source
        let board_name = result
            .labels
            .keys()
            .find(|k| k.starts_with("_start"))
            .map(|_| "esp32c3") // default
            .unwrap_or("esp32c3");
        board_name.to_string()
    });

    // write temporary ELF
    let elf_path = format!("{}.elf", input.trim_end_matches(".kov"));
    let elf = codegen::elf::ElfWriter::new(result.flash_base, result.flash_base)
        .write(&result.compressed);
    if let Err(e) = std::fs::write(&elf_path, &elf) {
        die(&format!("cannot write {elf_path}: {e}"));
    }

    eprintln!(
        "  compiled: {} bytes ({} compressed)",
        result.code.len(),
        result.compressed.len()
    );
    eprintln!("  flashing: {} → {}", input, chip);

    // invoke probe-rs
    let status = std::process::Command::new("probe-rs")
        .args(["download", "--chip", &chip, &elf_path])
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("  flash: ok");
            // reset
            let _ = std::process::Command::new("probe-rs")
                .args(["reset", "--chip", &chip])
                .status();
        }
        Ok(s) => {
            die(&format!("probe-rs failed with exit code {:?}", s.code()));
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                die("probe-rs not found. install with: cargo install probe-rs-tools");
            }
            die(&format!("failed to run probe-rs: {e}"));
        }
    }

    // cleanup
    let _ = std::fs::remove_file(&elf_path);
}

fn cmd_check(args: &[String]) {
    if args.len() < 3 {
        eprintln!("usage: kov check <file.kov>");
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
        Err(errors) => {
            for e in &errors {
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
