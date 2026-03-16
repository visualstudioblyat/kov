use std::collections::HashMap;

const REG_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

fn r(n: u32) -> &'static str {
    REG_NAMES.get(n as usize).unwrap_or(&"?")
}

pub fn disassemble(code: &[u8], base_addr: u32, labels: &HashMap<String, usize>) -> String {
    let mut out = String::new();
    // invert labels: offset → name
    let mut addr_labels: HashMap<usize, String> = HashMap::new();
    for (name, &offset) in labels {
        addr_labels.insert(offset, name.clone());
    }

    let mut i = 0;
    while i + 3 < code.len() {
        if let Some(label) = addr_labels.get(&i) {
            out.push_str(&format!("\n{}:\n", label));
        }

        let inst = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
        let addr = base_addr + i as u32;
        let text = decode(inst, addr);
        out.push_str(&format!("  {:08x}:  {:08x}  {}\n", addr, inst, text));
        i += 4;
    }
    out
}

fn decode(inst: u32, pc: u32) -> String {
    let op = inst & 0x7F;
    let rd = (inst >> 7) & 0x1F;
    let f3 = (inst >> 12) & 0x7;
    let rs1 = (inst >> 15) & 0x1F;
    let rs2 = (inst >> 20) & 0x1F;
    let f7 = inst >> 25;

    match op {
        // LUI
        0x37 => format!("lui {}, {:#x}", r(rd), inst >> 12),
        // AUIPC
        0x17 => format!("auipc {}, {:#x}", r(rd), inst >> 12),

        // JAL
        0x6F => {
            let imm = imm_j(inst);
            let target = pc.wrapping_add(imm as u32);
            if rd == 0 {
                format!("j {:#x}", target)
            } else {
                format!("jal {}, {:#x}", r(rd), target)
            }
        }

        // JALR
        0x67 => {
            let imm = (inst as i32) >> 20;
            if rd == 0 && rs1 == 1 && imm == 0 {
                "ret".to_string()
            } else if rd == 0 {
                format!("jr {}", r(rs1))
            } else {
                format!("jalr {}, {}({})", r(rd), imm, r(rs1))
            }
        }

        // branches
        0x63 => {
            let imm = imm_b(inst);
            let target = pc.wrapping_add(imm as u32);
            let op_name = match f3 {
                0 => "beq",
                1 => "bne",
                4 => "blt",
                5 => "bge",
                6 => "bltu",
                7 => "bgeu",
                _ => "b?",
            };
            format!("{} {}, {}, {:#x}", op_name, r(rs1), r(rs2), target)
        }

        // loads
        0x03 => {
            let imm = (inst as i32) >> 20;
            let op_name = match f3 {
                0 => "lb",
                1 => "lh",
                2 => "lw",
                4 => "lbu",
                5 => "lhu",
                _ => "l?",
            };
            format!("{} {}, {}({})", op_name, r(rd), imm, r(rs1))
        }

        // stores
        0x23 => {
            let imm = imm_s(inst);
            let op_name = match f3 {
                0 => "sb",
                1 => "sh",
                2 => "sw",
                _ => "s?",
            };
            format!("{} {}, {}({})", op_name, r(rs2), imm, r(rs1))
        }

        // immediate ALU
        0x13 => {
            let imm = (inst as i32) >> 20;
            if rd == 0 && rs1 == 0 && imm == 0 {
                return "nop".to_string();
            }
            if f3 == 0 && rs1 == 0 {
                return format!("li {}, {}", r(rd), imm);
            }
            if f3 == 0 && imm == 0 {
                return format!("mv {}, {}", r(rd), r(rs1));
            }
            let op_name = match f3 {
                0 => "addi",
                1 => "slli",
                2 => "slti",
                3 => "sltiu",
                4 => "xori",
                5 => {
                    if f7 == 0x20 {
                        "srai"
                    } else {
                        "srli"
                    }
                }
                6 => "ori",
                7 => "andi",
                _ => "?",
            };
            format!("{} {}, {}, {}", op_name, r(rd), r(rs1), imm)
        }

        // register ALU
        0x33 => {
            let op_name = match (f3, f7) {
                (0, 0x00) => "add",
                (0, 0x20) => "sub",
                (1, 0x00) => "sll",
                (2, 0x00) => "slt",
                (3, 0x00) => "sltu",
                (4, 0x00) => "xor",
                (5, 0x00) => "srl",
                (5, 0x20) => "sra",
                (6, 0x00) => "or",
                (7, 0x00) => "and",
                // M extension
                (0, 0x01) => "mul",
                (1, 0x01) => "mulh",
                (2, 0x01) => "mulhsu",
                (3, 0x01) => "mulhu",
                (4, 0x01) => "div",
                (5, 0x01) => "divu",
                (6, 0x01) => "rem",
                (7, 0x01) => "remu",
                _ => "?",
            };
            format!("{} {}, {}, {}", op_name, r(rd), r(rs1), r(rs2))
        }

        // SYSTEM
        0x73 => {
            if inst == 0x00100073 {
                return "ebreak".to_string();
            }
            if inst == 0x10500073 {
                return "wfi".to_string();
            }
            let csr = (inst >> 20) & 0xFFF;
            match f3 {
                1 => format!("csrrw {}, {:#x}, {}", r(rd), csr, r(rs1)),
                2 => format!("csrrs {}, {:#x}, {}", r(rd), csr, r(rs1)),
                5 => format!("csrrwi {}, {:#x}, {}", r(rd), csr, rs1),
                6 => format!("csrrsi {}, {:#x}, {}", r(rd), csr, rs1),
                7 => format!("csrrci {}, {:#x}, {}", r(rd), csr, rs1),
                _ => format!("system {:#010x}", inst),
            }
        }

        // FENCE
        0x0F => "fence".to_string(),

        _ => format!(".word {:#010x}", inst),
    }
}

fn imm_j(inst: u32) -> i32 {
    let b19_12 = (inst >> 12) & 0xFF;
    let b11 = (inst >> 20) & 1;
    let b10_1 = (inst >> 21) & 0x3FF;
    let b20 = (inst >> 31) & 1;
    let imm = (b20 << 20) | (b19_12 << 12) | (b11 << 11) | (b10_1 << 1);
    (imm as i32) << 11 >> 11
}

fn imm_b(inst: u32) -> i32 {
    let b11 = (inst >> 7) & 1;
    let b4_1 = (inst >> 8) & 0xF;
    let b10_5 = (inst >> 25) & 0x3F;
    let b12 = (inst >> 31) & 1;
    let imm = (b12 << 12) | (b11 << 11) | (b10_5 << 5) | (b4_1 << 1);
    (imm as i32) << 19 >> 19
}

fn imm_s(inst: u32) -> i32 {
    let lo = (inst >> 7) & 0x1F;
    let hi = (inst >> 25) & 0x7F;
    (((hi << 5) | lo) as i32) << 20 >> 20
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::encode::*;

    #[test]
    fn disasm_add() {
        let inst = add(T0, A0, A1);
        let text = decode(inst, 0);
        assert!(text.contains("add") && text.contains("t0"));
    }

    #[test]
    fn disasm_ret() {
        let text = decode(ret(), 0);
        assert_eq!(text, "ret");
    }

    #[test]
    fn disasm_nop() {
        let text = decode(nop(), 0);
        assert_eq!(text, "nop");
    }

    #[test]
    fn disasm_li() {
        let text = decode(addi(T0, ZERO, 42), 0);
        assert!(text.contains("li") && text.contains("42"));
    }
}
