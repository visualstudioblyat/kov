// x86-64 instruction encoding
// research-informed: Agner Fog tables, Intel SDM, red team findings

// registers (encoding values)
pub const RAX: u8 = 0;
pub const RCX: u8 = 1;
pub const RDX: u8 = 2;
pub const RBX: u8 = 3;
pub const RSP: u8 = 4;
pub const RBP: u8 = 5;
pub const RSI: u8 = 6;
pub const RDI: u8 = 7;
pub const R8: u8 = 8;
pub const R9: u8 = 9;
pub const R10: u8 = 10;
pub const R11: u8 = 11;
pub const R12: u8 = 12;
pub const R13: u8 = 13;
pub const R14: u8 = 14;
pub const R15: u8 = 15;

// System V AMD64 ABI argument registers
pub const ARG_REGS: [u8; 6] = [RDI, RSI, RDX, RCX, R8, R9];
pub const RET_REG: u8 = RAX;

fn needs_rex(reg: u8) -> bool {
    reg >= 8
}

fn rex_b(reg: u8) -> u8 {
    if reg >= 8 { 1 } else { 0 }
}

fn rex_r(reg: u8) -> u8 {
    if reg >= 8 { 1 } else { 0 }
}

fn reg_lo(reg: u8) -> u8 {
    reg & 7
}

fn modrm(md: u8, reg: u8, rm: u8) -> u8 {
    (md << 6) | ((reg & 7) << 3) | (rm & 7)
}

fn rex_w(r: u8, x: u8, b: u8) -> u8 {
    0x48 | (r << 2) | (x << 1) | b
}

pub struct X86Emitter {
    pub code: Vec<u8>,
    pub labels: std::collections::HashMap<String, usize>,
    pub fixups: Vec<X86Fixup>,
}

pub struct X86Fixup {
    pub offset: usize,
    pub label: String,
    pub kind: X86FixupKind,
}

pub enum X86FixupKind {
    Rel32, // 32-bit relative (CALL, JMP, JCC)
}

impl Default for X86Emitter {
    fn default() -> Self {
        Self::new()
    }
}

impl X86Emitter {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            labels: std::collections::HashMap::new(),
            fixups: Vec::new(),
        }
    }

    pub fn label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.code.len());
    }

    pub fn pos(&self) -> usize {
        self.code.len()
    }

    pub fn emit(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    fn emit1(&mut self, b: u8) {
        self.code.push(b);
    }

    fn emit4(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    fn emit8(&mut self, v: u64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }

    // REX.W + opcode + ModRM (reg,reg)
    fn rr64(&mut self, opcode: u8, dst: u8, src: u8) {
        self.emit1(rex_w(rex_r(dst), 0, rex_b(src)));
        self.emit1(opcode);
        self.emit1(modrm(0b11, dst, src));
    }

    // === MOV ===

    pub fn mov_rr(&mut self, dst: u8, src: u8) {
        if dst == src {
            return;
        } // elide identity moves
        self.rr64(0x89, src, dst); // MOV r/m64, r64
    }

    pub fn mov_ri32(&mut self, dst: u8, imm: i32) {
        // MOV r32, imm32 (zero-extends to 64-bit)
        if needs_rex(dst) {
            self.emit1(0x41);
        }
        self.emit1(0xB8 + reg_lo(dst));
        self.emit4(imm as u32);
    }

    pub fn mov_ri64(&mut self, dst: u8, imm: i64) {
        // MOV r64, imm64 (movabs)
        self.emit1(rex_w(0, 0, rex_b(dst)));
        self.emit1(0xB8 + reg_lo(dst));
        self.emit8(imm as u64);
    }

    pub fn mov_load(&mut self, dst: u8, base: u8, offset: i32) {
        // MOV r64, [base+offset]
        self.emit1(rex_w(rex_r(dst), 0, rex_b(base)));
        self.emit1(0x8B);
        self.emit_memop(dst, base, offset);
    }

    pub fn mov_store(&mut self, base: u8, offset: i32, src: u8) {
        // MOV [base+offset], r64
        self.emit1(rex_w(rex_r(src), 0, rex_b(base)));
        self.emit1(0x89);
        self.emit_memop(src, base, offset);
    }

    fn emit_memop(&mut self, reg: u8, base: u8, offset: i32) {
        let bl = reg_lo(base);
        let rl = reg_lo(reg);

        // RSP as base needs SIB byte
        if bl == 4 {
            if offset == 0 {
                self.emit1(modrm(0b00, rl, 0b100));
                self.emit1(0x24); // SIB: base=RSP, index=none, scale=1
            } else if (-128..=127).contains(&offset) {
                self.emit1(modrm(0b01, rl, 0b100));
                self.emit1(0x24);
                self.emit1(offset as u8);
            } else {
                self.emit1(modrm(0b10, rl, 0b100));
                self.emit1(0x24);
                self.emit4(offset as u32);
            }
        }
        // RBP as base with offset=0 needs disp8=0
        else if bl == 5 && offset == 0 {
            self.emit1(modrm(0b01, rl, bl));
            self.emit1(0);
        } else if offset == 0 {
            self.emit1(modrm(0b00, rl, bl));
        } else if (-128..=127).contains(&offset) {
            self.emit1(modrm(0b01, rl, bl));
            self.emit1(offset as u8);
        } else {
            self.emit1(modrm(0b10, rl, bl));
            self.emit4(offset as u32);
        }
    }

    // === ALU ===

    pub fn add_rr(&mut self, dst: u8, src: u8) {
        self.rr64(0x01, src, dst);
    }

    pub fn sub_rr(&mut self, dst: u8, src: u8) {
        self.rr64(0x29, src, dst);
    }

    pub fn imul_rr(&mut self, dst: u8, src: u8) {
        self.emit1(rex_w(rex_r(dst), 0, rex_b(src)));
        self.emit(&[0x0F, 0xAF]);
        self.emit1(modrm(0b11, dst, src));
    }

    pub fn and_rr(&mut self, dst: u8, src: u8) {
        self.rr64(0x21, src, dst);
    }

    pub fn or_rr(&mut self, dst: u8, src: u8) {
        self.rr64(0x09, src, dst);
    }

    pub fn xor_rr(&mut self, dst: u8, src: u8) {
        self.rr64(0x31, src, dst);
    }

    // xor eax,eax — zero idiom, 2 bytes, breaks dependency chain
    pub fn zero_reg(&mut self, reg: u8) {
        if needs_rex(reg) {
            self.emit1(0x41 | rex_b(reg));
        }
        self.emit1(0x31);
        self.emit1(modrm(0b11, reg_lo(reg), reg_lo(reg)));
    }

    pub fn add_ri32(&mut self, dst: u8, imm: i32) {
        if (-128..=127).contains(&imm) {
            // ADD r/m64, imm8 (sign-extended)
            self.emit1(rex_w(0, 0, rex_b(dst)));
            self.emit1(0x83);
            self.emit1(modrm(0b11, 0, dst));
            self.emit1(imm as u8);
        } else {
            self.emit1(rex_w(0, 0, rex_b(dst)));
            self.emit1(0x81);
            self.emit1(modrm(0b11, 0, dst));
            self.emit4(imm as u32);
        }
    }

    pub fn sub_ri32(&mut self, dst: u8, imm: i32) {
        if (-128..=127).contains(&imm) {
            self.emit1(rex_w(0, 0, rex_b(dst)));
            self.emit1(0x83);
            self.emit1(modrm(0b11, 5, dst));
            self.emit1(imm as u8);
        } else {
            self.emit1(rex_w(0, 0, rex_b(dst)));
            self.emit1(0x81);
            self.emit1(modrm(0b11, 5, dst));
            self.emit4(imm as u32);
        }
    }

    // === SHIFT ===

    pub fn shl_ri(&mut self, dst: u8, imm: u8) {
        self.emit1(rex_w(0, 0, rex_b(dst)));
        self.emit1(0xC1);
        self.emit1(modrm(0b11, 4, dst));
        self.emit1(imm);
    }

    pub fn shr_ri(&mut self, dst: u8, imm: u8) {
        self.emit1(rex_w(0, 0, rex_b(dst)));
        self.emit1(0xC1);
        self.emit1(modrm(0b11, 5, dst));
        self.emit1(imm);
    }

    // === COMPARE ===

    // emit CMP+JCC adjacent for macro-fusion
    pub fn cmp_rr(&mut self, a: u8, b: u8) {
        self.rr64(0x39, b, a);
    }

    pub fn cmp_ri32(&mut self, reg: u8, imm: i32) {
        if (-128..=127).contains(&imm) {
            self.emit1(rex_w(0, 0, rex_b(reg)));
            self.emit1(0x83);
            self.emit1(modrm(0b11, 7, reg));
            self.emit1(imm as u8);
        } else {
            self.emit1(rex_w(0, 0, rex_b(reg)));
            self.emit1(0x81);
            self.emit1(modrm(0b11, 7, reg));
            self.emit4(imm as u32);
        }
    }

    pub fn test_rr(&mut self, a: u8, b: u8) {
        self.rr64(0x85, b, a);
    }

    // === JUMPS + CALLS ===

    pub fn jmp(&mut self, label: &str) {
        self.emit1(0xE9);
        self.fixups.push(X86Fixup {
            offset: self.code.len(),
            label: label.to_string(),
            kind: X86FixupKind::Rel32,
        });
        self.emit4(0); // placeholder
    }

    pub fn jcc(&mut self, cc: u8, label: &str) {
        self.emit(&[0x0F, 0x80 + cc]);
        self.fixups.push(X86Fixup {
            offset: self.code.len(),
            label: label.to_string(),
            kind: X86FixupKind::Rel32,
        });
        self.emit4(0);
    }

    pub fn je(&mut self, label: &str) {
        self.jcc(0x04, label);
    }
    pub fn jne(&mut self, label: &str) {
        self.jcc(0x05, label);
    }
    pub fn jl(&mut self, label: &str) {
        self.jcc(0x0C, label);
    }
    pub fn jge(&mut self, label: &str) {
        self.jcc(0x0D, label);
    }
    pub fn jle(&mut self, label: &str) {
        self.jcc(0x0E, label);
    }
    pub fn jg(&mut self, label: &str) {
        self.jcc(0x0F, label);
    }
    pub fn jb(&mut self, label: &str) {
        self.jcc(0x02, label);
    }
    pub fn jae(&mut self, label: &str) {
        self.jcc(0x03, label);
    }

    pub fn call(&mut self, label: &str) {
        self.emit1(0xE8);
        self.fixups.push(X86Fixup {
            offset: self.code.len(),
            label: label.to_string(),
            kind: X86FixupKind::Rel32,
        });
        self.emit4(0);
    }

    pub fn ret(&mut self) {
        self.emit1(0xC3);
    }

    pub fn push(&mut self, reg: u8) {
        if needs_rex(reg) {
            self.emit1(0x41);
        }
        self.emit1(0x50 + reg_lo(reg));
    }

    pub fn pop(&mut self, reg: u8) {
        if needs_rex(reg) {
            self.emit1(0x41);
        }
        self.emit1(0x58 + reg_lo(reg));
    }

    pub fn nop(&mut self) {
        self.emit1(0x90);
    }

    pub fn syscall(&mut self) {
        self.emit(&[0x0F, 0x05]);
    }

    // === NEG ===

    pub fn neg(&mut self, reg: u8) {
        self.emit1(rex_w(0, 0, rex_b(reg)));
        self.emit1(0xF7);
        self.emit1(modrm(0b11, 3, reg));
    }

    pub fn not(&mut self, reg: u8) {
        self.emit1(rex_w(0, 0, rex_b(reg)));
        self.emit1(0xF7);
        self.emit1(modrm(0b11, 2, reg));
    }

    // === SETCC (for comparisons returning bool) ===

    pub fn sete(&mut self, dst: u8) {
        if needs_rex(dst) {
            self.emit1(0x41);
        }
        self.emit(&[0x0F, 0x94]);
        self.emit1(modrm(0b11, 0, reg_lo(dst)));
    }

    pub fn setne(&mut self, dst: u8) {
        if needs_rex(dst) {
            self.emit1(0x41);
        }
        self.emit(&[0x0F, 0x95]);
        self.emit1(modrm(0b11, 0, reg_lo(dst)));
    }

    pub fn setl(&mut self, dst: u8) {
        if needs_rex(dst) {
            self.emit1(0x41);
        }
        self.emit(&[0x0F, 0x9C]);
        self.emit1(modrm(0b11, 0, reg_lo(dst)));
    }

    pub fn setge(&mut self, dst: u8) {
        if needs_rex(dst) {
            self.emit1(0x41);
        }
        self.emit(&[0x0F, 0x9D]);
        self.emit1(modrm(0b11, 0, reg_lo(dst)));
    }

    // === MOVZX (zero extend byte to 64-bit) ===

    pub fn movzx_r8(&mut self, dst: u8, src: u8) {
        self.emit1(rex_w(rex_r(dst), 0, rex_b(src)));
        self.emit(&[0x0F, 0xB6]);
        self.emit1(modrm(0b11, dst, src));
    }

    // === RESOLVE FIXUPS ===

    pub fn resolve(&mut self) -> Result<(), String> {
        for fixup in &self.fixups {
            let target = match self.labels.get(&fixup.label) {
                Some(&addr) => addr,
                None => continue, // external symbol, leave for linker
            };
            let offset = fixup.offset;
            let rel = (target as i64 - (offset as i64 + 4)) as i32;
            self.code[offset..offset + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_mov_rr() {
        let mut e = X86Emitter::new();
        e.mov_rr(RAX, RCX);
        // REX.W MOV r/m64, r64: 48 89 C8
        assert_eq!(&e.code, &[0x48, 0x89, 0xC8]);
    }

    #[test]
    fn encode_mov_ri32() {
        let mut e = X86Emitter::new();
        e.mov_ri32(RAX, 42);
        assert_eq!(e.code[0], 0xB8); // MOV eax, imm32
        assert_eq!(
            u32::from_le_bytes([e.code[1], e.code[2], e.code[3], e.code[4]]),
            42
        );
    }

    #[test]
    fn encode_zero_idiom() {
        let mut e = X86Emitter::new();
        e.zero_reg(RAX);
        // xor eax, eax: 31 C0 (2 bytes, no REX needed)
        assert_eq!(&e.code, &[0x31, 0xC0]);
    }

    #[test]
    fn encode_add_rr() {
        let mut e = X86Emitter::new();
        e.add_rr(RAX, RCX);
        assert_eq!(&e.code, &[0x48, 0x01, 0xC8]);
    }

    #[test]
    fn encode_push_pop() {
        let mut e = X86Emitter::new();
        e.push(RBP);
        e.pop(RBP);
        assert_eq!(&e.code, &[0x55, 0x5D]);
    }

    #[test]
    fn encode_push_r15() {
        let mut e = X86Emitter::new();
        e.push(R15);
        // REX.B PUSH r15: 41 57
        assert_eq!(&e.code, &[0x41, 0x57]);
    }

    #[test]
    fn encode_ret() {
        let mut e = X86Emitter::new();
        e.ret();
        assert_eq!(&e.code, &[0xC3]);
    }

    #[test]
    fn encode_sub_ri_short() {
        let mut e = X86Emitter::new();
        e.sub_ri32(RSP, 16);
        // REX.W SUB r/m64, imm8: 48 83 EC 10
        assert_eq!(&e.code, &[0x48, 0x83, 0xEC, 0x10]);
    }

    #[test]
    fn encode_call_and_resolve() {
        let mut e = X86Emitter::new();
        e.call("puts");
        e.label("puts");
        e.ret();
        e.resolve().unwrap();
        // CALL should have rel32 = 1 (skip the 4-byte immediate + this instruction)
        let rel = i32::from_le_bytes([e.code[1], e.code[2], e.code[3], e.code[4]]);
        assert_eq!(rel, 0); // target is immediately after CALL (offset 5, fixup at 1, rel = 5-(1+4) = 0)
    }

    #[test]
    fn encode_cmp_jcc_adjacent() {
        // macro-fusion: CMP + JCC must be adjacent
        let mut e = X86Emitter::new();
        e.cmp_rr(RAX, RCX);
        e.je("target");
        e.label("target");
        e.ret();
        e.resolve().unwrap();
        // CMP is 3 bytes, JE is 6 bytes, target is at offset 9
        assert!(e.code.len() >= 9);
    }

    #[test]
    fn identity_mov_elided() {
        let mut e = X86Emitter::new();
        e.mov_rr(RAX, RAX);
        assert!(e.code.is_empty(), "mov rax,rax should be elided");
    }

    #[test]
    fn extended_registers() {
        let mut e = X86Emitter::new();
        e.mov_rr(R8, R9);
        // needs REX: 4D 89 C8
        assert_eq!(e.code[0] & 0xF0, 0x40, "should have REX prefix");
    }
}
