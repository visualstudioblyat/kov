use std::collections::HashMap;

pub struct Emitter {
    pub code: Vec<u8>,
    pub labels: HashMap<String, usize>,
    pub fixups: Vec<Fixup>,
}

pub struct Fixup {
    pub offset: usize,
    pub label: String,
    pub kind: FixupKind,
}

#[derive(Copy, Clone)]
pub enum FixupKind {
    Branch, // B-type, ±4KB
    Jump,   // J-type, ±1MB
}

impl Emitter {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            labels: HashMap::new(),
            fixups: Vec::new(),
        }
    }

    pub fn pos(&self) -> u32 {
        self.code.len() as u32
    }

    pub fn emit32(&mut self, inst: u32) {
        self.code.extend_from_slice(&inst.to_le_bytes());
    }

    pub fn label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.code.len());
    }

    // emit a branch with placeholder offset, fix up later
    pub fn emit_branch(&mut self, base_inst: u32, target: &str) {
        self.fixups.push(Fixup {
            offset: self.pos() as usize,
            label: target.to_string(),
            kind: FixupKind::Branch,
        });
        self.emit32(base_inst);
    }

    // emit a jump with placeholder offset, fix up later
    pub fn emit_jump(&mut self, base_inst: u32, target: &str) {
        self.fixups.push(Fixup {
            offset: self.pos() as usize,
            label: target.to_string(),
            kind: FixupKind::Jump,
        });
        self.emit32(base_inst);
    }

    pub fn resolve(&mut self) -> Result<(), String> {
        for fixup in &self.fixups {
            let target_addr = match self.labels.get(&fixup.label) {
                Some(&addr) => addr,
                None => {
                    // unresolved external → NOP so it doesn't self-loop
                    let idx = fixup.offset;
                    self.code[idx..idx + 4].copy_from_slice(&0x00000013u32.to_le_bytes());
                    continue;
                }
            };
            let offset = target_addr as i32 - fixup.offset as i32;
            let idx = fixup.offset;

            // read the existing instruction
            let mut inst = u32::from_le_bytes([
                self.code[idx],
                self.code[idx + 1],
                self.code[idx + 2],
                self.code[idx + 3],
            ]);

            match fixup.kind {
                FixupKind::Branch => {
                    if !(-4096..=4095).contains(&offset) {
                        return Err(format!(
                            "branch to {} out of range: {}",
                            fixup.label, offset
                        ));
                    }
                    // patch B-type immediate bits
                    inst = patch_b_imm(inst, offset);
                }
                FixupKind::Jump => {
                    if !(-1048576..=1048575).contains(&offset) {
                        return Err(format!("jump to {} out of range: {}", fixup.label, offset));
                    }
                    // patch J-type immediate bits
                    inst = patch_j_imm(inst, offset);
                }
            }

            let bytes = inst.to_le_bytes();
            self.code[idx..idx + 4].copy_from_slice(&bytes);
        }
        Ok(())
    }
}

// patch B-type immediate into an existing instruction (preserving opcode/regs/funct3)
fn patch_b_imm(inst: u32, offset: i32) -> u32 {
    let imm = (offset as u32) & 0x1FFE;
    let b12 = (imm >> 12) & 1;
    let b11 = (imm >> 11) & 1;
    let b10_5 = (imm >> 5) & 0x3F;
    let b4_1 = (imm >> 1) & 0xF;

    // clear existing immediate bits, keep opcode/funct3/rs1/rs2
    let base = inst & 0x01FFF07F;
    base | (b11 << 7) | (b4_1 << 8) | (b10_5 << 25) | (b12 << 31)
}

// patch J-type immediate into an existing instruction (preserving opcode/rd)
fn patch_j_imm(inst: u32, offset: i32) -> u32 {
    let imm = (offset as u32) & 0x1FFFFF;
    let b20 = (imm >> 20) & 1;
    let b10_1 = (imm >> 1) & 0x3FF;
    let b11 = (imm >> 11) & 1;
    let b19_12 = (imm >> 12) & 0xFF;

    let base = inst & 0x00000FFF; // keep opcode + rd
    base | (b19_12 << 12) | (b11 << 20) | (b10_1 << 21) | (b20 << 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::encode;

    #[test]
    fn emit_and_resolve_jump() {
        let mut e = Emitter::new();
        e.emit_jump(encode::jal(encode::ZERO, 0), "target");
        e.emit32(encode::nop());
        e.emit32(encode::nop());
        e.label("target");
        e.emit32(encode::nop());

        e.resolve().unwrap();

        // jump should be at offset 0, target at offset 12
        // so offset = 12
        let inst = u32::from_le_bytes([e.code[0], e.code[1], e.code[2], e.code[3]]);
        assert_eq!(inst & 0x7F, 0x6F); // JAL opcode preserved
    }

    #[test]
    fn emit_and_resolve_branch() {
        let mut e = Emitter::new();
        e.label("loop");
        e.emit32(encode::nop());
        e.emit_branch(encode::beq(1, 0, 0), "loop");

        e.resolve().unwrap();

        // branch at offset 4, target at offset 0, so offset = -4
        let inst = u32::from_le_bytes([e.code[4], e.code[5], e.code[6], e.code[7]]);
        assert_eq!(inst & 0x7F, 0x63); // BRANCH opcode preserved
    }

    #[test]
    fn forward_and_backward_refs() {
        let mut e = Emitter::new();

        // forward reference
        e.emit_jump(encode::j_offset(0), "end");

        e.label("start");
        e.emit32(encode::addi(1, 0, 42));

        // backward reference
        e.emit_branch(encode::beq(1, 0, 0), "start");

        e.label("end");
        e.emit32(encode::ret());

        e.resolve().unwrap();
        assert_eq!(e.code.len(), 16); // 4 instructions
    }
}
