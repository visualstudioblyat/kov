pub mod alloc;
pub mod arm;
pub mod builtins;
pub mod cheader;
pub mod compress;
pub mod crash;
pub mod defmt;
pub mod delta;
pub mod disasm;
pub mod elf;
pub mod elf64;
pub mod emit;
pub mod encode;
pub mod energy;
pub mod loopbound;
pub mod mmio;
pub mod stack;
pub mod startup;
pub mod svd;
pub mod wcet;
pub mod x86;
pub mod x86_codegen;

use crate::ir::globals::GlobalTable;
use crate::ir::{Function, Op, Terminator, Value};
use emit::Emitter;
use encode::*;
use std::collections::HashMap;

pub struct CodeGen {
    pub emitter: Emitter,
    pub ram_base: u32,
    pub global_addrs: HashMap<String, u32>, // name → absolute address
}

// s-register numbers for callee-saved tracking
const S_REGS: &[u32] = &[S0, S1, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27];

struct RegAlloc {
    map: HashMap<u32, u32>, // Value.0 → register
    free_regs: Vec<u32>,    // available registers (stack, pop to allocate)
    used_s_regs: Vec<u32>,  // callee-saved registers actually used
    spill_count: i32,
    spill_slots: HashMap<u32, i32>,
    pending_spills: Vec<(u32, u32, i32)>,
    pending_loads: Vec<(u32, i32)>,
    last_use: HashMap<u32, usize>, // Value.0 → instruction index of last use
    current_inst: usize,           // current instruction index during codegen
}

// allocatable registers in priority order: temporaries first, then callee-saved
const REGS: &[u32] = &[
    T0, T1, T2, A0, A1, A2, A3, A4, A5, A6, A7, S1, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
];

// compute last-use index for every value in a function
fn compute_last_use(func: &Function) -> HashMap<u32, usize> {
    let mut last_use: HashMap<u32, usize> = HashMap::new();
    let mut idx = 0usize;
    for block in &func.blocks {
        for inst in &block.insts {
            for v in op_uses(&inst.op) {
                last_use.insert(v, idx);
            }
            idx += 1;
        }
        // terminators also use values
        for v in term_uses(&block.terminator) {
            last_use.insert(v, idx);
        }
        idx += 1;
    }
    last_use
}

fn op_uses(op: &Op) -> Vec<u32> {
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
        | Op::Geu(a, b)
        | Op::Store(a, b)
        | Op::VolatileStore(a, b) => vec![a.0, b.0],
        Op::Neg(a)
        | Op::Not(a)
        | Op::Load(a, _)
        | Op::VolatileLoad(a, _)
        | Op::Zext(a, _)
        | Op::Sext(a, _)
        | Op::Trunc(a, _)
        | Op::MakeError(a) => vec![a.0],
        Op::Call(_, args) => args.iter().map(|a| a.0).collect(),
        _ => vec![],
    }
}

fn term_uses(term: &Terminator) -> Vec<u32> {
    match term {
        Terminator::Return(Some(v)) => vec![v.0],
        Terminator::ReturnError(a, b) => vec![a.0, b.0],
        Terminator::BranchIf { cond, .. } => vec![cond.0],
        Terminator::Jump(_, args) => args.iter().map(|a| a.0).collect(),
        Terminator::TailCall(_, args) => args.iter().map(|a| a.0).collect(),
        _ => vec![],
    }
}

impl RegAlloc {
    fn new(last_use: HashMap<u32, usize>) -> Self {
        // initialize free regs in reverse order (pop gives temporaries first)
        let mut free = REGS.to_vec();
        free.reverse();
        Self {
            map: HashMap::new(),
            free_regs: free,
            used_s_regs: Vec::new(),
            spill_count: 0,
            spill_slots: HashMap::new(),
            pending_spills: Vec::new(),
            pending_loads: Vec::new(),
            last_use,
            current_inst: 0,
        }
    }

    // free registers for values that are dead after the current instruction
    fn expire_old(&mut self) {
        let mut dead_vals: Vec<u32> = self
            .map
            .keys()
            .filter(|val_id| self.last_use.get(val_id).copied().unwrap_or(0) < self.current_inst)
            .copied()
            .collect();
        dead_vals.sort(); // deterministic order for reproducible builds
        for val_id in dead_vals {
            if let Some(reg) = self.map.remove(&val_id) {
                if !self.free_regs.contains(&reg) {
                    self.free_regs.push(reg);
                    self.free_regs.sort();
                    self.free_regs.reverse(); // keep temporaries first
                }
            }
        }
    }

    fn get(&mut self, val: Value) -> u32 {
        if let Some(&reg) = self.map.get(&val.0) {
            if let Some(&slot) = self.spill_slots.get(&val.0) {
                self.pending_loads.push((reg, slot));
            }
            return reg;
        }

        // expire dead values to free their registers
        self.expire_old();

        if let Some(reg) = self.free_regs.pop() {
            self.map.insert(val.0, reg);
            if S_REGS.contains(&reg) && !self.used_s_regs.contains(&reg) {
                self.used_s_regs.push(reg);
            }
            reg
        } else {
            // spill: evict value with the latest last-use (furthest in future)
            let evict_val = self
                .map
                .iter()
                .filter(|(_, r)| **r != T0 && **r != T1)
                .max_by_key(|(v, _)| self.last_use.get(v).copied().unwrap_or(0))
                .map(|(v, r)| (*v, *r));

            if let Some((evict_id, evict_reg)) = evict_val {
                let slot = self.spill_count;
                self.spill_count += 1;
                self.spill_slots.insert(evict_id, slot);
                self.pending_spills.push((evict_id, evict_reg, slot));
                self.map.remove(&evict_id);
                self.map.insert(val.0, evict_reg);
                evict_reg
            } else {
                T0
            }
        }
    }

    fn assign(&mut self, val: Value, reg: u32) {
        self.map.insert(val.0, reg);
        // remove from free list
        self.free_regs.retain(|&r| r != reg);
    }

    fn advance_inst(&mut self) {
        self.current_inst += 1;
    }

    fn frame_size(&self) -> i32 {
        let save_slots = 1 + self.used_s_regs.len() as i32;
        let total_slots = save_slots + self.spill_count;
        ((total_slots * 4) + 15) & !15
    }

    fn spill_offset(&self, slot: i32) -> i32 {
        slot * 4
    }
}

impl Default for CodeGen {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGen {
    pub fn new() -> Self {
        Self {
            emitter: Emitter::new(),
            ram_base: 0x2000_0000,
            global_addrs: HashMap::new(),
        }
    }

    pub fn new_with_globals(ram_base: u32, globals: &GlobalTable) -> Self {
        let mut global_addrs = HashMap::new();
        for g in &globals.globals {
            if let Some(offset) = globals.offset_of(&g.name) {
                global_addrs.insert(g.name.clone(), ram_base + offset);
            }
        }
        for (label, _) in &globals.strings {
            if let Some(offset) = globals.offset_of(label) {
                global_addrs.insert(label.clone(), ram_base + offset);
            }
        }
        Self {
            emitter: Emitter::new(),
            ram_base,
            global_addrs,
        }
    }

    pub fn gen_function(&mut self, func: &Function) {
        // two-pass approach:
        // 1. generate body into a temporary emitter to discover register usage
        // 2. emit real prologue + body + epilogue

        let mut body_emitter = Emitter::new();
        let last_use = compute_last_use(func);
        let mut ra = RegAlloc::new(last_use);

        // bind function params to a0..a7
        for (i, (_name, _ty)) in func.params.iter().enumerate() {
            let param_val = Value(i as u32);
            ra.assign(param_val, A0 + i as u32);
        }

        // generate body into temporary emitter
        std::mem::swap(&mut self.emitter, &mut body_emitter);

        for (bi, block) in func.blocks.iter().enumerate() {
            let block_label = format!("{}.b{}", func.name, bi);
            self.emitter.label(&block_label);

            for (val, _ty) in &block.params {
                ra.get(*val);
            }

            for inst in &block.insts {
                let rd = ra.get(inst.result);
                self.flush_spills(&mut ra);
                self.gen_inst(rd, &inst.op, &mut ra);
                self.flush_loads(&mut ra);
                ra.advance_inst();
            }

            self.flush_spills(&mut ra);
            self.gen_terminator(&block.terminator, &func.name, &mut ra);
            self.flush_loads(&mut ra);
            ra.advance_inst();
        }

        // swap back — body_emitter now has the body code
        std::mem::swap(&mut self.emitter, &mut body_emitter);

        // now emit real function: prologue + body + epilogue
        let frame = ra.frame_size();

        // function label
        self.emitter.label(&func.name);

        // prologue: allocate frame, save RA and used s-regs
        self.emitter.emit32(addi(SP, SP, -frame));
        self.emitter.emit32(sw(SP, RA, frame - 4));
        for (i, &sreg) in ra.used_s_regs.iter().enumerate() {
            self.emitter
                .emit32(sw(SP, sreg, frame - 8 - (i as i32 * 4)));
        }

        // append body (copy code and transfer labels/fixups)
        let body_offset = self.emitter.code.len();
        self.emitter.code.extend_from_slice(&body_emitter.code);
        // transfer labels with offset adjustment
        for (name, pos) in &body_emitter.labels {
            self.emitter.labels.insert(name.clone(), pos + body_offset);
        }
        // transfer fixups with offset adjustment
        for fixup in &body_emitter.fixups {
            self.emitter.fixups.push(emit::Fixup {
                offset: fixup.offset + body_offset,
                label: fixup.label.clone(),
                kind: fixup.kind,
            });
        }

        // epilogue
        let epilogue_label = format!("{}.epilogue", func.name);
        self.emitter.label(&epilogue_label);
        self.emitter.emit32(lw(RA, SP, frame - 4));
        for (i, &sreg) in ra.used_s_regs.iter().enumerate() {
            self.emitter
                .emit32(lw(sreg, SP, frame - 8 - (i as i32 * 4)));
        }
        self.emitter.emit32(addi(SP, SP, frame));
        self.emitter.emit32(ret());
    }

    fn flush_spills(&mut self, ra: &mut RegAlloc) {
        let spills: Vec<_> = ra.pending_spills.drain(..).collect();
        for (_val_id, reg, slot) in spills {
            let offset = ra.spill_offset(slot);
            self.emitter.emit32(sw(SP, reg, offset));
        }
    }

    fn flush_loads(&mut self, ra: &mut RegAlloc) {
        let loads: Vec<_> = ra.pending_loads.drain(..).collect();
        for (reg, slot) in loads {
            let offset = ra.spill_offset(slot);
            self.emitter.emit32(lw(reg, SP, offset));
        }
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

            Op::Add(a, b) => {
                self.emitter.emit32(add(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Sub(a, b) => {
                self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Mul(a, b) => {
                self.emitter.emit32(mul(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Div(a, b) => {
                self.emitter.emit32(div(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Rem(a, b) => {
                self.emitter.emit32(rem_(rd, ra.get(*a), ra.get(*b)));
            }

            Op::And(a, b) => {
                self.emitter.emit32(and(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Or(a, b) => {
                self.emitter.emit32(or(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Xor(a, b) => {
                self.emitter.emit32(xor(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Shl(a, b) => {
                self.emitter.emit32(sll(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Shr(a, b) => {
                self.emitter.emit32(srl(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Sar(a, b) => {
                self.emitter.emit32(sra(rd, ra.get(*a), ra.get(*b)));
            }

            Op::Eq(a, b) => {
                self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(sltiu(rd, rd, 1));
            }
            Op::Ne(a, b) => {
                self.emitter.emit32(sub(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(sltu(rd, ZERO, rd));
            }
            Op::Lt(a, b) => {
                self.emitter.emit32(slt(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Ge(a, b) => {
                self.emitter.emit32(slt(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(xori(rd, rd, 1));
            }
            Op::Ltu(a, b) => {
                self.emitter.emit32(sltu(rd, ra.get(*a), ra.get(*b)));
            }
            Op::Geu(a, b) => {
                self.emitter.emit32(sltu(rd, ra.get(*a), ra.get(*b)));
                self.emitter.emit32(xori(rd, rd, 1));
            }

            Op::Neg(a) => {
                self.emitter.emit32(neg(rd, ra.get(*a)));
            }
            Op::Not(a) => {
                self.emitter.emit32(not(rd, ra.get(*a)));
            }

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
                let a = ra.get(*addr);
                match ty.size_bytes() {
                    1 => self.emitter.emit32(lbu(rd, a, 0)),
                    2 => self.emitter.emit32(lhu(rd, a, 0)),
                    _ => self.emitter.emit32(lw(rd, a, 0)),
                }
                self.emitter.emit32(encode::nop());
            }
            Op::VolatileStore(addr, val) => {
                self.emitter.emit32(sw(ra.get(*addr), ra.get(*val), 0));
                self.emitter.emit32(encode::nop());
            }

            Op::Call(name, args) => {
                for (i, arg) in args.iter().enumerate() {
                    if i < 8 {
                        let src = ra.get(*arg);
                        if src != A0 + i as u32 {
                            self.emitter.emit32(mv(A0 + i as u32, src));
                        }
                    }
                }
                self.emitter.emit_jump(jal(RA, 0), name);
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
                let addr = self.global_addrs.get(name).copied().unwrap_or(0) as i32;
                let (inst1, inst2) = li32(rd, addr);
                self.emitter.emit32(inst1);
                if let Some(i2) = inst2 {
                    self.emitter.emit32(i2);
                }
            }

            Op::GetErrorTag => {
                // error tag is in a1 after a call
                if rd != A1 {
                    self.emitter.emit32(mv(rd, A1));
                }
            }

            Op::MakeError(val) => {
                // set a0 = val (payload), a1 = 1 (error tag)
                let src = ra.get(*val);
                if src != A0 {
                    self.emitter.emit32(mv(A0, src));
                }
                self.emitter.emit32(addi(A1, ZERO, 1));
                if rd != A0 {
                    self.emitter.emit32(mv(rd, A0));
                }
            }

            Op::InlineAsm(template, _operands) => {
                // basic inline asm: encode known instructions
                match template.trim() {
                    "nop" => self.emitter.emit32(nop()),
                    "wfi" => self.emitter.emit32(wfi()),
                    "ebreak" => self.emitter.emit32(ebreak()),
                    "fence" => self.emitter.emit32(0x0000000F),
                    _ => {
                        // try to parse as hex literal: "0xNNNNNNNN"
                        if let Some(hex) = template.trim().strip_prefix("0x") {
                            if let Ok(inst) = u32::from_str_radix(hex, 16) {
                                self.emitter.emit32(inst);
                            } else {
                                self.emitter.emit32(nop()); // fallback
                            }
                        } else {
                            self.emitter.emit32(nop()); // unknown template
                        }
                    }
                }
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
                self.emitter
                    .emit_jump(j_offset(0), &format!("{}.epilogue", fn_name));
            }
            Terminator::Return(None) => {
                self.emitter
                    .emit_jump(j_offset(0), &format!("{}.epilogue", fn_name));
            }
            Terminator::Jump(target, _args) => {
                let target_label = format!("{}.b{}", fn_name, target.0);
                self.emitter.emit_jump(j_offset(0), &target_label);
            }
            Terminator::BranchIf {
                cond,
                then_block,
                else_block,
                ..
            } => {
                let cond_reg = ra.get(*cond);
                let then_label = format!("{}.b{}", fn_name, then_block.0);
                let else_label = format!("{}.b{}", fn_name, else_block.0);
                self.emitter
                    .emit_branch(bne(cond_reg, ZERO, 0), &then_label);
                self.emitter.emit_jump(j_offset(0), &else_label);
            }
            Terminator::ReturnError(payload, tag) => {
                let p = ra.get(*payload);
                let t = ra.get(*tag);
                if p != A0 {
                    self.emitter.emit32(mv(A0, p));
                }
                if t != A1 {
                    self.emitter.emit32(mv(A1, t));
                }
                self.emitter
                    .emit_jump(j_offset(0), &format!("{}.epilogue", fn_name));
            }
            Terminator::TailCall(name, args) => {
                for (i, arg) in args.iter().enumerate() {
                    if i < 8 {
                        let src = ra.get(*arg);
                        if src != A0 + i as u32 {
                            self.emitter.emit32(mv(A0 + i as u32, src));
                        }
                    }
                }
                // jump to target instead of call — reuses caller's return address
                self.emitter.emit_jump(j_offset(0), name);
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
    use crate::ir::lower::Lowering;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

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
        assert!(code.len() >= 20);
    }

    #[test]
    fn codegen_add() {
        let code = compile("fn add(a: u32, b: u32) u32 { let c = a + b; return c; }");
        assert!(!code.is_empty());
        let has_add = code.windows(4).any(|w| {
            let inst = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            inst & 0xFE00707F == 0x00000033
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
    }

    #[test]
    fn codegen_blink() {
        let source = std::fs::read_to_string("examples/blink.kov").unwrap();
        let tokens = Lexer::tokenize(&source).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);

        let mut cg = CodeGen::new();
        for func in &ir.functions {
            cg.gen_function(func);
        }
        let code = cg.finish().unwrap();

        assert!(!code.is_empty());
        println!(
            "blink.kov compiled to {} bytes of RISC-V machine code",
            code.len()
        );
        assert_eq!(code.len() % 4, 0);
    }

    fn compile_with_globals(src: &str) -> Vec<u8> {
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let ir = Lowering::lower(&program);

        let mut cg = CodeGen::new_with_globals(0x2000_0000, &ir.globals);
        for func in &ir.functions {
            cg.gen_function(func);
        }
        cg.finish().unwrap()
    }

    #[test]
    fn codegen_global_read() {
        let code =
            compile_with_globals("static mut counter: u32 = 0;\nfn get() u32 { return counter; }");
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
        let has_lw = code.windows(4).any(|w| {
            let inst = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            inst & 0x707F == 0x2003
        });
        assert!(has_lw, "expected LW instruction for global read");
    }

    #[test]
    fn codegen_break_in_loop() {
        let code = compile("fn f() { loop { break; } }");
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
    }

    #[test]
    fn codegen_continue_in_while() {
        let code = compile("fn f(x: u32) { while x > 0 { continue; } }");
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
    }

    #[test]
    fn codegen_global_increment_with_break() {
        let code = compile_with_globals(
            "static mut ticks: u32 = 0;\nfn f() { loop { ticks = ticks + 1; if ticks == 10 { break; } } }",
        );
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
        let has_sw = code.windows(4).any(|w| {
            let inst = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
            inst & 0x707F == 0x2023
        });
        assert!(has_sw, "expected SW instruction for global write");
    }

    #[test]
    fn codegen_match() {
        let code = compile("fn f(x: u32) u32 { match x { 0 => 10, 1 => 20, _ => 30, } }");
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
        let bne_count = code
            .windows(4)
            .filter(|w| {
                let inst = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
                inst & 0x707F == 0x1063
            })
            .count();
        assert!(
            bne_count >= 2,
            "match should emit BNE for each int pattern arm"
        );
    }

    #[test]
    fn codegen_callee_saved() {
        // function with enough values to use s-registers
        let code = compile(
            "fn f(a: u32, b: u32, c: u32, d: u32) u32 { let x = a + b; let y = c + d; let z = x + y; return z; }",
        );
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
        // prologue should save RA
        let first_inst = u32::from_le_bytes([code[0], code[1], code[2], code[3]]);
        // should be addi sp, sp, -N (negative immediate)
        assert_eq!(
            first_inst & 0x7F,
            0x13,
            "first instruction should be ADDI (sp adjust)"
        );
    }

    #[test]
    fn codegen_spill() {
        // force enough live values to exhaust all 22 registers and trigger spilling
        let code = compile(
            "fn f(a: u32, b: u32) u32 {
                let v0 = a + 1; let v1 = b + 2; let v2 = v0 + v1;
                let v3 = v2 + 3; let v4 = v3 + 4; let v5 = v4 + 5;
                let v6 = v5 + 6; let v7 = v6 + 7; let v8 = v7 + 8;
                let v9 = v8 + 9; let v10 = v9 + 10; let v11 = v10 + 11;
                let v12 = v11 + 12; let v13 = v12 + 13; let v14 = v13 + 14;
                return v0 + v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10 + v11 + v12 + v13 + v14;
            }",
        );
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
    }

    #[test]
    fn codegen_try_expression() {
        let code = compile(
            "fn read_sensor() !u32 { return 42; }\nfn f() !u32 { let x = try read_sensor(); return x; }",
        );
        assert!(!code.is_empty());
        assert_eq!(code.len() % 4, 0);
    }
}
