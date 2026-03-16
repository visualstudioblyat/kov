use crate::ir::{Function, Op, Terminator};

// cycle costs for RV32IM on a typical single-issue in-order core
fn op_cost(op: &Op) -> u32 {
    match op {
        Op::ConstI32(_) | Op::ConstBool(_) | Op::Nop => 1,
        Op::Add(_, _)
        | Op::Sub(_, _)
        | Op::And(_, _)
        | Op::Or(_, _)
        | Op::Xor(_, _)
        | Op::Shl(_, _)
        | Op::Shr(_, _)
        | Op::Sar(_, _)
        | Op::Neg(_)
        | Op::Not(_) => 1,
        Op::Eq(_, _)
        | Op::Ne(_, _)
        | Op::Lt(_, _)
        | Op::Ge(_, _)
        | Op::Ltu(_, _)
        | Op::Geu(_, _) => 2, // sub + sltiu/sltu
        Op::Mul(_, _) => 5,
        Op::Div(_, _) | Op::Rem(_, _) => 33,
        Op::Load(_, _) | Op::VolatileLoad(_, _) => 2, // assume SRAM, no cache
        Op::Store(_, _) | Op::VolatileStore(_, _) => 2,
        Op::Call(_, _) => 4, // jal + prologue overhead
        Op::StackAlloc(_) => 1,
        Op::GlobalAddr(_) => 2, // lui + addi
        Op::Zext(_, _) | Op::Sext(_, _) | Op::Trunc(_, _) => 1,
        Op::GetErrorTag | Op::MakeError(_) => 1,
        _ => 1,
    }
}

fn terminator_cost(term: &Terminator) -> u32 {
    match term {
        Terminator::Jump(_, _) => 1,
        Terminator::BranchIf { .. } => 2, // bne + j
        Terminator::Return(_) => 3,       // epilogue
        Terminator::ReturnError(_, _) => 4,
        Terminator::TailCall(_, _) => 2,
        Terminator::Unreachable => 0,
        Terminator::None => 0,
    }
}

pub struct WcetResult {
    pub function: String,
    pub total_cycles: u32,
    pub blocks: Vec<BlockCost>,
    pub limit: Option<u32>,
    pub exceeded: bool,
}

pub struct BlockCost {
    pub index: usize,
    pub cycles: u32,
}

// compute WCET for a function (no loops — takes worst-case branch path)
pub fn analyze(func: &Function, limit: Option<u32>) -> WcetResult {
    let mut blocks: Vec<BlockCost> = Vec::new();

    for (i, block) in func.blocks.iter().enumerate() {
        let inst_cost: u32 = block.insts.iter().map(|i| op_cost(&i.op)).sum();
        let term_cost = terminator_cost(&block.terminator);
        blocks.push(BlockCost {
            index: i,
            cycles: inst_cost + term_cost,
        });
    }

    // find worst-case path through the CFG
    // for acyclic functions: enumerate all paths from entry to return
    // for functions with loops: need loop bound annotations (skip for now, just sum all blocks)
    let has_back_edge = func
        .blocks
        .iter()
        .enumerate()
        .any(|(i, b)| match &b.terminator {
            Terminator::Jump(target, _) => (target.0 as usize) <= i,
            Terminator::BranchIf {
                then_block,
                else_block,
                ..
            } => (then_block.0 as usize) <= i || (else_block.0 as usize) <= i,
            _ => false,
        });

    let total_cycles = if has_back_edge {
        // loop detected — can't compute WCET without bounds, report block costs only
        blocks.iter().map(|b| b.cycles).max().unwrap_or(0)
    } else {
        // acyclic: worst case = longest path
        worst_case_path(&func.blocks, &blocks)
    };

    let exceeded = limit.map(|l| total_cycles > l).unwrap_or(false);

    WcetResult {
        function: func.name.clone(),
        total_cycles,
        blocks,
        limit,
        exceeded,
    }
}

// find the longest path through acyclic CFG
fn worst_case_path(ir_blocks: &[crate::ir::BasicBlock], costs: &[BlockCost]) -> u32 {
    let n = ir_blocks.len();
    if n == 0 {
        return 0;
    }
    let mut longest = vec![0u32; n];
    longest[0] = costs[0].cycles;

    // topological order (forward pass since no back edges)
    for i in 0..n {
        let cost = longest[i];
        match &ir_blocks[i].terminator {
            Terminator::Jump(target, _) => {
                let t = target.0 as usize;
                if t < n {
                    longest[t] = longest[t].max(cost + costs[t].cycles);
                }
            }
            Terminator::BranchIf {
                then_block,
                else_block,
                ..
            } => {
                let t = then_block.0 as usize;
                let e = else_block.0 as usize;
                if t < n {
                    longest[t] = longest[t].max(cost + costs[t].cycles);
                }
                if e < n {
                    longest[e] = longest[e].max(cost + costs[e].cycles);
                }
            }
            _ => {}
        }
    }

    *longest.iter().max().unwrap_or(&0)
}

// format WCET results for display
pub fn format_report(results: &[WcetResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&format!("  {}(): {} cycles", r.function, r.total_cycles));
        if let Some(limit) = r.limit {
            if r.exceeded {
                out.push_str(&format!(" — EXCEEDS limit of {}", limit));
            } else {
                out.push_str(&format!(" — within limit of {}", limit));
            }
        }
        out.push('\n');
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

    fn wcet(src: &str) -> Vec<WcetResult> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            opt::optimize(func);
        }
        ir.functions.iter().map(|f| analyze(f, None)).collect()
    }

    #[test]
    fn simple_function_wcet() {
        let results = wcet("fn f(a: u32, b: u32) u32 { return a + b; }");
        assert_eq!(results.len(), 1);
        assert!(results[0].total_cycles > 0);
        assert!(results[0].total_cycles < 20);
    }

    #[test]
    fn division_is_expensive() {
        let add_results = wcet("fn f(a: u32, b: u32) u32 { return a + b; }");
        let div_results = wcet("fn f(a: u32, b: u32) u32 { return a / b; }");
        assert!(
            div_results[0].total_cycles > add_results[0].total_cycles,
            "division should be more expensive than addition"
        );
    }

    #[test]
    fn branch_takes_worst_case() {
        let results =
            wcet("fn f(x: u32) u32 { if x == 0 { return x / 1; } else { return x + 1; } }");
        // worst case should include the division path
        assert!(results[0].total_cycles >= 33);
    }

    #[test]
    fn limit_check() {
        let tokens = Lexer::tokenize("fn f(a: u32, b: u32) u32 { return a / b; }").unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);
        let result = analyze(&ir.functions[0], Some(10));
        assert!(result.exceeded, "div function should exceed 10-cycle limit");
    }
}
