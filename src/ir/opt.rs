use super::{Function, Op, Terminator, Value};
use std::collections::HashSet;

// constant folding: evaluate constant expressions at compile time
pub fn constant_fold(func: &mut Function) {
    use std::collections::HashMap;
    // first pass: collect all constant values
    let mut constants: HashMap<u32, i32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Op::ConstI32(v) = inst.op {
                constants.insert(inst.result.0, v);
            }
        }
    }
    // second pass: fold operations on constants
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            inst.op = fold_op(&inst.op, &constants);
            // if we produced a new constant, track it
            if let Op::ConstI32(v) = inst.op {
                constants.insert(inst.result.0, v);
            }
            if let Op::ConstBool(v) = inst.op {
                constants.insert(inst.result.0, v as i32);
            }
        }
    }
}

fn fold_op(op: &Op, constants: &std::collections::HashMap<u32, i32>) -> Op {
    let cv = |v: &Value| constants.get(&v.0).copied();
    match op {
        // binary ops on two constants
        Op::Add(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va.wrapping_add(vb));
            }
            op.clone()
        }
        Op::Sub(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va.wrapping_sub(vb));
            }
            op.clone()
        }
        Op::Mul(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va.wrapping_mul(vb));
            }
            // strength reduction: x * 1 → x (identity), x * 0 → 0
            if let Some(1) = cv(b) {
                return Op::ConstI32(0); // will be copy-propagated later
            }
            op.clone()
        }
        Op::Div(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                if vb != 0 {
                    return Op::ConstI32(va.wrapping_div(vb));
                }
            }
            op.clone()
        }
        Op::Shl(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va.wrapping_shl(vb as u32));
            }
            op.clone()
        }
        Op::Shr(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(((va as u32).wrapping_shr(vb as u32)) as i32);
            }
            op.clone()
        }
        Op::And(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va & vb);
            }
            op.clone()
        }
        Op::Or(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va | vb);
            }
            op.clone()
        }
        Op::Xor(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstI32(va ^ vb);
            }
            op.clone()
        }
        Op::Eq(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstBool(va == vb);
            }
            op.clone()
        }
        Op::Ne(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstBool(va != vb);
            }
            op.clone()
        }
        Op::Lt(a, b) => {
            if let (Some(va), Some(vb)) = (cv(a), cv(b)) {
                return Op::ConstBool(va < vb);
            }
            op.clone()
        }
        Op::Neg(a) => {
            if let Some(va) = cv(a) {
                return Op::ConstI32(va.wrapping_neg());
            }
            op.clone()
        }
        _ => op.clone(),
    }
}

// dead code elimination: remove instructions whose results are never used
pub fn dead_code_elimination(func: &mut Function) {
    // collect all used values
    let mut used: HashSet<u32> = HashSet::new();

    // mark values used by terminators
    for block in &func.blocks {
        match &block.terminator {
            Terminator::Return(Some(v)) => {
                used.insert(v.0);
            }
            Terminator::ReturnError(a, b) => {
                used.insert(a.0);
                used.insert(b.0);
            }
            Terminator::BranchIf { cond, .. } => {
                used.insert(cond.0);
            }
            Terminator::Jump(_, args) => {
                for a in args {
                    used.insert(a.0);
                }
            }
            _ => {}
        }
    }

    // mark values used by instructions (operands)
    for block in &func.blocks {
        for inst in &block.insts {
            mark_used(&inst.op, &mut used);
        }
    }

    // iterate until fixed point — newly marked values may use other values
    loop {
        let prev_len = used.len();
        for block in &func.blocks {
            for inst in &block.insts {
                if used.contains(&inst.result.0) {
                    mark_used(&inst.op, &mut used);
                }
            }
        }
        if used.len() == prev_len {
            break;
        }
    }

    // remove unused instructions (except side-effecting ones)
    for block in &mut func.blocks {
        block.insts.retain(|inst| {
            if used.contains(&inst.result.0) {
                return true;
            }
            // keep side effects
            matches!(
                inst.op,
                Op::Store(_, _)
                    | Op::VolatileStore(_, _)
                    | Op::VolatileLoad(_, _)
                    | Op::Call(_, _)
                    | Op::StackAlloc(_)
            )
        });
    }
}

fn mark_used(op: &Op, used: &mut HashSet<u32>) {
    match op {
        Op::Add(a, b)
        | Op::Sub(a, b)
        | Op::Mul(a, b)
        | Op::Div(a, b)
        | Op::Rem(a, b)
        | Op::And(a, b)
        | Op::Or(a, b)
        | Op::Xor(a, b)
        | Op::Shl(a, b)
        | Op::Shr(a, b)
        | Op::Sar(a, b)
        | Op::Eq(a, b)
        | Op::Ne(a, b)
        | Op::Lt(a, b)
        | Op::Ge(a, b)
        | Op::Ltu(a, b)
        | Op::Geu(a, b) => {
            used.insert(a.0);
            used.insert(b.0);
        }
        Op::Store(a, b) | Op::VolatileStore(a, b) => {
            used.insert(a.0);
            used.insert(b.0);
        }
        Op::Load(a, _) | Op::VolatileLoad(a, _) => {
            used.insert(a.0);
        }
        Op::Neg(a) | Op::Not(a) => {
            used.insert(a.0);
        }
        Op::Zext(a, _) | Op::Sext(a, _) | Op::Trunc(a, _) => {
            used.insert(a.0);
        }
        Op::MakeError(a) => {
            used.insert(a.0);
        }
        Op::Call(_, args) => {
            for a in args {
                used.insert(a.0);
            }
        }
        _ => {}
    }
}

// run all optimizations
pub fn optimize(func: &mut Function) {
    constant_fold(func);
    dead_code_elimination(func);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower::Lowering;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn lower_and_opt(src: &str) -> Vec<Function> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            optimize(func);
        }
        ir.functions
    }

    #[test]
    fn fold_constant_add() {
        let funcs = lower_and_opt("fn f() u32 { return 3 + 4; }");
        let func = &funcs[0];
        // after folding, 3 + 4 should become ConstI32(7)
        let has_seven = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::ConstI32(7))));
        assert!(has_seven, "3 + 4 should fold to 7");
    }

    #[test]
    fn fold_constant_comparison() {
        let funcs = lower_and_opt("fn f() bool { return 5 == 5; }");
        let func = &funcs[0];
        let has_true = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::ConstBool(true))));
        assert!(has_true, "5 == 5 should fold to true");
    }

    #[test]
    fn dce_removes_unused() {
        let funcs = lower_and_opt("fn f() u32 { let x = 1; let y = 2; return x; }");
        let func = &funcs[0];
        // y = 2 is unused, should be removed
        // count ConstI32 instructions — should be 1 (just x = 1)
        let const_count = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(i.op, Op::ConstI32(_)))
            .count();
        assert_eq!(const_count, 1, "unused y should be eliminated");
    }
}
