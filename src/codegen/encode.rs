// RISC-V RV32I instruction encoding.
// Each function packs fields into a 32-bit little-endian instruction word.

// register aliases
pub const ZERO: u32 = 0;  // x0, hardwired zero
pub const RA: u32 = 1;    // return address
pub const SP: u32 = 2;    // stack pointer
pub const GP: u32 = 3;    // global pointer
pub const TP: u32 = 4;    // thread pointer
pub const T0: u32 = 5;
pub const T1: u32 = 6;
pub const T2: u32 = 7;
pub const S0: u32 = 8;    // frame pointer
pub const S1: u32 = 9;
pub const A0: u32 = 10;   // arg 0 / return value
pub const A1: u32 = 11;
pub const A2: u32 = 12;
pub const A3: u32 = 13;
pub const A4: u32 = 14;
pub const A5: u32 = 15;
pub const A6: u32 = 16;
pub const A7: u32 = 17;

// R-type: register-register
fn r(opcode: u32, rd: u32, funct3: u32, rs1: u32, rs2: u32, funct7: u32) -> u32 {
    opcode | (rd << 7) | (funct3 << 12) | (rs1 << 15) | (rs2 << 20) | (funct7 << 25)
}

// I-type: immediate
fn i(opcode: u32, rd: u32, funct3: u32, rs1: u32, imm: i32) -> u32 {
    opcode | (rd << 7) | (funct3 << 12) | (rs1 << 15) | (((imm as u32) & 0xFFF) << 20)
}

// S-type: store
fn s(opcode: u32, funct3: u32, rs1: u32, rs2: u32, imm: i32) -> u32 {
    let imm = (imm as u32) & 0xFFF;
    opcode | ((imm & 0x1F) << 7) | (funct3 << 12) | (rs1 << 15) | (rs2 << 20) | (((imm >> 5) & 0x7F) << 25)
}

// B-type: branch (offset is byte offset from branch instruction, must be even)
fn b(opcode: u32, funct3: u32, rs1: u32, rs2: u32, imm: i32) -> u32 {
    let imm = (imm as u32) & 0x1FFE;
    let b12 = (imm >> 12) & 1;
    let b11 = (imm >> 11) & 1;
    let b10_5 = (imm >> 5) & 0x3F;
    let b4_1 = (imm >> 1) & 0xF;
    opcode | (b11 << 7) | (b4_1 << 8) | (funct3 << 12) | (rs1 << 15) | (rs2 << 20) | (b10_5 << 25) | (b12 << 31)
}

// U-type: upper immediate
fn u_type(opcode: u32, rd: u32, imm: u32) -> u32 {
    opcode | (rd << 7) | (imm & 0xFFFFF000)
}

// J-type: jump (offset is byte offset, must be even)
fn j(opcode: u32, rd: u32, imm: i32) -> u32 {
    let imm = (imm as u32) & 0x1FFFFF;
    let b20 = (imm >> 20) & 1;
    let b10_1 = (imm >> 1) & 0x3FF;
    let b11 = (imm >> 11) & 1;
    let b19_12 = (imm >> 12) & 0xFF;
    opcode | (rd << 7) | (b19_12 << 12) | (b11 << 20) | (b10_1 << 21) | (b20 << 31)
}

// ── arithmetic ──

pub fn add(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 0, rs1, rs2, 0x00) }
pub fn sub(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 0, rs1, rs2, 0x20) }
pub fn and(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 7, rs1, rs2, 0x00) }
pub fn or(rd: u32, rs1: u32, rs2: u32) -> u32  { r(0x33, rd, 6, rs1, rs2, 0x00) }
pub fn xor(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 4, rs1, rs2, 0x00) }
pub fn sll(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 1, rs1, rs2, 0x00) }
pub fn srl(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 5, rs1, rs2, 0x00) }
pub fn sra(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 5, rs1, rs2, 0x20) }
pub fn slt(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 2, rs1, rs2, 0x00) }
pub fn sltu(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 3, rs1, rs2, 0x00) }

// M extension
pub fn mul(rd: u32, rs1: u32, rs2: u32) -> u32  { r(0x33, rd, 0, rs1, rs2, 0x01) }
pub fn div(rd: u32, rs1: u32, rs2: u32) -> u32  { r(0x33, rd, 4, rs1, rs2, 0x01) }
pub fn divu(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 5, rs1, rs2, 0x01) }
pub fn rem_(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 6, rs1, rs2, 0x01) }
pub fn remu(rd: u32, rs1: u32, rs2: u32) -> u32 { r(0x33, rd, 7, rs1, rs2, 0x01) }

// ── immediate arithmetic ──

pub fn addi(rd: u32, rs1: u32, imm: i32) -> u32  { i(0x13, rd, 0, rs1, imm) }
pub fn andi(rd: u32, rs1: u32, imm: i32) -> u32  { i(0x13, rd, 7, rs1, imm) }
pub fn ori(rd: u32, rs1: u32, imm: i32) -> u32   { i(0x13, rd, 6, rs1, imm) }
pub fn xori(rd: u32, rs1: u32, imm: i32) -> u32  { i(0x13, rd, 4, rs1, imm) }
pub fn slti(rd: u32, rs1: u32, imm: i32) -> u32  { i(0x13, rd, 2, rs1, imm) }
pub fn sltiu(rd: u32, rs1: u32, imm: i32) -> u32 { i(0x13, rd, 3, rs1, imm) }
pub fn slli(rd: u32, rs1: u32, shamt: u32) -> u32 { i(0x13, rd, 1, rs1, shamt as i32) }
pub fn srli(rd: u32, rs1: u32, shamt: u32) -> u32 { i(0x13, rd, 5, rs1, shamt as i32) }
pub fn srai(rd: u32, rs1: u32, shamt: u32) -> u32 { i(0x13, rd, 5, rs1, (0x400 | shamt) as i32) }

// ── loads ──

pub fn lb(rd: u32, rs1: u32, offset: i32) -> u32  { i(0x03, rd, 0, rs1, offset) }
pub fn lh(rd: u32, rs1: u32, offset: i32) -> u32  { i(0x03, rd, 1, rs1, offset) }
pub fn lw(rd: u32, rs1: u32, offset: i32) -> u32  { i(0x03, rd, 2, rs1, offset) }
pub fn lbu(rd: u32, rs1: u32, offset: i32) -> u32 { i(0x03, rd, 4, rs1, offset) }
pub fn lhu(rd: u32, rs1: u32, offset: i32) -> u32 { i(0x03, rd, 5, rs1, offset) }

// ── stores ──

pub fn sb(rs1: u32, rs2: u32, offset: i32) -> u32 { s(0x23, 0, rs1, rs2, offset) }
pub fn sh(rs1: u32, rs2: u32, offset: i32) -> u32 { s(0x23, 1, rs1, rs2, offset) }
pub fn sw(rs1: u32, rs2: u32, offset: i32) -> u32 { s(0x23, 2, rs1, rs2, offset) }

// ── branches ──

pub fn beq(rs1: u32, rs2: u32, offset: i32) -> u32  { b(0x63, 0, rs1, rs2, offset) }
pub fn bne(rs1: u32, rs2: u32, offset: i32) -> u32  { b(0x63, 1, rs1, rs2, offset) }
pub fn blt(rs1: u32, rs2: u32, offset: i32) -> u32  { b(0x63, 4, rs1, rs2, offset) }
pub fn bge(rs1: u32, rs2: u32, offset: i32) -> u32  { b(0x63, 5, rs1, rs2, offset) }
pub fn bltu(rs1: u32, rs2: u32, offset: i32) -> u32 { b(0x63, 6, rs1, rs2, offset) }
pub fn bgeu(rs1: u32, rs2: u32, offset: i32) -> u32 { b(0x63, 7, rs1, rs2, offset) }

// ── jumps ──

pub fn jal(rd: u32, offset: i32) -> u32 { j(0x6F, rd, offset) }
pub fn jalr(rd: u32, rs1: u32, offset: i32) -> u32 { i(0x67, rd, 0, rs1, offset) }

// ── upper immediate ──

pub fn lui(rd: u32, imm: u32) -> u32   { u_type(0x37, rd, imm) }
pub fn auipc(rd: u32, imm: u32) -> u32 { u_type(0x17, rd, imm) }

// ── system ──

pub fn ecall() -> u32  { i(0x73, 0, 0, 0, 0) }
pub fn ebreak() -> u32 { i(0x73, 0, 0, 0, 1) }

// CSR instructions
pub fn csrrw(rd: u32, csr: u32, rs1: u32) -> u32  { i(0x73, rd, 1, rs1, csr as i32) }
pub fn csrrs(rd: u32, csr: u32, rs1: u32) -> u32  { i(0x73, rd, 2, rs1, csr as i32) }
pub fn csrrc(rd: u32, csr: u32, rs1: u32) -> u32  { i(0x73, rd, 3, rs1, csr as i32) }
pub fn csrrwi(rd: u32, csr: u32, imm: u32) -> u32 { i(0x73, rd, 5, imm, csr as i32) }
pub fn csrrsi(rd: u32, csr: u32, imm: u32) -> u32 { i(0x73, rd, 6, imm, csr as i32) }
pub fn csrrci(rd: u32, csr: u32, imm: u32) -> u32 { i(0x73, rd, 7, imm, csr as i32) }

pub fn wfi() -> u32 { i(0x73, 0, 0, 0, 0x105) }
pub fn mret() -> u32 { i(0x73, 0, 0, 0, 0x302) }

// ── pseudo-instructions ──

pub fn nop() -> u32 { addi(ZERO, ZERO, 0) }
pub fn mv(rd: u32, rs1: u32) -> u32 { addi(rd, rs1, 0) }
pub fn li(rd: u32, imm: i32) -> u32 { addi(rd, ZERO, imm) } // only for -2048..2047
pub fn not(rd: u32, rs1: u32) -> u32 { xori(rd, rs1, -1) }
pub fn neg(rd: u32, rs1: u32) -> u32 { sub(rd, ZERO, rs1) }
pub fn ret() -> u32 { jalr(ZERO, RA, 0) }
pub fn call_offset(offset: i32) -> u32 { jal(RA, offset) }
pub fn j_offset(offset: i32) -> u32 { jal(ZERO, offset) }

// load 32-bit immediate — returns (lui_inst, addi_inst) or just (addi_inst, nop)
pub fn li32(rd: u32, value: i32) -> (u32, Option<u32>) {
    // fits in 12-bit signed immediate
    if value >= -2048 && value < 2048 {
        return (addi(rd, ZERO, value), None);
    }

    let mut upper = ((value as u32) + 0x800) & 0xFFFFF000;
    let lower = value.wrapping_sub(upper as i32);

    (lui(rd, upper), Some(addi(rd, rd, lower)))
}

// CSR addresses
pub const MSTATUS: u32 = 0x300;
pub const MTVEC: u32 = 0x305;
pub const MEPC: u32 = 0x341;
pub const MCAUSE: u32 = 0x342;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_add() {
        // ADD x1, x2, x3 = 0x003100B3
        assert_eq!(add(1, 2, 3), 0x003100B3);
    }

    #[test]
    fn encode_addi() {
        // ADDI x1, x0, 10 = 0x00A00093
        assert_eq!(addi(1, 0, 10), 0x00A00093);
    }

    #[test]
    fn encode_sw() {
        // SW x2, 0(x1) = 0x0020A023
        assert_eq!(sw(1, 2, 0), 0x0020A023);
    }

    #[test]
    fn encode_beq() {
        // BEQ x1, x2, +8
        let inst = beq(1, 2, 8);
        // offset 8: imm[12]=0, imm[10:5]=0, imm[4:1]=0100, imm[11]=0
        assert_eq!(inst & 0x7F, 0x63); // opcode
        assert_eq!((inst >> 12) & 7, 0); // funct3 = BEQ
    }

    #[test]
    fn encode_jal() {
        // JAL x1, 0 (placeholder offset)
        let inst = jal(1, 0);
        assert_eq!(inst & 0x7F, 0x6F); // opcode
        assert_eq!((inst >> 7) & 0x1F, 1); // rd = x1
    }

    #[test]
    fn encode_lui_addi_pair() {
        // load 0x12345 into x5
        let (inst1, inst2) = li32(5, 0x12345);
        assert_eq!(inst1 & 0x7F, 0x37); // LUI opcode
        assert!(inst2.is_some());
    }

    #[test]
    fn encode_small_immediate() {
        // load 42 into x5 — should be just ADDI, no LUI
        let (inst, extra) = li32(5, 42);
        assert_eq!(inst, addi(5, 0, 42));
        assert!(extra.is_none());
    }

    #[test]
    fn encode_nop() {
        assert_eq!(nop(), addi(0, 0, 0));
    }

    #[test]
    fn encode_ret() {
        // JALR x0, x1, 0
        assert_eq!(ret(), jalr(0, 1, 0));
    }

    #[test]
    fn encode_system() {
        // ECALL = 0x00000073
        assert_eq!(ecall(), 0x00000073);
        // EBREAK = 0x00100073
        assert_eq!(ebreak(), 0x00100073);
    }
}
