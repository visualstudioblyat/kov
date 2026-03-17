use crate::parser::ast::*;
use std::collections::HashMap;

// DMA safety via typestate: Buffer<OwnedByCpu> can be read/written,
// Buffer<DmaActive> cannot. Transfer consumes the buffer, wait() returns it.
//
// This is enforced by tracking buffer state through the program:
// - dma.start(buf) moves buf to DmaActive state
// - transfer.wait() moves it back to OwnedByCpu
// - accessing a DmaActive buffer is a compile error

pub struct DmaSafety {
    pub errors: Vec<String>,
}

#[derive(Clone, PartialEq)]
enum BufState {
    OwnedByCpu,
    DmaActive,
    Consumed, // moved into a transfer
}

impl DmaSafety {
    pub fn check(program: &Program) -> Self {
        let mut errors = Vec::new();

        for item in &program.items {
            if let TopItem::Function(f) = item {
                let mut buf_states: HashMap<String, BufState> = HashMap::new();
                check_block(&f.body, &mut buf_states, &mut errors);
            }
        }

        Self { errors }
    }
}

fn check_block(block: &Block, states: &mut HashMap<String, BufState>, errors: &mut Vec<String>) {
    for stmt in &block.stmts {
        check_stmt(stmt, states, errors);
    }
}

fn check_stmt(stmt: &Stmt, states: &mut HashMap<String, BufState>, errors: &mut Vec<String>) {
    match stmt {
        Stmt::Let { name, value, .. } => {
            // check access BEFORE state transitions (so dma.start(buf) doesn't flag buf)
            let is_start = is_dma_start(value).is_some();
            let is_wait = is_dma_wait(value).is_some();
            if !is_start && !is_wait {
                check_buffer_access(value, states, errors);
            }

            if is_buffer_create(value) {
                states.insert(name.clone(), BufState::OwnedByCpu);
            }
            if let Some(buf_name) = is_dma_start(value) {
                match states.get(&buf_name) {
                    Some(BufState::OwnedByCpu) => {
                        states.insert(buf_name, BufState::Consumed);
                        states.insert(name.clone(), BufState::DmaActive);
                    }
                    Some(BufState::DmaActive | BufState::Consumed) => {
                        errors.push("buffer already in DMA transfer, cannot start another".into());
                    }
                    None => {}
                }
            }
            if let Some(transfer_name) = is_dma_wait(value) {
                if let Some(BufState::DmaActive) = states.get(&transfer_name) {
                    states.insert(name.clone(), BufState::OwnedByCpu);
                    states.insert(transfer_name, BufState::Consumed);
                }
            }
        }
        Stmt::Expr(expr) => check_buffer_access(expr, states, errors),
        Stmt::If {
            then_block,
            else_block,
            ..
        } => {
            check_block(then_block, states, errors);
            if let Some(ElseBranch::Else(b)) = else_block {
                check_block(b, states, errors);
            }
        }
        Stmt::Loop(_, body, _) => check_block(body, states, errors),
        Stmt::While { body, .. } => check_block(body, states, errors),
        Stmt::For { body, .. } => check_block(body, states, errors),
        _ => {}
    }
}

fn check_buffer_access(expr: &Expr, states: &HashMap<String, BufState>, errors: &mut Vec<String>) {
    match expr {
        Expr::Ident(name, _) => match states.get(name) {
            Some(BufState::DmaActive) | Some(BufState::Consumed) => {
                errors.push(format!(
                    "cannot access buffer '{}' while DMA transfer is active",
                    name
                ));
            }
            _ => {}
        },
        Expr::Index(obj, idx, _) => {
            check_buffer_access(obj, states, errors);
            check_buffer_access(idx, states, errors);
        }
        Expr::Field(obj, _, _) => check_buffer_access(obj, states, errors),
        Expr::Binary(l, _, r, _) => {
            check_buffer_access(l, states, errors);
            check_buffer_access(r, states, errors);
        }
        Expr::Call(_, args, _) | Expr::MethodCall(_, _, args, _) => {
            for a in args {
                check_buffer_access(a, states, errors);
            }
        }
        _ => {}
    }
}

// detect patterns like: Buffer { ... } or buffer_create(...)
fn is_buffer_create(expr: &Expr) -> bool {
    matches!(expr, Expr::StructLit(name, _, _) if name.contains("Buf") || name.contains("buf") || name.contains("Buffer") || name.contains("buffer"))
}

// detect: dma.start(buf_name) → returns the buffer name being consumed
fn is_dma_start(expr: &Expr) -> Option<String> {
    if let Expr::MethodCall(_, method, args, _) = expr {
        if method == "start" {
            if let Some(Expr::Ident(name, _)) = args.first() {
                return Some(name.clone());
            }
        }
    }
    None
}

// detect: transfer.wait() → returns the transfer name
fn is_dma_wait(expr: &Expr) -> Option<String> {
    if let Expr::MethodCall(obj, method, _, _) = expr {
        if method == "wait" {
            if let Expr::Ident(name, _) = obj.as_ref() {
                return Some(name.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check_dma(src: &str) -> Vec<String> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        DmaSafety::check(&program).errors
    }

    #[test]
    fn dma_safe_usage() {
        let errors = check_dma(
            "struct DmaBuf { data: u32 }
             fn f() {
                let buf = DmaBuf { data: 0 };
                let transfer = dma.start(buf);
                let result = transfer.wait();
            }",
        );
        assert!(errors.is_empty(), "correct DMA usage should pass");
    }

    #[test]
    fn dma_access_during_transfer() {
        let errors = check_dma(
            "struct DmaBuf { data: u32 }
             fn f() {
                let buf = DmaBuf { data: 0 };
                let transfer = dma.start(buf);
                let x = buf + 1;
            }",
        );
        assert!(
            !errors.is_empty(),
            "accessing buffer during DMA should fail"
        );
        assert!(errors[0].contains("DMA transfer is active"));
    }

    #[test]
    fn dma_double_start() {
        let errors = check_dma(
            "struct DmaBuf { data: u32 }
             fn f() {
                let buf = DmaBuf { data: 0 };
                let t1 = dma.start(buf);
                let t2 = dma.start(buf);
            }",
        );
        assert!(
            !errors.is_empty(),
            "starting DMA twice on same buffer should fail"
        );
    }
}
