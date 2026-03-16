use super::encode::*;

fn try_compress(inst: u32) -> Option<u16> {
    let opcode = inst & 0x7F;
    let rd = ((inst >> 7) & 0x1F) as u16;
    let rs1 = ((inst >> 15) & 0x1F) as u16;
    let rs2 = ((inst >> 20) & 0x1F) as u16;
    let funct3 = (inst >> 12) & 0x7;

    match opcode {
        // add rd, rs1, rs2
        0x33 if funct3 == 0 && (inst >> 25) == 0 => {
            if rs1 == 0 && rd != 0 && rs2 != 0 {
                return Some(0x8002 | (rd << 7) | (rs2 << 2)); // c.mv
            }
            if rd == rs1 && rd != 0 && rs2 != 0 {
                return Some(0x9002 | (rd << 7) | (rs2 << 2)); // c.add
            }
            None
        }

        // addi rd, rs1, imm
        0x13 if funct3 == 0 => {
            let imm = (inst as i32) >> 20;

            if rd == 0 && rs1 == 0 && imm == 0 {
                return Some(0x0001); // c.nop
            }
            if imm == 0 && rd != 0 && rs1 != 0 {
                return Some(0x8002 | (rd << 7) | (rs1 << 2)); // c.mv
            }
            if rs1 == 0 && rd != 0 && imm >= -32 && imm <= 31 {
                let imm = imm as u16;
                return Some(0x4001 | ((imm >> 5) & 1) << 12 | (rd << 7) | ((imm & 0x1F) << 2));
                // c.li
            }
            if rd == rs1 && rd != 0 && imm != 0 && imm >= -32 && imm <= 31 {
                let imm = imm as u16;
                return Some(0x0001 | ((imm >> 5) & 1) << 12 | (rd << 7) | ((imm & 0x1F) << 2));
                // c.addi
            }
            None
        }

        // jal x0, offset → c.j
        0x6F if rd == 0 => {
            let imm20 = ((inst >> 31) & 1) as i32;
            let imm10_1 = ((inst >> 21) & 0x3FF) as i32;
            let imm11 = ((inst >> 20) & 1) as i32;
            let imm19_12 = ((inst >> 12) & 0xFF) as i32;
            let offset = (imm20 << 20) | (imm19_12 << 12) | (imm11 << 11) | (imm10_1 << 1);
            let offset = (offset << 11) >> 11;

            if offset >= -2048 && offset <= 2046 && offset % 2 == 0 {
                let off = offset as u16;
                return Some(
                    0xA001
                        | ((off >> 11) & 1) << 12
                        | ((off >> 4) & 1) << 11
                        | ((off >> 8) & 0x3) << 9
                        | ((off >> 10) & 1) << 8
                        | ((off >> 6) & 1) << 7
                        | ((off >> 7) & 1) << 6
                        | ((off >> 1) & 0x7) << 3
                        | ((off >> 5) & 1) << 2,
                ); // c.j
            }
            None
        }

        // jalr x0, rs1, 0 → c.jr
        0x67 if rd == 0 && funct3 == 0 => {
            let imm = (inst as i32) >> 20;
            if imm == 0 && rs1 != 0 {
                return Some(0x8002 | (rs1 << 7)); // c.jr
            }
            None
        }

        _ => None,
    }
}

pub fn compress(code: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(code.len());
    let mut i = 0;

    while i + 3 < code.len() {
        let inst = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
        if let Some(c) = try_compress(inst) {
            output.extend_from_slice(&c.to_le_bytes());
        } else {
            output.extend_from_slice(&code[i..i + 4]);
        }
        i += 4;
    }
    while i < code.len() {
        output.push(code[i]);
        i += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_nop() {
        let code = 0x00000013u32.to_le_bytes();
        let result = compress(&code);
        assert_eq!(result.len(), 2);
        assert_eq!(u16::from_le_bytes([result[0], result[1]]), 0x0001);
    }

    #[test]
    fn compress_li() {
        let code = addi(T0, ZERO, 5).to_le_bytes();
        let result = compress(&code);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn compress_mv() {
        let code = addi(T0, T1, 0).to_le_bytes();
        let result = compress(&code);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn no_compress_large_imm() {
        let code = addi(T0, ZERO, 500).to_le_bytes();
        let result = compress(&code);
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn compress_ret() {
        let code = ret().to_le_bytes();
        let result = compress(&code);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn compress_mixed() {
        let mut code = Vec::new();
        code.extend_from_slice(&0x00000013u32.to_le_bytes()); // nop → 2
        code.extend_from_slice(&addi(T0, ZERO, 500).to_le_bytes()); // stays 4
        code.extend_from_slice(&ret().to_le_bytes()); // ret → 2
        let result = compress(&code);
        assert_eq!(result.len(), 8);
    }
}
