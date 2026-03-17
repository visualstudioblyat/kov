// ARM Cortex-M (Thumb-2) instruction encoding

pub const R0: u32 = 0;
pub const R1: u32 = 1;
pub const R2: u32 = 2;
pub const R3: u32 = 3;
pub const R4: u32 = 4;
pub const R5: u32 = 5;
pub const R6: u32 = 6;
pub const R7: u32 = 7;
pub const SP: u32 = 13;
pub const LR: u32 = 14;
pub const PC: u32 = 15;

// 16-bit Thumb instructions (returned as u16)
pub fn mov_imm8(rd: u32, imm: u32) -> u16 {
    (0x2000 | ((rd & 7) << 8) | (imm & 0xFF)) as u16
}

pub fn add_reg(rd: u32, rn: u32, rm: u32) -> u16 {
    (0x1800 | ((rm & 7) << 6) | ((rn & 7) << 3) | (rd & 7)) as u16
}

pub fn sub_reg(rd: u32, rn: u32, rm: u32) -> u16 {
    (0x1A00 | ((rm & 7) << 6) | ((rn & 7) << 3) | (rd & 7)) as u16
}

pub fn add_imm8(rd: u32, imm: u32) -> u16 {
    (0x3000 | ((rd & 7) << 8) | (imm & 0xFF)) as u16
}

pub fn sub_imm8(rd: u32, imm: u32) -> u16 {
    (0x3800 | ((rd & 7) << 8) | (imm & 0xFF)) as u16
}

pub fn ldr_sp(rd: u32, offset: u32) -> u16 {
    (0x9800 | ((rd & 7) << 8) | ((offset >> 2) & 0xFF)) as u16
}

pub fn str_sp(rd: u32, offset: u32) -> u16 {
    (0x9000 | ((rd & 7) << 8) | ((offset >> 2) & 0xFF)) as u16
}

pub fn push(regs: u32) -> u16 {
    (0xB400 | (regs & 0x1FF)) as u16
}

pub fn pop(regs: u32) -> u16 {
    (0xBC00 | (regs & 0x1FF)) as u16
}

pub fn bx(rm: u32) -> u16 {
    (0x4700 | ((rm & 0xF) << 3)) as u16
}

pub fn nop_thumb() -> u16 {
    0xBF00
}

pub fn cmp_imm8(rn: u32, imm: u32) -> u16 {
    (0x2800 | ((rn & 7) << 8) | (imm & 0xFF)) as u16
}

pub fn b_uncond(offset: i32) -> u16 {
    (0xE000 | (((offset >> 1) as u32) & 0x7FF)) as u16
}

// 32-bit Thumb-2 instructions
pub fn movw(rd: u32, imm: u32) -> u32 {
    let imm16 = imm & 0xFFFF;
    let imm4 = imm16 >> 12;
    let i = (imm16 >> 11) & 1;
    let imm3 = (imm16 >> 8) & 7;
    let imm8 = imm16 & 0xFF;
    let hi = 0xF240 | (i << 10) | imm4;
    let lo = (imm3 << 12) | (rd << 8) | imm8;
    (hi << 16) | lo
}

pub fn movt(rd: u32, imm: u32) -> u32 {
    let imm16 = imm & 0xFFFF;
    let imm4 = imm16 >> 12;
    let i = (imm16 >> 11) & 1;
    let imm3 = (imm16 >> 8) & 7;
    let imm8 = imm16 & 0xFF;
    let hi = 0xF2C0 | (i << 10) | imm4;
    let lo = (imm3 << 12) | (rd << 8) | imm8;
    (hi << 16) | lo
}

pub fn bl(offset: i32) -> u32 {
    let s = if offset < 0 { 1u32 } else { 0 };
    let imm = (offset >> 1) as u32;
    let imm10 = (imm >> 11) & 0x3FF;
    let imm11 = imm & 0x7FF;
    let j1 = ((!(imm >> 22)) ^ s) & 1;
    let j2 = ((!(imm >> 21)) ^ s) & 1;
    let hi = 0xF000 | (s << 10) | imm10;
    let lo = 0xD000 | (j1 << 13) | (j2 << 11) | imm11;
    (hi << 16) | lo
}

// load 32-bit immediate into register
pub fn li32_arm(rd: u32, val: u32) -> (u32, u32) {
    (movw(rd, val & 0xFFFF), movt(rd, val >> 16))
}

pub fn emit_arm_startup(code: &mut Vec<u8>, stack_top: u32, reset_offset: u32) {
    // vector table: SP, reset handler, then default handlers
    code.extend_from_slice(&stack_top.to_le_bytes());
    code.extend_from_slice(&(reset_offset | 1).to_le_bytes());
    for _ in 2..16 {
        code.extend_from_slice(&(reset_offset | 1).to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_mov_imm() {
        assert_eq!(mov_imm8(R0, 42), 0x202A);
    }

    #[test]
    fn encode_add_sub() {
        let a = add_reg(R0, R1, R2);
        assert_ne!(a, 0);
        let s = sub_reg(R0, R1, R2);
        assert_ne!(s, 0);
    }

    #[test]
    fn encode_push_pop() {
        let p = push((1 << R4) | (1 << R5));
        assert_ne!(p, 0);
        let q = pop((1 << R4) | (1 << R5));
        assert_ne!(q, 0);
    }

    #[test]
    fn encode_bx_lr() {
        assert_eq!(bx(LR), 0x4770);
    }

    #[test]
    fn encode_movw_movt() {
        let (lo, hi) = li32_arm(R0, 0x12345678);
        assert_ne!(lo, 0);
        assert_ne!(hi, 0);
    }

    #[test]
    fn arm_vector_table() {
        let mut code = Vec::new();
        emit_arm_startup(&mut code, 0x20008000, 0x00000040);
        assert_eq!(code.len(), 64);
        let sp = u32::from_le_bytes([code[0], code[1], code[2], code[3]]);
        assert_eq!(sp, 0x20008000);
        let reset = u32::from_le_bytes([code[4], code[5], code[6], code[7]]);
        assert_eq!(reset, 0x00000041);
    }
}
