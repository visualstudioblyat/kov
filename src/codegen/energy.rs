// energy-aware compilation: estimate energy consumption per function
// #[max_energy(50uJ)] fails compilation if estimated energy exceeds limit

use crate::ir::{Function, Op, Terminator};

// energy costs in picojoules for RV32IM on a typical low-power core
// these are rough estimates for a 32MHz Cortex-M class core at 1.8V
fn op_energy_pj(op: &Op) -> u64 {
    match op {
        Op::ConstI32(_) | Op::ConstBool(_) | Op::Nop => 50,
        Op::Add(_, _)
        | Op::Sub(_, _)
        | Op::And(_, _)
        | Op::Or(_, _)
        | Op::Xor(_, _)
        | Op::Shl(_, _)
        | Op::Shr(_, _)
        | Op::Sar(_, _)
        | Op::Neg(_)
        | Op::Not(_) => 100,
        Op::Eq(_, _)
        | Op::Ne(_, _)
        | Op::Lt(_, _)
        | Op::Ge(_, _)
        | Op::Ltu(_, _)
        | Op::Geu(_, _) => 150,
        Op::Mul(_, _) => 500,
        Op::Div(_, _) | Op::Rem(_, _) => 3000,
        Op::Load(_, _) | Op::VolatileLoad(_, _) => 200,
        Op::Store(_, _) | Op::VolatileStore(_, _) => 250,
        Op::Call(_, _) => 400,
        Op::StackAlloc(_) => 100,
        Op::GlobalAddr(_) => 150,
        _ => 100,
    }
}

fn term_energy_pj(term: &Terminator) -> u64 {
    match term {
        Terminator::Jump(_, _) => 100,
        Terminator::BranchIf { .. } => 200,
        Terminator::Return(_) => 300,
        _ => 100,
    }
}

pub struct EnergyResult {
    pub function: String,
    pub total_pj: u64,
    pub total_uj: f64,
}

pub fn analyze_energy(func: &Function) -> EnergyResult {
    let mut total = 0u64;
    for block in &func.blocks {
        for inst in &block.insts {
            total += op_energy_pj(&inst.op);
        }
        total += term_energy_pj(&block.terminator);
    }

    EnergyResult {
        function: func.name.clone(),
        total_pj: total,
        total_uj: total as f64 / 1_000_000.0,
    }
}

pub fn format_energy(results: &[EnergyResult]) -> String {
    let mut out = String::new();
    for r in results {
        out.push_str(&format!(
            "  {}(): {:.3} uJ ({} pJ)\n",
            r.function, r.total_uj, r.total_pj
        ));
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

    #[test]
    fn energy_estimate() {
        let tokens = Lexer::tokenize("fn f(a: u32, b: u32) u32 { return a + b; }").unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut ir = Lowering::lower(&program);
        for func in &mut ir.functions {
            opt::optimize(func);
        }
        let result = analyze_energy(&ir.functions[0]);
        assert!(result.total_pj > 0);
        assert!(result.total_uj < 1.0); // simple add should be < 1 uJ
    }

    #[test]
    fn division_costs_more() {
        let tokens_add = Lexer::tokenize("fn f(a: u32, b: u32) u32 { return a + b; }").unwrap();
        let tokens_div = Lexer::tokenize("fn f(a: u32, b: u32) u32 { return a / b; }").unwrap();

        let prog_add = Parser::new(tokens_add).parse().unwrap();
        let prog_div = Parser::new(tokens_div).parse().unwrap();

        let ir_add = Lowering::lower(&prog_add);
        let ir_div = Lowering::lower(&prog_div);

        let e_add = analyze_energy(&ir_add.functions[0]);
        let e_div = analyze_energy(&ir_div.functions[0]);

        assert!(
            e_div.total_pj > e_add.total_pj,
            "division should cost more energy"
        );
    }
}
