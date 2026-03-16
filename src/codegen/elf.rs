pub struct ElfWriter {
    pub entry: u32,
    pub text_base: u32,
}

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const EM_RISCV: u16 = 243;
const EHDR_SIZE: u16 = 52;
const PHDR_SIZE: u16 = 32;
const PT_LOAD: u32 = 1;
const PF_RX: u32 = 5;

impl ElfWriter {
    pub fn new(text_base: u32, entry: u32) -> Self {
        Self { entry, text_base }
    }

    pub fn write(&self, code: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        let code_offset = EHDR_SIZE as u32 + PHDR_SIZE as u32;
        let code_size = code.len() as u32;

        // ELF header
        buf.extend_from_slice(&ELF_MAGIC);
        buf.push(ELFCLASS32);
        buf.push(ELFDATA2LSB);
        buf.push(1); // version
        buf.extend_from_slice(&[0; 9]); // OS/ABI + padding
        buf.extend_from_slice(&ET_EXEC.to_le_bytes());
        buf.extend_from_slice(&EM_RISCV.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());
        buf.extend_from_slice(&self.entry.to_le_bytes());
        buf.extend_from_slice(&(EHDR_SIZE as u32).to_le_bytes()); // phoff
        buf.extend_from_slice(&0u32.to_le_bytes()); // shoff
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&EHDR_SIZE.to_le_bytes());
        buf.extend_from_slice(&PHDR_SIZE.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // phnum
        buf.extend_from_slice(&0u16.to_le_bytes()); // shentsize
        buf.extend_from_slice(&0u16.to_le_bytes()); // shnum
        buf.extend_from_slice(&0u16.to_le_bytes()); // shstrndx

        // program header — single PT_LOAD for .text
        buf.extend_from_slice(&PT_LOAD.to_le_bytes());
        buf.extend_from_slice(&code_offset.to_le_bytes());
        buf.extend_from_slice(&self.text_base.to_le_bytes()); // vaddr
        buf.extend_from_slice(&self.text_base.to_le_bytes()); // paddr
        buf.extend_from_slice(&code_size.to_le_bytes());
        buf.extend_from_slice(&code_size.to_le_bytes());
        buf.extend_from_slice(&PF_RX.to_le_bytes());
        buf.extend_from_slice(&4u32.to_le_bytes()); // align

        buf.extend_from_slice(code);
        buf
    }

    pub fn write_flat(code: &[u8]) -> Vec<u8> {
        code.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_elf_header() {
        let elf = ElfWriter::new(0x0800_0000, 0x0800_0000).write(&[0x13, 0x00, 0x00, 0x00]);

        assert_eq!(&elf[0..4], &ELF_MAGIC);
        assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), EM_RISCV);
        assert_eq!(
            u32::from_le_bytes([elf[24], elf[25], elf[26], elf[27]]),
            0x0800_0000
        );
    }

    #[test]
    fn code_at_correct_offset() {
        let code = vec![0x33, 0x01, 0x00, 0x00];
        let elf = ElfWriter::new(0x0800_0000, 0x0800_0000).write(&code);
        let off = (EHDR_SIZE + PHDR_SIZE) as usize;
        assert_eq!(&elf[off..off + 4], &code);
    }

    #[test]
    fn flat_is_just_code() {
        let code = vec![0x13, 0x00, 0x00, 0x00];
        assert_eq!(ElfWriter::write_flat(&code), code);
    }
}
