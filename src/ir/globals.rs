use super::types::IrType;

#[derive(Debug)]
pub struct Global {
    pub name: String,
    pub ty: IrType,
    pub init: GlobalInit,
    pub mutable: bool,
}

#[derive(Debug)]
pub enum GlobalInit {
    Zero,
    Int(i32),
    Bytes(Vec<u8>),
}

#[derive(Debug)]
pub struct GlobalTable {
    pub globals: Vec<Global>,
    pub strings: Vec<(String, Vec<u8>)>, // (label, bytes)
    next_string: u32,
}

impl GlobalTable {
    pub fn new() -> Self {
        Self {
            globals: Vec::new(),
            strings: Vec::new(),
            next_string: 0,
        }
    }

    pub fn add_global(&mut self, name: String, ty: IrType, init: GlobalInit, mutable: bool) {
        self.globals.push(Global {
            name,
            ty,
            init,
            mutable,
        });
    }

    pub fn add_string(&mut self, data: &[u8]) -> String {
        let label = format!(".str{}", self.next_string);
        self.next_string += 1;
        self.strings.push((label.clone(), data.to_vec()));
        label
    }

    pub fn find(&self, name: &str) -> Option<&Global> {
        self.globals.iter().find(|g| g.name == name)
    }

    // compute total .data size (initialized globals)
    pub fn data_size(&self) -> u32 {
        self.globals
            .iter()
            .filter(|g| !matches!(g.init, GlobalInit::Zero))
            .map(|g| g.ty.size_bytes())
            .sum::<u32>()
            + self
                .strings
                .iter()
                .map(|(_, d)| (d.len() as u32 + 3) & !3)
                .sum::<u32>() // align to 4
    }

    // compute total .bss size (zero-initialized globals)
    pub fn bss_size(&self) -> u32 {
        self.globals
            .iter()
            .filter(|g| matches!(g.init, GlobalInit::Zero))
            .map(|g| g.ty.size_bytes())
            .sum()
    }

    // emit .data section bytes
    pub fn emit_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for g in &self.globals {
            match &g.init {
                GlobalInit::Zero => {}
                GlobalInit::Int(v) => data.extend_from_slice(&v.to_le_bytes()),
                GlobalInit::Bytes(b) => {
                    data.extend_from_slice(b);
                    // pad to 4-byte alignment
                    while data.len() % 4 != 0 {
                        data.push(0);
                    }
                }
            }
        }
        for (_, bytes) in &self.strings {
            data.extend_from_slice(bytes);
            data.push(0); // null terminator
            while data.len() % 4 != 0 {
                data.push(0);
            }
        }
        data
    }

    // compute address of a global relative to data section start
    pub fn offset_of(&self, name: &str) -> Option<u32> {
        let mut offset = 0u32;
        for g in &self.globals {
            if !matches!(g.init, GlobalInit::Zero) {
                if g.name == name {
                    return Some(offset);
                }
                offset += g.ty.size_bytes();
            }
        }
        // check zero-initialized globals (in BSS, after data)
        let bss_start = offset;
        let mut bss_offset = 0u32;
        for g in &self.globals {
            if matches!(g.init, GlobalInit::Zero) {
                if g.name == name {
                    return Some(bss_start + bss_offset);
                }
                bss_offset += g.ty.size_bytes();
            }
        }
        let after_bss = bss_start + bss_offset;
        // check strings
        let mut str_offset = after_bss;
        for (label, bytes) in &self.strings {
            if label == name {
                return Some(str_offset);
            }
            str_offset += ((bytes.len() as u32 + 1) + 3) & !3;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_table_basics() {
        let mut gt = GlobalTable::new();
        gt.add_global("counter".into(), IrType::I32, GlobalInit::Zero, true);
        gt.add_global("max_val".into(), IrType::I32, GlobalInit::Int(100), false);

        assert_eq!(gt.bss_size(), 4);
        assert_eq!(gt.data_size(), 4);
        assert!(gt.find("counter").is_some());
        assert!(gt.find("nonexistent").is_none());
    }

    #[test]
    fn string_storage() {
        let mut gt = GlobalTable::new();
        let label = gt.add_string(b"hello");
        assert!(label.starts_with(".str"));

        let data = gt.emit_data();
        assert!(data.len() >= 6); // "hello" + null + padding
    }
}
