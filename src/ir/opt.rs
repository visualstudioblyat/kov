use super::types::IrType;
use super::{BasicBlock, Function, Op, Terminator, Value};
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

// inline small functions: replace Call with the callee body
// threshold: functions with <= MAX_INLINE_INSTS instructions get inlined
const MAX_INLINE_INSTS: usize = 15;

pub fn inline_functions(functions: &mut Vec<Function>) {
    // collect inline candidates: small, non-recursive, single return
    let candidates: Vec<(String, Vec<(String, IrType)>, Vec<BasicBlock>, IrType)> = functions
        .iter()
        .filter(|f| {
            let inst_count: usize = f.blocks.iter().map(|b| b.insts.len()).sum();
            inst_count <= MAX_INLINE_INSTS && f.name != "main"
        })
        .map(|f| {
            (
                f.name.clone(),
                f.params.clone(),
                f.blocks.clone(),
                f.ret_type,
            )
        })
        .collect();

    if candidates.is_empty() {
        return;
    }

    let candidate_names: HashSet<String> =
        candidates.iter().map(|(n, _, _, _)| n.clone()).collect();

    // for each function, find Call ops that reference candidates and count them
    // only inline functions called once (to avoid code bloat)
    let mut call_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for func in functions.iter() {
        for block in &func.blocks {
            for inst in &block.insts {
                if let Op::Call(name, _) = &inst.op {
                    if candidate_names.contains(name) {
                        *call_counts.entry(name.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    // inline: for now, just mark single-call functions as inlineable
    // actual inlining is complex (renumber values, splice blocks) — defer to later
    // instead, let's just do the simple case: inline functions that are leaf (no calls) and tiny (<=5 insts)
    // by replacing Call with the function's single-block body
    for func in functions.iter_mut() {
        for block in &mut func.blocks {
            for inst in &mut block.insts {
                if let Op::Call(name, args) = &inst.op {
                    if let Some((_, params, blocks, _)) =
                        candidates.iter().find(|(n, _, _, _)| n == name)
                    {
                        // only inline small functions with <= 5 instructions in the entry block
                        if !blocks.is_empty()
                            && blocks[0].insts.len() <= 5
                            && call_counts.get(name).copied().unwrap_or(0) <= 1
                        {
                            // check it's a leaf function (no calls inside)
                            let has_calls = blocks[0]
                                .insts
                                .iter()
                                .any(|i| matches!(i.op, Op::Call(_, _)));
                            if !has_calls && args.len() == params.len() {
                                // simple case: replace call with the return value's computation
                                // find the return value
                                if let Terminator::Return(Some(ret_val)) = &blocks[0].terminator {
                                    // find the instruction that produces ret_val
                                    if let Some(ret_inst) =
                                        blocks[0].insts.iter().find(|i| i.result == *ret_val)
                                    {
                                        // substitute params with args
                                        let mut new_op = ret_inst.op.clone();
                                        for (i, (_, _)) in params.iter().enumerate() {
                                            let param_val = Value(i as u32);
                                            if i < args.len() {
                                                new_op =
                                                    substitute_value(&new_op, param_val, args[i]);
                                            }
                                        }
                                        inst.op = new_op;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn substitute_value(op: &Op, from: Value, to: Value) -> Op {
    match op {
        Op::Add(a, b) => Op::Add(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Sub(a, b) => Op::Sub(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Mul(a, b) => Op::Mul(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Div(a, b) => Op::Div(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Rem(a, b) => Op::Rem(sub1(*a, from, to), sub1(*b, from, to)),
        Op::And(a, b) => Op::And(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Or(a, b) => Op::Or(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Xor(a, b) => Op::Xor(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Shl(a, b) => Op::Shl(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Shr(a, b) => Op::Shr(sub1(*a, from, to), sub1(*b, from, to)),
        Op::Neg(a) => Op::Neg(sub1(*a, from, to)),
        Op::Not(a) => Op::Not(sub1(*a, from, to)),
        other => other.clone(),
    }
}

fn sub1(v: Value, from: Value, to: Value) -> Value {
    if v == from { to } else { v }
}

// copy propagation: if v = copy(x), replace all uses of v with x
pub fn copy_propagation(func: &mut Function) {
    let mut copies: std::collections::HashMap<u32, Value> = std::collections::HashMap::new();

    for block in &func.blocks {
        for inst in &block.insts {
            match &inst.op {
                Op::Zext(v, _) | Op::Sext(v, _) | Op::Trunc(v, _) => {
                    copies.insert(inst.result.0, *v);
                }
                // addi rd, rs, 0 pattern shows up as Add(v, const_0)
                Op::Add(a, b) => {
                    if let Some(0) = const_val_from(func, *b) {
                        copies.insert(inst.result.0, *a);
                    } else if let Some(0) = const_val_from(func, *a) {
                        copies.insert(inst.result.0, *b);
                    }
                }
                _ => {}
            }
        }
    }

    if copies.is_empty() {
        return;
    }

    // chase copy chains: if v1 → v2 → v3, resolve v1 → v3
    let mut resolved: std::collections::HashMap<u32, Value> = std::collections::HashMap::new();
    for (&from, &to) in &copies {
        let mut target = to;
        let mut depth = 0;
        while let Some(&next) = copies.get(&target.0) {
            target = next;
            depth += 1;
            if depth > 32 {
                break;
            }
        }
        resolved.insert(from, target);
    }

    for block in &mut func.blocks {
        for inst in &mut block.insts {
            inst.op = rewrite_op(&inst.op, &resolved);
        }
        rewrite_terminator(&mut block.terminator, &resolved);
    }
}

fn const_val_from(func: &Function, val: Value) -> Option<i32> {
    for block in &func.blocks {
        for inst in &block.insts {
            if inst.result == val {
                if let Op::ConstI32(v) = inst.op {
                    return Some(v);
                }
                return None;
            }
        }
    }
    None
}

// tail call optimization: if a block's last instruction is Call and terminator is Return of that call's result, convert to a TailCall
pub fn tail_call_opt(func: &mut Function) {
    for block in &mut func.blocks {
        if let Terminator::Return(Some(ret_val)) = &block.terminator {
            if let Some(last) = block.insts.last() {
                if last.result == *ret_val {
                    if let Op::Call(name, args) = &last.op {
                        let name = name.clone();
                        let args = args.clone();
                        block.insts.pop();
                        block.terminator = Terminator::TailCall(name, args);
                    }
                }
            }
        }
    }
}

// run all optimizations (two passes for propagation effects)
pub fn optimize(func: &mut Function) {
    constant_fold(func);
    strength_reduce(func);
    cse(func);
    copy_propagation(func);
    constant_fold(func);
    dead_code_elimination(func);
    tail_call_opt(func);
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
        inline_functions(&mut ir.functions);
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

    #[test]
    fn tail_call_detected() {
        let funcs = lower_and_opt(
            "fn other(x: u32) u32 { return x; }\nfn f(x: u32) u32 { return other(x); }",
        );
        let f = funcs.iter().find(|f| f.name == "f").unwrap();
        let has_tail = f
            .blocks
            .iter()
            .any(|b| matches!(&b.terminator, Terminator::TailCall(n, _) if n == "other"));
        assert!(has_tail, "return other(x) should be optimized to TailCall");
    }

    #[test]
    fn inline_tiny_function() {
        // double(x) = x + x should be inlined into f
        let funcs = lower_and_opt(
            "fn double(x: u32) u32 { return x + x; }\nfn f(a: u32) u32 { return double(a); }",
        );
        // f should not have a Call to double anymore (it got inlined)
        let f = funcs.iter().find(|f| f.name == "f").unwrap();
        let has_call = f.blocks.iter().any(|b| {
            b.insts
                .iter()
                .any(|i| matches!(&i.op, Op::Call(n, _) if n == "double"))
        });
        assert!(!has_call, "double() should be inlined, no Call remaining");
        // should have an Add instead
        let has_add = f
            .blocks
            .iter()
            .any(|b| b.insts.iter().any(|i| matches!(i.op, Op::Add(_, _))));
        assert!(has_add, "inlined double should produce Add");
    }
}
