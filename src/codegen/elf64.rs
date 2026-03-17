// ELF64 relocatable object file output
// produces .o files that cc/ld can link against libc

pub struct Elf64Writer {
    pub code: Vec<u8>,
    pub data: Vec<u8>,
    pub rodata: Vec<u8>,
    pub symbols: Vec<Elf64Sym>,
    pub relocations: Vec<Elf64Rela>,
    pub strtab: Vec<u8>,
}

pub struct Elf64Sym {
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub section: u16, // 1=.text, 2=.data, 3=.rodata, 0=external
    pub bind: u8,     // 0=local, 1=global
    pub typ: u8,      // 0=notype, 2=func
}

pub struct Elf64Rela {
    pub offset: u64,    // offset in .text where relocation applies
    pub sym_idx: u32,   // index into symbol table
    pub rela_type: u32, // R_X86_64_PC32=2, R_X86_64_PLT32=4
    pub addend: i64,
}

const ET_REL: u16 = 1;
const EM_X86_64: u16 = 62;
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHF_ALLOC: u64 = 2;
const SHF_EXECINSTR: u64 = 4;
const SHF_WRITE: u64 = 1;

impl Elf64Writer {
    pub fn new() -> Self {
        let mut strtab = vec![0u8]; // first byte is null
        Self {
            code: Vec::new(),
            data: Vec::new(),
            rodata: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
            strtab,
        }
    }

    fn add_string(&mut self, s: &str) -> u32 {
        let offset = self.strtab.len() as u32;
        self.strtab.extend_from_slice(s.as_bytes());
        self.strtab.push(0);
        offset
    }

    pub fn add_function(&mut self, name: &str, offset: u64, size: u64) {
        self.symbols.push(Elf64Sym {
            name: name.to_string(),
            value: offset,
            size,
            section: 1,
            bind: 1, // global
            typ: 2,  // function
        });
    }

    pub fn add_extern(&mut self, name: &str) {
        self.symbols.push(Elf64Sym {
            name: name.to_string(),
            value: 0,
            size: 0,
            section: 0, // undefined
            bind: 1,
            typ: 0,
        });
    }

    pub fn add_relocation(&mut self, offset: u64, sym_name: &str, addend: i64) {
        let sym_idx = self
            .symbols
            .iter()
            .position(|s| s.name == sym_name)
            .map(|i| (i + 1) as u32) // +1 because index 0 is null symbol
            .unwrap_or(0);
        self.relocations.push(Elf64Rela {
            offset,
            sym_idx,
            rela_type: 4, // R_X86_64_PLT32
            addend,
        });
    }

    pub fn write(&mut self) -> Vec<u8> {
        let mut buf = Vec::new();

        // section layout:
        // [ELF header: 64 bytes]
        // [.text]
        // [.data]
        // [.rodata]
        // [.symtab]
        // [.strtab]
        // [.shstrtab]
        // [.rela.text]
        // [section headers]

        let ehdr_size = 64u64;
        let text_off = ehdr_size;
        let text_size = self.code.len() as u64;
        let data_off = text_off + text_size;
        let data_size = self.data.len() as u64;
        let rodata_off = data_off + data_size;
        let rodata_size = self.rodata.len() as u64;

        // build strtab with all symbol names
        let names: Vec<String> = self.symbols.iter().map(|s| s.name.clone()).collect();
        let mut sym_name_offsets = Vec::new();
        for name in &names {
            sym_name_offsets.push(self.add_string(name));
        }

        // build shstrtab
        let mut shstrtab = vec![0u8];
        let sh_text = shstrtab.len();
        shstrtab.extend_from_slice(b".text\0");
        let sh_data = shstrtab.len();
        shstrtab.extend_from_slice(b".data\0");
        let sh_rodata = shstrtab.len();
        shstrtab.extend_from_slice(b".rodata\0");
        let sh_symtab = shstrtab.len();
        shstrtab.extend_from_slice(b".symtab\0");
        let sh_strtab = shstrtab.len();
        shstrtab.extend_from_slice(b".strtab\0");
        let sh_shstrtab = shstrtab.len();
        shstrtab.extend_from_slice(b".shstrtab\0");
        let sh_rela = shstrtab.len();
        shstrtab.extend_from_slice(b".rela.text\0");

        // symtab: null entry + symbols
        let sym_count = 1 + self.symbols.len();
        let symtab_entry_size = 24u64;
        let symtab_off = rodata_off + rodata_size;
        let symtab_size = sym_count as u64 * symtab_entry_size;

        let strtab_off = symtab_off + symtab_size;
        let strtab_size = self.strtab.len() as u64;

        let shstrtab_off = strtab_off + strtab_size;
        let shstrtab_size = shstrtab.len() as u64;

        let rela_off = shstrtab_off + shstrtab_size;
        let rela_entry_size = 24u64;
        let rela_size = self.relocations.len() as u64 * rela_entry_size;

        let shdr_off = rela_off + rela_size;
        // align to 8
        let shdr_off = (shdr_off + 7) & !7;
        let shdr_count = 8u16; // null + text + data + rodata + symtab + strtab + shstrtab + rela.text

        // ELF header
        buf.extend_from_slice(&[0x7F, b'E', b'L', b'F']);
        buf.push(ELFCLASS64);
        buf.push(ELFDATA2LSB);
        buf.push(1); // version
        buf.extend_from_slice(&[0; 9]); // padding
        buf.extend_from_slice(&ET_REL.to_le_bytes());
        buf.extend_from_slice(&EM_X86_64.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes()); // version
        buf.extend_from_slice(&0u64.to_le_bytes()); // entry (none for .o)
        buf.extend_from_slice(&0u64.to_le_bytes()); // phoff (none)
        buf.extend_from_slice(&shdr_off.to_le_bytes()); // shoff
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&64u16.to_le_bytes()); // ehsize
        buf.extend_from_slice(&0u16.to_le_bytes()); // phentsize
        buf.extend_from_slice(&0u16.to_le_bytes()); // phnum
        buf.extend_from_slice(&64u16.to_le_bytes()); // shentsize
        buf.extend_from_slice(&shdr_count.to_le_bytes());
        buf.extend_from_slice(&6u16.to_le_bytes()); // shstrndx (index of .shstrtab)

        // sections
        buf.extend_from_slice(&self.code);
        buf.extend_from_slice(&self.data);
        buf.extend_from_slice(&self.rodata);

        // symtab: null entry
        buf.extend_from_slice(&[0u8; 24]);
        // symtab: actual symbols
        for (i, sym) in self.symbols.iter().enumerate() {
            let name_off = if i < sym_name_offsets.len() {
                sym_name_offsets[i]
            } else {
                0
            };
            buf.extend_from_slice(&name_off.to_le_bytes());
            buf.push((sym.bind << 4) | sym.typ);
            buf.push(0); // other
            buf.extend_from_slice(&sym.section.to_le_bytes());
            buf.extend_from_slice(&sym.value.to_le_bytes());
            buf.extend_from_slice(&sym.size.to_le_bytes());
        }

        // strtab
        buf.extend_from_slice(&self.strtab);

        // shstrtab
        buf.extend_from_slice(&shstrtab);

        // rela.text
        for rela in &self.relocations {
            buf.extend_from_slice(&rela.offset.to_le_bytes());
            let info = ((rela.sym_idx as u64) << 32) | rela.rela_type as u64;
            buf.extend_from_slice(&info.to_le_bytes());
            buf.extend_from_slice(&rela.addend.to_le_bytes());
        }

        // pad to shdr alignment
        while buf.len() < shdr_off as usize {
            buf.push(0);
        }

        // section headers (64 bytes each)
        // 0: null
        write_shdr(&mut buf, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
        // 1: .text
        write_shdr(
            &mut buf,
            sh_text as u32,
            SHT_PROGBITS,
            SHF_ALLOC | SHF_EXECINSTR,
            0,
            text_off,
            text_size,
            0,
            0,
            16,
            0,
        );
        // 2: .data
        write_shdr(
            &mut buf,
            sh_data as u32,
            SHT_PROGBITS,
            SHF_ALLOC | SHF_WRITE,
            0,
            data_off,
            data_size,
            0,
            0,
            8,
            0,
        );
        // 3: .rodata
        write_shdr(
            &mut buf,
            sh_rodata as u32,
            SHT_PROGBITS,
            SHF_ALLOC,
            0,
            rodata_off,
            rodata_size,
            0,
            0,
            8,
            0,
        );
        // 4: .symtab
        write_shdr(
            &mut buf,
            sh_symtab as u32,
            SHT_SYMTAB,
            0,
            0,
            symtab_off,
            symtab_size,
            5,
            1,
            8,
            symtab_entry_size,
        );
        // 5: .strtab
        write_shdr(
            &mut buf,
            sh_strtab as u32,
            SHT_STRTAB,
            0,
            0,
            strtab_off,
            strtab_size,
            0,
            0,
            1,
            0,
        );
        // 6: .shstrtab
        write_shdr(
            &mut buf,
            sh_shstrtab as u32,
            SHT_STRTAB,
            0,
            0,
            shstrtab_off,
            shstrtab_size,
            0,
            0,
            1,
            0,
        );
        // 7: .rela.text
        write_shdr(
            &mut buf,
            sh_rela as u32,
            SHT_RELA,
            0,
            0,
            rela_off,
            rela_size,
            4,
            1,
            8,
            rela_entry_size,
        );

        buf
    }
}

fn write_shdr(
    buf: &mut Vec<u8>,
    name: u32,
    typ: u32,
    flags: u64,
    addr: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
) {
    buf.extend_from_slice(&name.to_le_bytes());
    buf.extend_from_slice(&typ.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&addr.to_le_bytes());
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&size.to_le_bytes());
    buf.extend_from_slice(&link.to_le_bytes());
    buf.extend_from_slice(&info.to_le_bytes());
    buf.extend_from_slice(&align.to_le_bytes());
    buf.extend_from_slice(&entsize.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_elf64_header() {
        let mut w = Elf64Writer::new();
        w.code = vec![0xC3]; // ret
        w.add_function("main", 0, 1);
        let elf = w.write();
        assert_eq!(&elf[0..4], &[0x7F, b'E', b'L', b'F']);
        assert_eq!(elf[4], ELFCLASS64);
        assert_eq!(u16::from_le_bytes([elf[18], elf[19]]), EM_X86_64);
        assert_eq!(u16::from_le_bytes([elf[16], elf[17]]), ET_REL);
    }

    #[test]
    fn has_text_section() {
        let mut w = Elf64Writer::new();
        w.code = vec![0x48, 0x89, 0xC8, 0xC3]; // mov rax,rcx; ret
        w.add_function("test_fn", 0, 4);
        let elf = w.write();
        // code should appear at offset 64
        assert_eq!(&elf[64..68], &[0x48, 0x89, 0xC8, 0xC3]);
    }

    #[test]
    fn extern_symbol() {
        let mut w = Elf64Writer::new();
        w.code = vec![0xC3];
        w.add_extern("puts");
        w.add_function("main", 0, 1);
        let elf = w.write();
        assert!(elf.len() > 64);
    }
}
