use crate::codegen;
use crate::ir;
use crate::lexer;
use crate::parser;
use crate::types;

pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub cycles: u64,
    pub message: Option<String>,
}

pub fn run_tests(source: &str) -> Vec<TestResult> {
    let tokens = match lexer::Lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            return vec![TestResult {
                name: "(lex)".into(),
                passed: false,
                cycles: 0,
                message: Some(format!("{e}")),
            }];
        }
    };

    let program = match parser::Parser::new(tokens).parse() {
        Ok(p) => p,
        Err(errors) => {
            return vec![TestResult {
                name: "(parse)".into(),
                passed: false,
                cycles: 0,
                message: Some(
                    errors
                        .iter()
                        .map(|e| e.message.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            }];
        }
    };

    // find all functions with #[test] attribute
    let test_fns: Vec<&parser::ast::FnDef> = program
        .items
        .iter()
        .filter_map(|item| {
            if let parser::ast::TopItem::Function(f) = item {
                if f.attrs.iter().any(|a| a.name == "test") {
                    return Some(f);
                }
            }
            None
        })
        .collect();

    if test_fns.is_empty() {
        return vec![];
    }

    let mut results = Vec::new();

    for test_fn in &test_fns {
        // compile and run each test function individually
        // wrap it so main() calls the test function
        let wrapper = format!("{}\nfn main() {{ {}(); }}", source, test_fn.name);

        let result = match compile_and_run(&wrapper, 100_000) {
            Ok((cycles, halted)) => TestResult {
                name: test_fn.name.clone(),
                passed: halted, // test passes if it halts (reaches end of main)
                cycles,
                message: if halted {
                    None
                } else {
                    Some("did not halt (infinite loop or timeout)".into())
                },
            },
            Err(e) => TestResult {
                name: test_fn.name.clone(),
                passed: false,
                cycles: 0,
                message: Some(e),
            },
        };
        results.push(result);
    }

    results
}

fn compile_and_run(source: &str, max_cycles: u64) -> Result<(u64, bool), String> {
    let tokens = lexer::Lexer::tokenize(source).map_err(|e| format!("{e}"))?;
    let mut program = parser::Parser::new(tokens).parse().map_err(|errors| {
        errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", ")
    })?;

    parser::monomorph::monomorphize(&mut program);

    if let Err(errors) = types::check::TypeChecker::new().check(&program) {
        return Err(errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }

    let board_name = program.items.iter().find_map(|item| {
        if let parser::ast::TopItem::Board(b) = item {
            Some(b.name.clone())
        } else {
            None
        }
    });

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

    if let Some(ref board) = board_config {
        codegen::startup::emit_startup(&mut cg.emitter, board);
        let clock_hz = 160_000_000u32;
        codegen::builtins::emit_builtins(&mut cg.emitter, clock_hz);
    }

    for func in &ir_result.functions {
        cg.gen_function(func);
    }

    let code = cg.finish()?;

    let flash_base = board_config
        .as_ref()
        .map(|b| b.flash_start)
        .unwrap_or(0x0800_0000);
    let ram_top = board_config
        .as_ref()
        .map(|b| b.stack_top())
        .unwrap_or(0x2000_8000);

    let mut cpu = crate::emu::Cpu::with_memory(flash_base, flash_base, ram_base);
    cpu.mem.load_flash(&code);
    cpu.regs[2] = ram_top;
    cpu.run(max_cycles);

    Ok((cpu.cycles, cpu.halted))
}

pub fn format_results(results: &[TestResult]) -> String {
    let mut out = String::new();
    let mut passed = 0;
    let mut failed = 0;

    for r in results {
        if r.passed {
            out.push_str(&format!("  ok   {} ({} cycles)\n", r.name, r.cycles));
            passed += 1;
        } else {
            out.push_str(&format!(
                "  FAIL {} — {}\n",
                r.name,
                r.message.as_deref().unwrap_or("unknown")
            ));
            failed += 1;
        }
    }

    out.push_str(&format!("\n{} passed, {} failed\n", passed, failed));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_framework_finds_tests() {
        let results = run_tests(
            "fn not_a_test() { }
             #[test] fn my_test() { let x = 1 + 1; }",
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "my_test");
    }

    #[test]
    fn test_passing_test() {
        let results = run_tests("#[test] fn simple() { let x = 42; }");
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "simple test should pass");
    }

    #[test]
    fn test_no_tests() {
        let results = run_tests("fn f() { }");
        assert!(results.is_empty());
    }
}
