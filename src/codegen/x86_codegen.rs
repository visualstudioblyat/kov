// x86-64 code generation from Kov IR
// uses the x86 encoder + ELF64 writer to produce native .o files

use super::elf64::*;
use super::x86::*;
use crate::ir::{Function, Op, Terminator, Value};
use std::collections::HashMap;

// x86-64 register allocation pool
// caller-saved first (no save/restore needed for temps)
const X86_REGS: [u8; 14] = [
    RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11, // caller-saved
    RBX, R12, R13, R14, R15, // callee-saved
];

const X86_CALLEE_SAVED: [u8; 5] = [RBX, R12, R13, R14, R15];

struct X86RegAlloc {
    map: HashMap<u32, u8>,
    free: Vec<u8>,
    used_callee: Vec<u8>,
}

impl X86RegAlloc {
    fn new() -> Self {
        let mut free = X86_REGS.to_vec();
        free.reverse();
        Self {
            map: HashMap::new(),
            free,
            used_callee: Vec::new(),
        }
    }

    fn get(&mut self, val: Value) -> u8 {
        if let Some(&reg) = self.map.get(&val.0) {
            return reg;
        }
        if let Some(reg) = self.free.pop() {
            self.map.insert(val.0, reg);
            if X86_CALLEE_SAVED.contains(&reg) && !self.used_callee.contains(&reg) {
                self.used_callee.push(reg);
            }
            reg
        } else {
            RAX // fallback
        }
    }

    fn assign(&mut self, val: Value, reg: u8) {
        self.map.insert(val.0, reg);
        self.free.retain(|&r| r != reg);
    }

    fn frame_size(&self) -> i32 {
        // callee-saved regs * 8, aligned to 16
        let saves = self.used_callee.len() as i32 * 8;
        ((saves + 15) & !15).max(16)
    }
}

pub struct X86CodeGen {
    pub emitter: X86Emitter,
    pub elf: Elf64Writer,
    pub externs: Vec<String>,
}

impl X86CodeGen {
    pub fn new() -> Self {
        Self {
            emitter: X86Emitter::new(),
            elf: Elf64Writer::new(),
            externs: Vec::new(),
        }
    }

    pub fn add_extern(&mut self, name: &str) {
        if !self.externs.contains(&name.to_string()) {
            self.externs.push(name.to_string());
            self.elf.add_extern(name);
        }
    }

    pub fn gen_function(&mut self, func: &Function) {
        let mut ra = X86RegAlloc::new();
        let fn_start = self.emitter.pos();

        // bind params to ABI registers
        for (i, (_name, _ty)) in func.params.iter().enumerate() {
            if i < ARG_REGS.len() {
                let param_val = Value(i as u32);
                ra.assign(param_val, ARG_REGS[i]);
            }
        }

        // generate body into temp buffer to discover callee-saved usage
        let mut body = X86Emitter::new();
        std::mem::swap(&mut self.emitter, &mut body);

        for (bi, block) in func.blocks.iter().enumerate() {
            let label = format!("{}.b{}", func.name, bi);
            self.emitter.label(&label);

            for (val, _ty) in &block.params {
                ra.get(*val);
            }

            for inst in &block.insts {
                let rd = ra.get(inst.result);
                self.gen_inst(rd, &inst.op, &mut ra);
            }
            self.gen_terminator(&block.terminator, &func.name, &mut ra);
        }

        std::mem::swap(&mut self.emitter, &mut body);

        // emit real function: label + prologue + body + epilogue
        self.emitter.label(&func.name);

        // prologue: push callee-saved regs + align stack
        let frame = ra.frame_size();
        for &reg in &ra.used_callee {
            self.emitter.push(reg);
        }
        if frame > 0 {
            self.emitter.sub_ri32(RSP, frame);
        }

        // append body
        let body_offset = self.emitter.pos();
        self.emitter.code.extend_from_slice(&body.code);
        for (name, pos) in &body.labels {
            self.emitter.labels.insert(name.clone(), pos + body_offset);
        }
        for fixup in &body.fixups {
            self.emitter.fixups.push(X86Fixup {
                offset: fixup.offset + body_offset,
                label: fixup.label.clone(),
                kind: X86FixupKind::Rel32,
            });
        }

        // epilogue
        let epilogue = format!("{}.epilogue", func.name);
        self.emitter.label(&epilogue);
        if frame > 0 {
            self.emitter.add_ri32(RSP, frame);
        }
        for &reg in ra.used_callee.iter().rev() {
            self.emitter.pop(reg);
        }
        self.emitter.ret();

        let fn_size = self.emitter.pos() - fn_start;
        self.elf
            .add_function(&func.name, fn_start as u64, fn_size as u64);
    }

    fn gen_inst(&mut self, rd: u8, op: &Op, ra: &mut X86RegAlloc) {
        match op {
            Op::ConstI32(v) => {
                if *v == 0 {
                    self.emitter.zero_reg(rd); // xor optimization
                } else {
                    self.emitter.mov_ri32(rd, *v);
                }
            }
            Op::ConstBool(v) => {
                if *v {
                    self.emitter.mov_ri32(rd, 1);
                } else {
                    self.emitter.zero_reg(rd);
                }
            }

            Op::Add(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.add_rr(rd, rb_reg);
            }
            Op::Sub(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.sub_rr(rd, rb_reg);
            }
            Op::Mul(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.imul_rr(rd, rb_reg);
            }
            Op::And(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.and_rr(rd, rb_reg);
            }
            Op::Or(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.or_rr(rd, rb_reg);
            }
            Op::Xor(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                if rd != ra_reg {
                    self.emitter.mov_rr(rd, ra_reg);
                }
                self.emitter.xor_rr(rd, rb_reg);
            }

            Op::Eq(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                self.emitter.cmp_rr(ra_reg, rb_reg);
                self.emitter.sete(rd);
                self.emitter.movzx_r8(rd, rd);
            }
            Op::Ne(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                self.emitter.cmp_rr(ra_reg, rb_reg);
                self.emitter.setne(rd);
                self.emitter.movzx_r8(rd, rd);
            }
            Op::Lt(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                self.emitter.cmp_rr(ra_reg, rb_reg);
                self.emitter.setl(rd);
                self.emitter.movzx_r8(rd, rd);
            }
            Op::Ge(a, b) => {
                let ra_reg = ra.get(*a);
                let rb_reg = ra.get(*b);
                self.emitter.cmp_rr(ra_reg, rb_reg);
                self.emitter.setge(rd);
                self.emitter.movzx_r8(rd, rd);
            }

            Op::Neg(a) => {
                let src = ra.get(*a);
                if rd != src {
                    self.emitter.mov_rr(rd, src);
                }
                self.emitter.neg(rd);
            }
            Op::Not(a) => {
                let src = ra.get(*a);
                if rd != src {
                    self.emitter.mov_rr(rd, src);
                }
                self.emitter.not(rd);
            }

            Op::Load(addr, _ty) => {
                let base = ra.get(*addr);
                self.emitter.mov_load(rd, base, 0);
            }
            Op::Store(addr, val) => {
                let base = ra.get(*addr);
                let src = ra.get(*val);
                self.emitter.mov_store(base, 0, src);
            }

            Op::Call(name, args) => {
                // set up args per System V AMD64 ABI
                for (i, arg) in args.iter().enumerate() {
                    if i < ARG_REGS.len() {
                        let src = ra.get(*arg);
                        if src != ARG_REGS[i] {
                            self.emitter.mov_rr(ARG_REGS[i], src);
                        }
                    }
                }
                self.emitter.call(name);
                if rd != RAX {
                    self.emitter.mov_rr(rd, RAX);
                }
            }

            Op::Nop => {}
            _ => {}
        }
    }

    fn gen_terminator(&mut self, term: &Terminator, fn_name: &str, ra: &mut X86RegAlloc) {
        match term {
            Terminator::Return(Some(val)) => {
                let src = ra.get(*val);
                if src != RAX {
                    self.emitter.mov_rr(RAX, src);
                }
                self.emitter.jmp(&format!("{}.epilogue", fn_name));
            }
            Terminator::Return(None) => {
                self.emitter.jmp(&format!("{}.epilogue", fn_name));
            }
            Terminator::Jump(target, _) => {
                self.emitter.jmp(&format!("{}.b{}", fn_name, target.0));
            }
            Terminator::BranchIf {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let cr = ra.get(*cond);
                self.emitter.test_rr(cr, cr);
                self.emitter.jne(&format!("{}.b{}", fn_name, then_block.0));
                self.emitter.jmp(&format!("{}.b{}", fn_name, else_block.0));
            }
            Terminator::Unreachable => {
                // ud2
                self.emitter.emit(&[0x0F, 0x0B]);
            }
            _ => {}
        }
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, String> {
        // collect unresolved symbols
        let unresolved: Vec<(usize, String)> = self
            .emitter
            .fixups
            .iter()
            .filter(|f| !self.emitter.labels.contains_key(&f.label))
            .map(|f| (f.offset, f.label.clone()))
            .collect();

        for (offset, label) in &unresolved {
            self.add_extern(label);
            self.elf.add_relocation(*offset as u64, label, -4);
        }

        self.emitter.resolve()?;
        self.elf.code = self.emitter.code.clone();
        Ok(self.elf.write())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::lower::Lowering;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile_x86(src: &str) -> Vec<u8> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);
        let mut cg = X86CodeGen::new();
        for func in &ir.functions {
            cg.gen_function(func);
        }
        cg.finish().unwrap()
    }

    #[test]
    fn x86_simple_return() {
        let elf = compile_x86("fn answer() u32 { return 42; }");
        assert!(!elf.is_empty());
        assert_eq!(&elf[0..4], &[0x7F, b'E', b'L', b'F']);
        assert_eq!(elf[4], 2); // ELFCLASS64
    }

    #[test]
    fn x86_add() {
        let elf = compile_x86("fn add(a: u32, b: u32) u32 { return a + b; }");
        assert!(!elf.is_empty());
    }

    #[test]
    fn x86_branch() {
        let elf = compile_x86("fn f(x: u32) u32 { if x == 0 { return 1; } return 0; }");
        assert!(!elf.is_empty());
    }

    #[test]
    fn x86_loop() {
        let elf = compile_x86("fn f() { let x = 0; while x < 10 { } }");
        assert!(!elf.is_empty());
    }
}
