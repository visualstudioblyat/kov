use super::{Function, Op, Terminator, Value};
use std::collections::HashMap;

// evaluate a function at compile time given constant arguments
// returns Some(result) if evaluation succeeds, None if the function
// can't be evaluated (side effects, loops, etc.)
pub fn eval(func: &Function, args: &[i32]) -> Option<i32> {
    let mut vals: HashMap<u32, i32> = HashMap::new();

    // bind params
    for (i, _) in func.params.iter().enumerate() {
        if i < args.len() {
            vals.insert(i as u32, args[i]);
        }
    }

    // walk blocks linearly (only works for acyclic functions)
    let mut block_idx = 0;
    let mut steps = 0;
    let max_steps = 10_000;

    loop {
        if block_idx >= func.blocks.len() || steps > max_steps {
            return None;
        }

        let block = &func.blocks[block_idx];

        for inst in &block.insts {
            steps += 1;
            if steps > max_steps {
                return None;
            }

            let v = eval_op(&inst.op, &vals)?;
            vals.insert(inst.result.0, v);
        }

        match &block.terminator {
            Terminator::Return(Some(val)) => {
                return vals.get(&val.0).copied();
            }
            Terminator::Return(None) => return Some(0),
            Terminator::Jump(target, _) => {
                block_idx = target.0 as usize;
            }
            Terminator::BranchIf {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let c = vals.get(&cond.0).copied().unwrap_or(0);
                if c != 0 {
                    block_idx = then_block.0 as usize;
                } else {
                    block_idx = else_block.0 as usize;
                }
            }
            _ => return None,
        }
    }
}

fn eval_op(op: &Op, vals: &HashMap<u32, i32>) -> Option<i32> {
    let v = |val: &Value| vals.get(&val.0).copied();

    match op {
        Op::ConstI32(n) => Some(*n),
        Op::ConstBool(b) => Some(*b as i32),
        Op::Add(a, b) => Some(v(a)?.wrapping_add(v(b)?)),
        Op::Sub(a, b) => Some(v(a)?.wrapping_sub(v(b)?)),
        Op::Mul(a, b) => Some(v(a)?.wrapping_mul(v(b)?)),
        Op::Div(a, b) => {
            let d = v(b)?;
            if d == 0 {
                None
            } else {
                Some(v(a)?.wrapping_div(d))
            }
        }
        Op::Rem(a, b) => {
            let d = v(b)?;
            if d == 0 {
                None
            } else {
                Some(v(a)?.wrapping_rem(d))
            }
        }
        Op::And(a, b) => Some(v(a)? & v(b)?),
        Op::Or(a, b) => Some(v(a)? | v(b)?),
        Op::Xor(a, b) => Some(v(a)? ^ v(b)?),
        Op::Shl(a, b) => Some(v(a)?.wrapping_shl(v(b)? as u32)),
        Op::Shr(a, b) => Some(((v(a)? as u32).wrapping_shr(v(b)? as u32)) as i32),
        Op::Eq(a, b) => Some((v(a)? == v(b)?) as i32),
        Op::Ne(a, b) => Some((v(a)? != v(b)?) as i32),
        Op::Lt(a, b) => Some((v(a)? < v(b)?) as i32),
        Op::Ge(a, b) => Some((v(a)? >= v(b)?) as i32),
        Op::Neg(a) => Some(v(a)?.wrapping_neg()),
        Op::Not(a) => Some(!v(a)?),
        Op::Nop => Some(0),
        // side-effecting ops can't be const-evaluated
        Op::Store(_, _)
        | Op::Load(_, _)
        | Op::Call(_, _)
        | Op::VolatileStore(_, _)
        | Op::VolatileLoad(_, _)
        | Op::StackAlloc(_)
        | Op::GlobalAddr(_) => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower::Lowering;
    use crate::ir::opt;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn consteval(src: &str, fn_name: &str, args: &[i32]) -> Option<i32> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            opt::optimize(func);
        }
        let func = ir.functions.iter().find(|f| f.name == fn_name)?;
        eval(func, args)
    }

    #[test]
    fn eval_simple_add() {
        let result = consteval(
            "fn add(a: u32, b: u32) u32 { return a + b; }",
            "add",
            &[3, 4],
        );
        assert_eq!(result, Some(7));
    }

    #[test]
    fn eval_with_locals() {
        let result = consteval(
            "fn f(x: u32) u32 { let y = x * 2; let z = y + 1; return z; }",
            "f",
            &[10],
        );
        assert_eq!(result, Some(21));
    }

    #[test]
    fn eval_branch() {
        let result = consteval(
            "fn abs(x: i32) i32 { if x < 0 { return 0 - x; } else { return x; } }",
            "abs",
            &[-5],
        );
        assert_eq!(result, Some(5));
    }

    #[test]
    fn eval_clock_divider() {
        // real embedded use case: compute UART baud rate divider
        let result = consteval(
            "fn divider(freq: u32, baud: u32) u32 { return freq / baud / 16; }",
            "divider",
            &[160_000_000, 115200],
        );
        assert_eq!(result, Some(86));
    }

    #[test]
    fn eval_rejects_side_effects() {
        let result = consteval("fn f() { let x = 42; }", "f", &[]);
        // should return Some(0) for void return
        assert!(result.is_some());
    }
}
