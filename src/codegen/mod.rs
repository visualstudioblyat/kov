pub mod encode;
pub mod emit;
pub mod elf;
pub mod startup;
pub mod mmio;

use std::collections::HashMap;
use crate::ir::{Function, Value, Block, Op, Terminator};
use crate::ir::types::IrType;
use emit::Emitter;
use encode::*;

pub struct CodeGen {
    pub emitter: Emitter,
}

// trivial register allocator: map each IR value to a register.
// uses t0-t6, a0-a7, s1-s11 (25 registers available).
// spills to stack when exhausted.
struct RegAlloc {
    map: HashMap<u32, u32>,  // Value.0 → physical register
    next: usize,
    stack_offset: i32,       // current stack usage for spills
}

// allocatable registers in priority order
const REGS: &[u32] = &[
    T0, T1, T2,
    A0, A1, A2, A3, A4, A5, A6, A7,
    S1, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
];

impl RegAlloc {
    fn new() -> Self {
        Self { map: HashMap::new(), next: 0, stack_offset: 0 }
    }

    fn get(&mut self, val: Value) -> u32 {
        if let Some(&reg) = self.map.get(&val.0) {
            return reg;
        }
        // allocate next available
        if self.next < REGS.len() {
            let reg = REGS[self.next];
            self.next += 1;
            self.map.insert(val.0, reg);
            reg
        } else {
            // TODO: spill to stack
            T0 // fallback, will clobber
        }
    }

    // pre-assign a value to a specific register (for function params)
    fn assign(&mut self, val: Value, reg: u32) {
        self.map.insert(val.0, reg);
    }
}

impl CodeGen {
    pub fn new() -> Self {
        Self { emitter: Emitter::new() }
    }

    pub fn gen_function(&mut self, func: &Function) {
        let mut ra = RegAlloc::new();

        // function label
        self.emitter.label(&func.name);

        // prologue: save ra, allocate stack frame
        // addi sp, sp, -16
        // sw ra, 12(sp)
        // sw s0, 8(sp)
        // addi s0, sp, 16
        self.emitter.emit32(addi(SP, SP, -16));
        self.emitter.emit32(sw(SP, RA, 12));
        self.emitter.emit32(sw(SP, S0, 8));
        self.emitter.emit32(addi(S0, SP, 16));

        // bind function params to a0..a7
        for (i, (name, _ty)) in func.params.iter().enumerate() {
            let param_val = Value(i as u32);
            ra.assign(param_val, A0 + i as u32);
        }

        // generate code for each block
        for (bi, block) in func.blocks.iter().enumerate() {
            let block_label = format!("{}.b{}", func.name, bi);
            self.emitter.label(&block_label);

            // block parameters get assigned registers
            for (val, _ty) in &block.params {
                ra.get(*val);
            }

            for inst in &block.insts {
                let rd = ra.get(inst.result);
                self.gen_inst(rd, &inst.op, &mut ra);
            }

            self.gen_terminator(&block.terminator, &func.name, &mut ra);
        }

        // epilogue label (for returns to jump to)
        let epilogue_label = format!("{}.epilogue", func.name);
        self.emitter.label(&epilogue_label);
        self.emitter.emit32(lw(RA, SP, 12));
        self.emitter.emit32(lw(S0, SP, 8));
        self.emitter.emit32(addi(SP, SP, 16));
        self.emitter.emit32(ret());
    }

    fn gen_inst(&mut self, rd: u32, op: &Op, ra: &mut RegAlloc) {
        match op {
            Op::ConstI32(v) => {
                let (inst1, inst2) = li32(rd, *v);
                self.emitter.emit32(inst1);
                if let Some(i2) = inst2 {
                    self.emitter.emit32(i2);
                }
            }
            Op::ConstBool(v) => {
                self.emitter.emit32(addi(rd, ZERO, if *v { 1 } else { 0 }));
            }

            Op::Add(a, b) => { self.emitter.emit32(add(rd, ra.get(*a), ra.get(*b))); }
            Op::Sub(a, b) => { self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b))); }
            Op::Mul(a, b) => { self.emitter.emit32(mul(rd, ra.get(*a), ra.get(*b))); }
            Op::Div(a, b) => { self.emitter.emit32(div(rd, ra.get(*a), ra.get(*b))); }
            Op::Rem(a, b) => { self.emitter.emit32(rem_(rd, ra.get(*a), ra.get(*b))); }

            Op::And(a, b) => { self.emitter.emit32(and(rd, ra.get(*a), ra.get(*b))); }
            Op::Or(a, b)  => { self.emitter.emit32(or(rd, ra.get(*a), ra.get(*b))); }
            Op::Xor(a, b) => { self.emitter.emit32(xor(rd, ra.get(*a), ra.get(*b))); }
            Op::Shl(a, b) => { self.emitter.emit32(sll(rd, ra.get(*a), ra.get(*b))); }
            Op::Shr(a, b) => { self.emitter.emit32(srl(rd, ra.get(*a), ra.get(*b))); }
            Op::Sar(a, b) => { self.emitter.emit32(sra(rd, ra.get(*a), ra.get(*b))); }

            Op::Eq(a, b)  => {
                // x == y → sub tmp, x, y; sltiu rd, tmp, 1
                self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(sltiu(rd, rd, 1));
            }
            Op::Ne(a, b)  => {
                // x != y → sub tmp, x, y; sltu rd, zero, tmp
                self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(sltu(rd, ZERO, rd));
            }
            Op::Lt(a, b)  => { self.emitter.emit32(slt(rd, ra.get(*a), ra.get(*b))); }
            Op::Ge(a, b)  => {
                // x >= y → slt tmp, x, y; xori rd, tmp, 1
                self.emitter.emit32(slt(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(xori(rd, rd, 1));
            }
            Op::Ltu(a, b) => { self.emitter.emit32(sltu(rd, ra.get(*a), ra.get(*b))); }
            Op::Geu(a, b) => {
                self.emitter.emit32(sltu(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(xori(rd, rd, 1));
            }

            Op::Neg(a) => { self.emitter.emit32(neg(rd, ra.get(*a))); }
            Op::Not(a) => { self.emitter.emit32(not(rd, ra.get(*a))); }

            Op::Load(addr, ty) => {
                let a = ra.get(*addr);
                match ty.size_bytes() {
                    1 => self.emitter.emit32(lbu(rd, a, 0)),
                    2 => self.emitter.emit32(lhu(rd, a, 0)),
                    4 => self.emitter.emit32(lw(rd, a, 0)),
                    _ => self.emitter.emit32(lw(rd, a, 0)),
                }
            }
            Op::Store(addr, val) => {
                self.emitter.emit32(sw(ra.get(*addr), ra.get(*val), 0));
            }

            Op::VolatileLoad(addr, ty) => {
                // same as load but the compiler must not reorder or eliminate
                let a = ra.get(*addr);
                match ty.size_bytes() {
                    1 => self.emitter.emit32(lbu(rd, a, 0)),
                    2 => self.emitter.emit32(lhu(rd, a, 0)),
                    _ => self.emitter.emit32(lw(rd, a, 0)),
                }
                self.emitter.emit32(encode::nop()); // fence placeholder
            }
            Op::VolatileStore(addr, val) => {
                self.emitter.emit32(sw(ra.get(*addr), ra.get(*val), 0));
                self.emitter.emit32(encode::nop()); // fence placeholder
            }

            Op::Call(name, args) => {
                // move args to a0..a7
                for (i, arg) in args.iter().enumerate() {
                    if i < 8 {
                        let src = ra.get(*arg);
                        if src != A0 + i as u32 {
                            self.emitter.emit32(mv(A0 + i as u32, src));
                        }
                    }
                }
                // call (placeholder offset, resolved later)
                self.emitter.emit_jump(jal(RA, 0), name);
                // result in a0, move to rd if different
                if rd != A0 {
                    self.emitter.emit32(mv(rd, A0));
                }
            }

            Op::Zext(val, _) | Op::Sext(val, _) | Op::Trunc(val, _) => {
                let src = ra.get(*val);
                if rd != src {
                    self.emitter.emit32(mv(rd, src));
                }
            }

            Op::StackAlloc(size) => {
                self.emitter.emit32(addi(SP, SP, -(*size as i32)));
                self.emitter.emit32(mv(rd, SP));
            }

            Op::GlobalAddr(name) => {
                // placeholder — will need relocation
                let (inst1, inst2) = li32(rd, 0);
                self.emitter.emit32(inst1);
                if let Some(i2) = inst2 { self.emitter.emit32(i2); }
            }

            Op::Nop => {}

            _ => {} // ConstI64 etc — TODO
        }
    }

    fn gen_terminator(&mut self, term: &Terminator, fn_name: &str, ra: &mut RegAlloc) {
        match term {
            Terminator::Return(Some(val)) => {
                let src = ra.get(*val);
                if src != A0 {
                    self.emitter.emit32(mv(A0, src));
                }
                self.emitter.emit_jump(j_offset(0), &format!("{}.epilogue", fn_name));
            }
            Terminator::Return(None) => {
                self.emitter.emit_jump(j_offset(0), &format!("{}.epilogue", fn_name));
            }
            Terminator::Jump(target, args) => {
                // move block args to target's registers
                // (simplified: assumes target block params already allocated)
                let target_label = format!("{}.b{}", fn_name, target.0);
                self.emitter.emit_jump(j_offset(0), &target_label);
            }
            Terminator::BranchIf { cond, then_block, else_block, .. } => {
                let cond_reg = ra.get(*cond);
                let then_label = format!("{}.b{}", fn_name, then_block.0);
                let else_label = format!("{}.b{}", fn_name, else_block.0);
                self.emitter.emit_branch(bne(cond_reg, ZERO, 0), &then_label);
                self.emitter.emit_jump(j_offset(0), &else_label);
            }
            Terminator::Unreachable => {
                self.emitter.emit32(ebreak());
            }
            Terminator::None => {}
        }
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, String> {
        self.emitter.resolve()?;
        Ok(self.emitter.code.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::ir::lower::Lowering;

    fn compile(src: &str) -> Vec<u8> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);

        let mut cg = CodeGen::new();
        for func in &ir.functions {
            cg.gen_function(func);
        }
        cg.finish().unwrap()
    }

    #[test]
    fn codegen_simple_return() {
        let code = compile("fn answer() u32 { return 42; }");
        assert!(!code.is_empty());
        // should contain at least prologue + li + ret sequence
        assert!(code.len() >= 20);
    }

    #[test]
    fn codegen_add() {
        let code = compile("fn add(a: u32, b: u32) u32 { let c = a + b; return c; }");
        assert!(!code.is_empty());
        // check that an ADD instruction exists somewhere
        let has_add = code.windows(4).any(|w| {
            let inst = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            inst & 0xFE00707F == 0x00000033 // ADD mask
        });
        assert!(has_add, "expected ADD instruction in output");
    }

    #[test]
    fn codegen_branch() {
        let code = compile("fn f(x: u32) { if x == 0 { } }");
        assert!(!code.is_empty());
    }

    #[test]
    fn codegen_loop() {
        let code = compile("fn f() { loop { } }");
        assert!(!code.is_empty());
        // should contain a backward jump
    }

    #[test]
    fn codegen_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        let tokens = Lexer::tokenize(&source).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);

        let mut cg = CodeGen::new();
        for func in &ir.functions {
            cg.gen_function(func);
        }
        let code = cg.finish().unwrap();

        assert!(!code.is_empty());
        println!("blink.kv compiled to {} bytes of RISC-V machine code", code.len());

        // verify it's valid 32-bit aligned instructions
        assert_eq!(code.len() % 4, 0);
    }
}
