#![allow(dead_code)]
#![allow(
    clippy::enum_variant_names,
    clippy::collapsible_if,
    clippy::type_complexity,
    clippy::ptr_arg
)]

pub mod codegen;
pub mod emu;
pub mod errors;
pub mod ir;
pub mod lexer;
pub mod parser;
pub mod types;

use std::collections::HashMap;

pub struct CompileOutput {
    pub code: Vec<u8>,
    pub compressed: Vec<u8>,
    pub labels: HashMap<String, usize>,
    pub flash_base: u32,
    pub ram_base: u32,
    pub ram_top: u32,
    pub compile_ms: f64,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Diagnostic {
    pub severity: String,
    pub message: String,
    pub line: usize,
    pub column: usize,
}

pub struct RunOutput {
    pub cycles: u64,
    pub halted: bool,
    pub mmio_writes: Vec<(u32, u32)>,
    pub regs: [u32; 32],
}

/// compile kov source to RISC-V machine code
pub fn compile(source: &str) -> Result<CompileOutput, Vec<Diagnostic>> {
    #[cfg(not(target_arch = "wasm32"))]
    let start = std::time::Instant::now();
    #[cfg(target_arch = "wasm32")]
    let start_ms = 0.0f64;

    let tokens = lexer::Lexer::tokenize(source).map_err(|e| {
        vec![Diagnostic {
            severity: "error".into(),
            message: format!("{e}"),
            line: 0,
            column: 0,
        }]
    })?;

    let program = parser::Parser::new(tokens).parse().map_err(|errors| {
        errors
            .iter()
            .map(|e| {
                let (line, col, _) = errors::locate(source, e.span.start);
                Diagnostic {
                    severity: "error".into(),
                    message: e.message.clone(),
                    line,
                    column: col,
                }
            })
            .collect::<Vec<Diagnostic>>()
    })?;

    let mut diagnostics = Vec::new();

    match types::check::TypeChecker::new().check(&program) {
        Ok(warnings) => {
            for w in &warnings {
                let (line, col, _) = errors::locate(source, w.span.start);
                diagnostics.push(Diagnostic {
                    severity: "warning".into(),
                    message: w.message.clone(),
                    line,
                    column: col,
                });
            }
        }
        Err(errs) => {
            return Err(errs
                .iter()
                .map(|e| {
                    let (line, col, _) = errors::locate(source, e.span.start);
                    Diagnostic {
                        severity: "error".into(),
                        message: e.message.clone(),
                        line,
                        column: col,
                    }
                })
                .collect());
        }
    }

    // interrupt safety
    let isr = types::interrupt::InterruptSafety::check(&program);
    for e in &isr.errors {
        diagnostics.push(Diagnostic {
            severity: "warning".into(),
            message: e.clone(),
            line: 0,
            column: 0,
        });
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
    ir::opt::inline_functions(&mut ir_result.functions);
    for func in &mut ir_result.functions {
        ir::opt::optimize(func);
    }

    let ram_base = board_config
        .as_ref()
        .map(|b| b.ram_start)
        .unwrap_or(0x2000_0000);
    let mut cg = codegen::CodeGen::new_with_globals(ram_base, &ir_result.globals);

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

    let labels = cg.emitter.labels.clone();
    let code = cg.finish().map_err(|e| {
        vec![Diagnostic {
            severity: "error".into(),
            message: e,
            line: 0,
            column: 0,
        }]
    })?;
    let compressed = codegen::compress::compress(&code);

    Ok(CompileOutput {
        code,
        compressed,
        labels,
        flash_base: board_config
            .as_ref()
            .map(|b| b.flash_start)
            .unwrap_or(0x0800_0000),
        ram_base,
        ram_top: board_config
            .as_ref()
            .map(|b| b.stack_top())
            .unwrap_or(0x2000_8000),
        compile_ms: {
            #[cfg(not(target_arch = "wasm32"))]
            {
                start.elapsed().as_secs_f64() * 1000.0
            }
            #[cfg(target_arch = "wasm32")]
            {
                start_ms
            }
        },
        diagnostics,
    })
}

/// compile and run in the emulator
pub fn run(source: &str, max_cycles: u64) -> Result<RunOutput, Vec<Diagnostic>> {
    let compiled = compile(source)?;
    let mut cpu =
        emu::Cpu::with_memory(compiled.flash_base, compiled.flash_base, compiled.ram_base);
    cpu.mem.load_flash(&compiled.code);
    cpu.regs[2] = compiled.ram_top;
    cpu.run(max_cycles);

    let mmio_writes: Vec<(u32, u32)> = cpu
        .mem
        .mmio_log
        .iter()
        .filter(|a| a.is_write)
        .map(|a| (a.address, a.value))
        .collect();

    Ok(RunOutput {
        cycles: cpu.cycles,
        halted: cpu.halted,
        mmio_writes,
        regs: cpu.regs,
    })
}

/// disassemble compiled output
pub fn disassemble(output: &CompileOutput) -> String {
    codegen::disasm::disassemble(&output.code, output.flash_base, &output.labels)
}

// WASM bindings — only compiled when wasm-bindgen feature is enabled
#[cfg(feature = "wasm-bindgen")]
mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn wasm_compile(source: &str) -> String {
        match super::compile(source) {
            Ok(output) => {
                let diags: Vec<String> = output
                    .diagnostics
                    .iter()
                    .map(|d| format!("{}:{}:{}: {}", d.severity, d.line, d.column, d.message))
                    .collect();
                format!(
                    r#"{{"ok":true,"code_size":{},"compressed_size":{},"compile_ms":{:.2},"diagnostics":[{}]}}"#,
                    output.code.len(),
                    output.compressed.len(),
                    output.compile_ms,
                    diags
                        .iter()
                        .map(|d| format!("\"{}\"", d.replace('"', "\\\"")))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            }
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|d| {
                        format!(
                            "\"{}:{}:{}: {}\"",
                            d.severity,
                            d.line,
                            d.column,
                            d.message.replace('"', "\\\"")
                        )
                    })
                    .collect();
                format!(r#"{{"ok":false,"errors":[{}]}}"#, msgs.join(","))
            }
        }
    }

    #[wasm_bindgen]
    pub fn wasm_run(source: &str, max_cycles: u32) -> String {
        match super::run(source, max_cycles as u64) {
            Ok(output) => {
                let writes: Vec<String> = output
                    .mmio_writes
                    .iter()
                    .take(100)
                    .map(|(a, v)| format!("[{},{}]", a, v))
                    .collect();
                format!(
                    r#"{{"ok":true,"cycles":{},"halted":{},"mmio_writes":[{}]}}"#,
                    output.cycles,
                    output.halted,
                    writes.join(","),
                )
            }
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|d| format!("\"{}\"", d.message.replace('"', "\\\"")))
                    .collect();
                format!(r#"{{"ok":false,"errors":[{}]}}"#, msgs.join(","))
            }
        }
    }

    #[wasm_bindgen]
    pub fn wasm_disassemble(source: &str) -> String {
        match super::compile(source) {
            Ok(output) => super::disassemble(&output),
            Err(errors) => errors
                .iter()
                .map(|d| format!("error: {}", d.message))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}
