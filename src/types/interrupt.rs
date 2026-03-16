use crate::parser::ast::*;
use std::collections::{HashMap, HashSet};

pub struct InterruptSafety {
    pub errors: Vec<String>,
}

struct Context {
    // globals accessed by main-context functions
    main_globals: HashSet<String>,
    // globals accessed by interrupt handlers
    isr_globals: HashSet<String>,
    // which functions are ISRs
    isr_fns: HashSet<String>,
    // function → set of globals it touches
    fn_globals: HashMap<String, HashSet<String>>,
}

impl InterruptSafety {
    pub fn check(program: &Program) -> Self {
        let mut ctx = Context {
            main_globals: HashSet::new(),
            isr_globals: HashSet::new(),
            isr_fns: HashSet::new(),
            fn_globals: HashMap::new(),
        };

        // collect ISR function names
        for item in &program.items {
            if let TopItem::Interrupt(i) = item {
                ctx.isr_fns.insert(i.fn_name.clone());
            }
        }

        // collect globals accessed by each function
        for item in &program.items {
            match item {
                TopItem::Function(f) => {
                    let mut globals = HashSet::new();
                    collect_globals_in_block(&f.body, &mut globals);
                    ctx.fn_globals.insert(f.name.clone(), globals);
                }
                TopItem::Interrupt(i) => {
                    let mut globals = HashSet::new();
                    collect_globals_in_block(&i.body, &mut globals);
                    ctx.fn_globals.insert(i.fn_name.clone(), globals);
                }
                _ => {}
            }
        }

        // collect all global/static names
        let all_globals: HashSet<String> = program
            .items
            .iter()
            .filter_map(|item| {
                if let TopItem::Static(s) = item {
                    Some(s.name.clone())
                } else {
                    None
                }
            })
            .collect();

        // classify: which globals are touched by main vs ISR context
        for (fn_name, globals) in &ctx.fn_globals {
            for g in globals {
                if !all_globals.contains(g) {
                    continue;
                }
                if ctx.isr_fns.contains(fn_name) {
                    ctx.isr_globals.insert(g.clone());
                } else {
                    ctx.main_globals.insert(g.clone());
                }
            }
        }

        // find shared globals (accessed from both contexts)
        let shared: Vec<String> = ctx
            .main_globals
            .intersection(&ctx.isr_globals)
            .cloned()
            .collect();

        let mut errors = Vec::new();
        for name in &shared {
            errors.push(format!(
                "global '{}' accessed from both main and interrupt context without synchronization",
                name
            ));
        }

        Self { errors }
    }
}

fn collect_globals_in_block(block: &Block, globals: &mut HashSet<String>) {
    for stmt in &block.stmts {
        collect_globals_in_stmt(stmt, globals);
    }
}

fn collect_globals_in_stmt(stmt: &Stmt, globals: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { value, .. } => collect_globals_in_expr(value, globals),
        Stmt::Assign { target, value, .. } => {
            collect_globals_in_expr(target, globals);
            collect_globals_in_expr(value, globals);
        }
        Stmt::Expr(expr) => collect_globals_in_expr(expr, globals),
        Stmt::Return(Some(expr), _) => collect_globals_in_expr(expr, globals),
        Stmt::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            collect_globals_in_expr(condition, globals);
            collect_globals_in_block(then_block, globals);
            if let Some(eb) = else_block {
                match eb {
                    ElseBranch::Else(b) => collect_globals_in_block(b, globals),
                    ElseBranch::ElseIf(s) => collect_globals_in_stmt(s, globals),
                }
            }
        }
        Stmt::Loop(_, body, _) => collect_globals_in_block(body, globals),
        Stmt::While { body, .. } => collect_globals_in_block(body, globals),
        Stmt::For { body, .. } => collect_globals_in_block(body, globals),
        Stmt::Match { expr, arms, .. } => {
            collect_globals_in_expr(expr, globals);
            for arm in arms {
                collect_globals_in_expr(&arm.body, globals);
            }
        }
        _ => {}
    }
}

fn collect_globals_in_expr(expr: &Expr, globals: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            globals.insert(name.clone());
        }
        Expr::Binary(l, _, r, _) => {
            collect_globals_in_expr(l, globals);
            collect_globals_in_expr(r, globals);
        }
        Expr::Unary(_, inner, _) => collect_globals_in_expr(inner, globals),
        Expr::Call(callee, args, _) => {
            collect_globals_in_expr(callee, globals);
            for a in args {
                collect_globals_in_expr(a, globals);
            }
        }
        Expr::MethodCall(obj, _, args, _) => {
            collect_globals_in_expr(obj, globals);
            for a in args {
                collect_globals_in_expr(a, globals);
            }
        }
        Expr::Field(obj, _, _) => collect_globals_in_expr(obj, globals),
        Expr::Index(obj, idx, _) => {
            collect_globals_in_expr(obj, globals);
            collect_globals_in_expr(idx, globals);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_safety(src: &str) -> Vec<String> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        InterruptSafety::check(&program).errors
    }

    #[test]
    fn no_shared_globals_ok() {
        let errors = check_safety(
            "static mut main_var: u32 = 0;
             static mut isr_var: u32 = 0;
             fn main_fn() { main_var = 1; }
             interrupt(timer0, priority = 1) fn on_tick() { isr_var = isr_var + 1; }",
        );
        assert!(errors.is_empty(), "separate globals should be fine");
    }

    #[test]
    fn shared_global_detected() {
        let errors = check_safety(
            "static mut counter: u32 = 0;
             fn main_fn() { counter = counter + 1; }
             interrupt(timer0, priority = 1) fn on_tick() { counter = counter + 1; }",
        );
        assert!(
            !errors.is_empty(),
            "shared global without sync should be flagged"
        );
        assert!(errors[0].contains("counter"));
    }

    #[test]
    fn isr_only_global_ok() {
        let errors = check_safety(
            "static mut ticks: u32 = 0;
             interrupt(timer0, priority = 1) fn on_tick() { ticks = ticks + 1; }",
        );
        assert!(errors.is_empty(), "ISR-only global should be fine");
    }
}
