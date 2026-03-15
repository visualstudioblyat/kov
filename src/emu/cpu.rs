use super::memory::Memory;

pub struct Cpu {
    pub regs: [u32; 32],
    pub pc: u32,
    pub mem: Memory,
    pub halted: bool,
    pub cycles: u64,
}

// instruction fields
fn opcode(inst: u32) -> u32 { inst & 0x7F }
fn rd(inst: u32) -> usize { ((inst >> 7) & 0x1F) as usize }
fn funct3(inst: u32) -> u32 { (inst >> 12) & 7 }
fn rs1(inst: u32) -> usize { ((inst >> 15) & 0x1F) as usize }
fn rs2(inst: u32) -> usize { ((inst >> 20) & 0x1F) as usize }
fn funct7(inst: u32) -> u32 { inst >> 25 }

// immediate extraction
fn imm_i(inst: u32) -> i32 { (inst as i32) >> 20 }
fn imm_s(inst: u32) -> i32 {
    let lo = (inst >> 7) & 0x1F;
    let hi = (inst >> 25) & 0x7F;
    (((hi << 5) | lo) as i32) << 20 >> 20
}
fn imm_b(inst: u32) -> i32 {
    let b11 = (inst >> 7) & 1;
    let b4_1 = (inst >> 8) & 0xF;
    let b10_5 = (inst >> 25) & 0x3F;
    let b12 = (inst >> 31) & 1;
    let imm = (b12 << 12) | (b11 << 11) | (b10_5 << 5) | (b4_1 << 1);
    (imm as i32) << 19 >> 19
}
fn imm_u(inst: u32) -> u32 { inst & 0xFFFFF000 }
fn imm_j(inst: u32) -> i32 {
    let b19_12 = (inst >> 12) & 0xFF;
    let b11 = (inst >> 20) & 1;
    let b10_1 = (inst >> 21) & 0x3FF;
    let b20 = (inst >> 31) & 1;
    let imm = (b20 << 20) | (b19_12 << 12) | (b11 << 11) | (b10_1 << 1);
    (imm as i32) << 11 >> 11
}

impl Cpu {
    pub fn new(entry: u32) -> Self {
        Self {
            regs: [0u32; 32],
            pc: entry,
            mem: Memory::new(),
            halted: false,
            cycles: 0,
        }
    }

    fn reg(&self, r: usize) -> u32 { self.regs[r] }

    fn set_reg(&mut self, r: usize, val: u32) {
        if r != 0 { self.regs[r] = val; } // x0 always zero
    }

    pub fn step(&mut self) -> bool {
        if self.halted { return false; }

        let inst = self.mem.read32(self.pc);
        if inst == 0 { self.halted = true; return false; }

        let mut next_pc = self.pc.wrapping_add(4);
        self.cycles += 1;

        match opcode(inst) {
            // LUI
            0x37 => self.set_reg(rd(inst), imm_u(inst)),

            // AUIPC
            0x17 => self.set_reg(rd(inst), self.pc.wrapping_add(imm_u(inst))),

            // JAL
            0x6F => {
                self.set_reg(rd(inst), next_pc);
                next_pc = self.pc.wrapping_add(imm_j(inst) as u32);
            }

            // JALR
            0x67 => {
                let target = self.reg(rs1(inst)).wrapping_add(imm_i(inst) as u32) & !1;
                self.set_reg(rd(inst), next_pc);
                next_pc = target;
            }

            // branches
            0x63 => {
                let a = self.reg(rs1(inst));
                let b = self.reg(rs2(inst));
                let taken = match funct3(inst) {
                    0 => a == b,                               // BEQ
                    1 => a != b,                               // BNE
                    4 => (a as i32) < (b as i32),              // BLT
                    5 => (a as i32) >= (b as i32),             // BGE
                    6 => a < b,                                // BLTU
                    7 => a >= b,                                // BGEU
                    _ => false,
                };
                if taken { next_pc = self.pc.wrapping_add(imm_b(inst) as u32); }
            }

            // loads
            0x03 => {
                let addr = self.reg(rs1(inst)).wrapping_add(imm_i(inst) as u32);
                let val = match funct3(inst) {
                    0 => self.mem.read8(addr) as i8 as i32 as u32,  // LB
                    1 => self.mem.read16(addr) as i16 as i32 as u32, // LH
                    2 => self.mem.read32(addr),                      // LW
                    4 => self.mem.read8(addr) as u32,                // LBU
                    5 => self.mem.read16(addr) as u32,               // LHU
                    _ => 0,
                };
                self.set_reg(rd(inst), val);
            }

            // stores
            0x23 => {
                let addr = self.reg(rs1(inst)).wrapping_add(imm_s(inst) as u32);
                let val = self.reg(rs2(inst));
                match funct3(inst) {
                    0 => self.mem.write8(addr, val as u8),
                    1 => self.mem.write16(addr, val as u16),
                    2 => self.mem.write32(addr, val),
                    _ => {}
                }
            }

            // immediate arithmetic
            0x13 => {
                let src = self.reg(rs1(inst));
                let imm = imm_i(inst);
                let val = match funct3(inst) {
                    0 => src.wrapping_add(imm as u32),              // ADDI
                    1 => src << (imm & 0x1F),                       // SLLI
                    2 => ((src as i32) < imm) as u32,               // SLTI
                    3 => (src < (imm as u32)) as u32,               // SLTIU
                    4 => src ^ (imm as u32),                        // XORI
                    5 => {
                        let shamt = (imm & 0x1F) as u32;
                        if (inst >> 30) & 1 == 1 {
                            ((src as i32) >> shamt) as u32           // SRAI
                        } else {
                            src >> shamt                              // SRLI
                        }
                    }
                    6 => src | (imm as u32),                        // ORI
                    7 => src & (imm as u32),                        // ANDI
                    _ => 0,
                };
                self.set_reg(rd(inst), val);
            }

            // register arithmetic
            0x33 => {
                let a = self.reg(rs1(inst));
                let b = self.reg(rs2(inst));
                let val = match (funct3(inst), funct7(inst)) {
                    (0, 0x00) => a.wrapping_add(b),                 // ADD
                    (0, 0x20) => a.wrapping_sub(b),                 // SUB
                    (1, 0x00) => a << (b & 0x1F),                   // SLL
                    (2, 0x00) => ((a as i32) < (b as i32)) as u32,  // SLT
                    (3, 0x00) => (a < b) as u32,                    // SLTU
                    (4, 0x00) => a ^ b,                             // XOR
                    (5, 0x00) => a >> (b & 0x1F),                   // SRL
                    (5, 0x20) => ((a as i32) >> (b & 0x1F)) as u32, // SRA
                    (6, 0x00) => a | b,                             // OR
                    (7, 0x00) => a & b,                             // AND
                    // M extension
                    (0, 0x01) => a.wrapping_mul(b),                 // MUL
                    (4, 0x01) => {                                   // DIV
                        if b == 0 { u32::MAX } else { ((a as i32).wrapping_div(b as i32)) as u32 }
                    }
                    (5, 0x01) => {                                   // DIVU
                        if b == 0 { u32::MAX } else { a / b }
                    }
                    (6, 0x01) => {                                   // REM
                        if b == 0 { a } else { ((a as i32).wrapping_rem(b as i32)) as u32 }
                    }
                    (7, 0x01) => {                                   // REMU
                        if b == 0 { a } else { a % b }
                    }
                    _ => 0,
                };
                self.set_reg(rd(inst), val);
            }

            // SYSTEM
            0x73 => {
                let imm = imm_i(inst) as u32;
                match imm {
                    0x000 => {} // ECALL — no-op in emulator
                    0x001 => { self.halted = true; } // EBREAK → halt
                    0x105 => { self.halted = true; } // WFI → halt
                    0x302 => { self.halted = true; } // MRET → halt (no interrupt support yet)
                    _ => {} // CSR ops — ignored for now
                }
            }

            _ => {
                // unknown opcode — halt
                self.halted = true;
                return false;
            }
        }

        self.pc = next_pc;
        true
    }

    // run until halted or max cycles
    pub fn run(&mut self, max_cycles: u64) {
        while self.cycles < max_cycles && self.step() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::encode as rv;

    fn run_code(instructions: &[u32]) -> Cpu {
        let entry = 0x0800_0000;
        let mut cpu = Cpu::new(entry);
        let code: Vec<u8> = instructions.iter().flat_map(|i| i.to_le_bytes()).collect();
        cpu.mem.load_flash(&code);
        cpu.run(1000);
        cpu
    }

    #[test]
    fn addi() {
        let cpu = run_code(&[
            rv::addi(1, 0, 42),  // x1 = 42
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[1], 42);
    }

    #[test]
    fn add_sub() {
        let cpu = run_code(&[
            rv::addi(1, 0, 10),   // x1 = 10
            rv::addi(2, 0, 20),   // x2 = 20
            rv::add(3, 1, 2),     // x3 = 30
            rv::sub(4, 2, 1),     // x4 = 10
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 30);
        assert_eq!(cpu.regs[4], 10);
    }

    #[test]
    fn lui_addi() {
        let cpu = run_code(&[
            rv::lui(1, 0x12345000),     // x1 = 0x12345000
            rv::addi(1, 1, 0x678),      // x1 = 0x12345678
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[1], 0x12345678);
    }

    #[test]
    fn branch_taken() {
        let cpu = run_code(&[
            rv::addi(1, 0, 5),     // x1 = 5
            rv::addi(2, 0, 5),     // x2 = 5
            rv::beq(1, 2, 8),      // if x1 == x2, skip next
            rv::addi(3, 0, 99),    // x3 = 99 (should be skipped)
            rv::addi(4, 0, 42),    // x4 = 42
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 0);  // skipped
        assert_eq!(cpu.regs[4], 42); // reached
    }

    #[test]
    fn branch_not_taken() {
        let cpu = run_code(&[
            rv::addi(1, 0, 5),
            rv::addi(2, 0, 10),
            rv::beq(1, 2, 8),     // not taken
            rv::addi(3, 0, 99),   // executed
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 99);
    }

    #[test]
    fn jal_and_ret() {
        let cpu = run_code(&[
            rv::jal(1, 8),         // jump forward 8 bytes (skip 1 inst), save return in x1
            rv::addi(3, 0, 99),    // skipped
            rv::addi(4, 0, 42),    // landed here
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 0);
        assert_eq!(cpu.regs[4], 42);
        assert_eq!(cpu.regs[1], 0x0800_0004); // return address
    }

    #[test]
    fn load_store() {
        let cpu = run_code(&[
            // store 0xDEAD to RAM
            rv::lui(1, 0x2000_0000),   // x1 = RAM base
            rv::addi(2, 0, 0x55),      // x2 = 0x55
            rv::sw(1, 2, 0),           // mem[x1] = x2
            rv::lw(3, 1, 0),           // x3 = mem[x1]
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 0x55);
    }

    #[test]
    fn mmio_write_logged() {
        let cpu = run_code(&[
            rv::lui(1, 0x6000_4000),   // x1 = GPIO base
            rv::addi(1, 1, 4),         // x1 = GPIO_BASE + OUTPUT_SET
            rv::addi(2, 0, 4),         // x2 = 1 << 2 (pin 2)
            rv::sw(1, 2, 0),           // write to GPIO
            rv::ebreak(),
        ]);
        assert!(!cpu.mem.mmio_log.is_empty());
        let access = &cpu.mem.mmio_log[0];
        assert_eq!(access.address, 0x6000_4004);
        assert_eq!(access.value, 4);
        assert!(access.is_write);
    }

    #[test]
    fn multiply() {
        let cpu = run_code(&[
            rv::addi(1, 0, 7),
            rv::addi(2, 0, 6),
            rv::mul(3, 1, 2),
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[3], 42);
    }

    #[test]
    fn loop_counts() {
        // count from 0 to 5
        let cpu = run_code(&[
            rv::addi(1, 0, 0),       // x1 = counter = 0
            rv::addi(2, 0, 5),       // x2 = limit = 5
            // loop:
            rv::addi(1, 1, 1),       // x1 += 1
            rv::blt(1, 2, -4i32 as u32 as i32), // if x1 < x2, jump back
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[1], 5);
    }

    #[test]
    fn x0_always_zero() {
        let cpu = run_code(&[
            rv::addi(0, 0, 42),  // try to write to x0
            rv::ebreak(),
        ]);
        assert_eq!(cpu.regs[0], 0); // x0 unchanged
    }

    #[test]
    fn run_compiled_blink() {
        let source = std::fs::read_to_string("examples/blink.kv").unwrap();
        let tokens = crate::lexer::Lexer::tokenize(&source).unwrap();
        let program = crate::parser::Parser::new(tokens).parse().unwrap();
        let ir = crate::ir::lower::Lowering::lower(&program);

        let mut cg = crate::codegen::CodeGen::new();
        let board = crate::codegen::startup::BoardConfig::from_name("gd32vf103").unwrap();
        crate::codegen::startup::emit_startup(&mut cg.emitter, &board);
        for func in &ir.functions {
            cg.gen_function(func);
        }
        let code = cg.finish().unwrap();

        let mut cpu = Cpu::new(0x0800_0000);
        cpu.mem.load_flash(&code);
        cpu.regs[2] = 0x2000_8000;
        cpu.run(2000);

        let gpio_writes: Vec<_> = cpu.mem.mmio_log.iter()
            .filter(|a| a.is_write && a.address >= 0x6000_4000 && a.address < 0x6000_5000)
            .collect();

        assert!(!gpio_writes.is_empty(), "expected GPIO register writes from blink program");
    }
}
