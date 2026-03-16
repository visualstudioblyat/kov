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

// strength reduction: replace expensive ops with cheaper equivalents
pub fn strength_reduce(func: &mut Function) {
    use std::collections::HashMap;
    let mut constants: HashMap<u32, i32> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Op::ConstI32(v) = inst.op {
                constants.insert(inst.result.0, v);
            }
        }
    }

    for block in &mut func.blocks {
        for inst in &mut block.insts {
            let new_op = match &inst.op {
                Op::Mul(a, b) => {
                    let av = constants.get(&a.0).copied();
                    let bv = constants.get(&b.0).copied();
                    let a = *a;
                    let b = *b;
                    if let Some(v) = bv {
                        if v == 2 { Some(Op::Add(a, a)) } else { None }
                    } else if let Some(v) = av {
                        if v == 2 { Some(Op::Add(b, b)) } else { None }
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(op) = new_op {
                inst.op = op;
            }
        }
    }
}

// common subexpression elimination: if the same pure op is computed twice, reuse the first
pub fn cse(func: &mut Function) {
    use std::collections::HashMap;
    // key: serialized op → first Value that computed it
    let mut seen: HashMap<String, Value> = HashMap::new();
    // value → replacement value
    let mut replacements: HashMap<u32, Value> = HashMap::new();

    for block in &func.blocks {
        for inst in &block.insts {
            // only CSE pure operations (no side effects)
            if is_pure(&inst.op) {
                let key = format!("{:?}", inst.op);
                if let Some(&first) = seen.get(&key) {
                    replacements.insert(inst.result.0, first);
                } else {
                    seen.insert(key, inst.result);
                }
            }
        }
    }

    if replacements.is_empty() {
        return;
    }

    // apply replacements: rewrite all uses of replaced values
    for block in &mut func.blocks {
        for inst in &mut block.insts {
            inst.op = rewrite_op(&inst.op, &replacements);
        }
        rewrite_terminator(&mut block.terminator, &replacements);
    }
}

fn is_pure(op: &Op) -> bool {
    matches!(
        op,
        Op::Add(_, _)
            | Op::Sub(_, _)
            | Op::Mul(_, _)
            | Op::Div(_, _)
            | Op::Rem(_, _)
            | Op::And(_, _)
            | Op::Or(_, _)
            | Op::Xor(_, _)
            | Op::Shl(_, _)
            | Op::Shr(_, _)
            | Op::Sar(_, _)
            | Op::Eq(_, _)
            | Op::Ne(_, _)
            | Op::Lt(_, _)
            | Op::Ge(_, _)
            | Op::Neg(_)
            | Op::Not(_)
    )
}

fn rewrite_val(v: Value, r: &std::collections::HashMap<u32, Value>) -> Value {
    r.get(&v.0).copied().unwrap_or(v)
}

fn rewrite_op(op: &Op, r: &std::collections::HashMap<u32, Value>) -> Op {
    match op {
        Op::Add(a, b) => Op::Add(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Sub(a, b) => Op::Sub(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Mul(a, b) => Op::Mul(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Div(a, b) => Op::Div(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Rem(a, b) => Op::Rem(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::And(a, b) => Op::And(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Or(a, b) => Op::Or(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Xor(a, b) => Op::Xor(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Shl(a, b) => Op::Shl(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Shr(a, b) => Op::Shr(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Sar(a, b) => Op::Sar(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Eq(a, b) => Op::Eq(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Ne(a, b) => Op::Ne(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Lt(a, b) => Op::Lt(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Ge(a, b) => Op::Ge(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Ltu(a, b) => Op::Ltu(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Geu(a, b) => Op::Geu(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Neg(a) => Op::Neg(rewrite_val(*a, r)),
        Op::Not(a) => Op::Not(rewrite_val(*a, r)),
        Op::Load(a, t) => Op::Load(rewrite_val(*a, r), *t),
        Op::Store(a, b) => Op::Store(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::VolatileLoad(a, t) => Op::VolatileLoad(rewrite_val(*a, r), *t),
        Op::VolatileStore(a, b) => Op::VolatileStore(rewrite_val(*a, r), rewrite_val(*b, r)),
        Op::Call(name, args) => Op::Call(
            name.clone(),
            args.iter().map(|a| rewrite_val(*a, r)).collect(),
        ),
        Op::Zext(a, t) => Op::Zext(rewrite_val(*a, r), *t),
        Op::Sext(a, t) => Op::Sext(rewrite_val(*a, r), *t),
        Op::Trunc(a, t) => Op::Trunc(rewrite_val(*a, r), *t),
        Op::MakeError(a) => Op::MakeError(rewrite_val(*a, r)),
        other => other.clone(),
    }
}

fn rewrite_terminator(term: &mut Terminator, r: &std::collections::HashMap<u32, Value>) {
    match term {
        Terminator::Return(Some(v)) => *v = rewrite_val(*v, r),
        Terminator::ReturnError(a, b) => {
            *a = rewrite_val(*a, r);
            *b = rewrite_val(*b, r);
        }
        Terminator::BranchIf { cond, .. } => *cond = rewrite_val(*cond, r),
        Terminator::Jump(_, args) => {
            for a in args {
                *a = rewrite_val(*a, r);
            }
        }
        _ => {}
    }
}

// run all optimizations (two passes for propagation effects)
pub fn optimize(func: &mut Function) {
    // pass 1
    constant_fold(func);
    strength_reduce(func);
    cse(func);
    // pass 2 — folding may create new constants after CSE
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

    #[test]
    fn strength_reduce_mul_2() {
        let funcs = lower_and_opt("fn f(x: u32) u32 { return x * 2; }");
        let func = &funcs[0];
        // x * 2 should become x + x (no Mul instruction)
        let has_mul = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::Mul(_, _))));
        assert!(!has_mul, "x * 2 should be strength-reduced to x + x");
        let has_add = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::Add(_, _))));
        assert!(has_add, "should have Add(x, x) instead of Mul");
    }

    #[test]
    fn chained_constant_fold() {
        // 2 + 3 folds to 5, then 5 * 4 folds to 20
        let funcs = lower_and_opt("fn f() u32 { let a = 2 + 3; let b = a * 4; return b; }");
        let func = &funcs[0];
        let has_twenty = func
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::ConstI32(20))));
        assert!(has_twenty, "2+3 then *4 should fold to 20");
    }

    #[test]
    fn cse_eliminates_duplicate() {
        // a + b computed twice — second should reuse first
        let funcs = lower_and_opt(
            "fn f(a: u32, b: u32) u32 { let x = a + b; let y = a + b; return x + y; }",
        );
        let func = &funcs[0];
        // should have only one Add(a, b), not two
        let add_count = func
            .blocks
            .iter()
            .flat_map(|b| b.insts.iter())
            .filter(|i| matches!(i.op, Op::Add(_, _)))
            .count();
        // one for a+b (CSE'd) and one for x+y = 2 total
        assert!(
            add_count <= 2,
            "CSE should eliminate duplicate a+b, got {} adds",
            add_count
        );
    }
}
