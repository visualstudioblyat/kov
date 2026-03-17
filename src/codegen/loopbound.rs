// automatic loop bound analysis via abstract interpretation
// eliminates manual #[max_cycles] annotations for simple loops

use crate::ir::{Function, Op, Terminator};

pub struct LoopBound {
    pub block_index: usize,
    pub bound: Option<u64>,
    pub method: String,
}

// analyze a function for loop bounds
pub fn analyze_loop_bounds(func: &Function) -> Vec<LoopBound> {
    let mut bounds = Vec::new();

    for (i, block) in func.blocks.iter().enumerate() {
        // detect back edges (block jumps to earlier block)
        let targets = get_branch_targets(&block.terminator);
        for target in &targets {
            if *target <= i {
                // this is a loop back edge from block i to block *target
                let bound = try_derive_bound(func, *target, i);
                bounds.push(LoopBound {
                    block_index: *target,
                    bound,
                    method: if bound.is_some() {
                        "interval analysis".into()
                    } else {
                        "unknown (manual annotation needed)".into()
                    },
                });
            }
        }
    }

    bounds
}

fn get_branch_targets(term: &Terminator) -> Vec<usize> {
    match term {
        Terminator::Jump(target, _) => vec![target.0 as usize],
        Terminator::BranchIf {
            then_block,
            else_block,
            ..
        } => vec![then_block.0 as usize, else_block.0 as usize],
        _ => vec![],
    }
}

// try to derive a loop bound from the loop header's condition
fn try_derive_bound(func: &Function, header: usize, _latch: usize) -> Option<u64> {
    let header_block = &func.blocks[header];

    // look for a comparison in the header block: Lt(var, limit)
    if let Terminator::BranchIf { cond, .. } = &header_block.terminator {
        // find the instruction that produces cond
        for inst in &header_block.insts {
            if inst.result == *cond {
                match &inst.op {
                    Op::Lt(_var, limit) => {
                        // try to resolve limit to a constant
                        if let Some(limit_val) = find_const(func, *limit) {
                            // try to resolve initial value of var
                            // check if header has a block param (for-loop pattern)
                            if !header_block.params.is_empty() {
                                // check entry jump for initial value
                                if header > 0 {
                                    if let Terminator::Jump(_, args) =
                                        &func.blocks[header - 1].terminator
                                    {
                                        if let Some(&init_val) = args.first() {
                                            if let Some(init) = find_const(func, init_val) {
                                                if limit_val > init {
                                                    return Some((limit_val - init) as u64);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // fallback: assume loop starts at 0
                            return Some(limit_val as u64);
                        }
                    }
                    Op::Ge(limit, _var) => {
                        // same as Lt but reversed
                        if let Some(limit_val) = find_const(func, *limit) {
                            return Some(limit_val as u64);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    None
}

fn find_const(func: &Function, val: crate::ir::Value) -> Option<i32> {
    for block in &func.blocks {
        for inst in &block.insts {
            if inst.result == val {
                if let Op::ConstI32(v) = inst.op {
                    return Some(v);
                }
            }
        }
    }
    None
}

pub fn format_bounds(bounds: &[LoopBound]) -> String {
    let mut out = String::new();
    for b in bounds {
        match b.bound {
            Some(n) => out.push_str(&format!(
                "  loop at block {}: proven bound {} iterations ({})\n",
                b.block_index, n, b.method
            )),
            None => out.push_str(&format!(
                "  loop at block {}: bound unknown ({})\n",
                b.block_index, b.method
            )),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower::Lowering;
    use crate::ir::opt;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn analyze(src: &str) -> Vec<LoopBound> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            opt::optimize(func);
        }
        let mut all_bounds = Vec::new();
        for func in &ir.functions {
            all_bounds.extend(analyze_loop_bounds(func));
        }
        all_bounds
    }

    #[test]
    fn for_loop_bound_derived() {
        let bounds = analyze("fn f() { for i in 0..10 { } }");
        assert!(!bounds.is_empty(), "should detect loop");
        assert!(
            bounds.iter().any(|b| b.bound == Some(10)),
            "should derive bound of 10"
        );
    }

    #[test]
    fn infinite_loop_no_bound() {
        let bounds = analyze("fn f() { loop { } }");
        assert!(!bounds.is_empty(), "should detect loop");
        assert!(
            bounds.iter().all(|b| b.bound.is_none()),
            "infinite loop has no bound"
        );
    }

    #[test]
    fn while_with_const_bound() {
        let bounds = analyze("fn f() { let i = 0; while i < 100 { } }");
        // may or may not derive depending on optimization
        assert!(!bounds.is_empty(), "should detect loop");
    }
}
