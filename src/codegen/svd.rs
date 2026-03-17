// minimal SVD (System View Description) parser
// extracts peripheral names, base addresses, and register offsets from XML
// no external dependencies — hand-rolled XML extraction

pub struct SvdPeripheral {
    pub name: String,
    pub base_address: u32,
    pub registers: Vec<SvdRegister>,
}

pub struct SvdRegister {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub access: String,
}

pub fn parse_svd(xml: &str) -> Vec<SvdPeripheral> {
    let mut peripherals = Vec::new();
    let mut pos = 0;

    while let Some(start) = xml[pos..].find("<peripheral>") {
        let abs_start = pos + start;
        let end = match xml[abs_start..].find("</peripheral>") {
            Some(e) => abs_start + e + 13,
            None => break,
        };
        let block = &xml[abs_start..end];

        let name = extract_tag(block, "name").unwrap_or_default();
        let base = extract_tag(block, "baseAddress")
            .and_then(|s| parse_hex_or_dec(&s))
            .unwrap_or(0);

        let mut registers = Vec::new();
        let mut rpos = 0;
        while let Some(rs) = block[rpos..].find("<register>") {
            let rabs = rpos + rs;
            let re = match block[rabs..].find("</register>") {
                Some(e) => rabs + e + 11,
                None => break,
            };
            let rblock = &block[rabs..re];

            let rname = extract_tag(rblock, "name").unwrap_or_default();
            let offset = extract_tag(rblock, "addressOffset")
                .and_then(|s| parse_hex_or_dec(&s))
                .unwrap_or(0);
            let size = extract_tag(rblock, "size")
                .and_then(|s| parse_hex_or_dec(&s))
                .unwrap_or(32);
            let access = extract_tag(rblock, "access").unwrap_or_else(|| "read-write".into());

            registers.push(SvdRegister {
                name: rname,
                offset,
                size,
                access,
            });
            rpos = re;
        }

        peripherals.push(SvdPeripheral {
            name,
            base_address: base,
            registers,
        });
        pos = end;
    }
    peripherals
}

// generate Kov source code from SVD peripherals
pub fn generate_kov(peripherals: &[SvdPeripheral], board_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("board {} {{\n", board_name));
    for p in peripherals {
        out.push_str(&format!(
            "    {}: {} @ {:#010X},\n",
            p.name.to_lowercase(),
            p.name,
            p.base_address
        ));
    }
    out.push_str("}\n\n");

    for p in peripherals {
        out.push_str(&format!(
            "// {} registers (base: {:#010X})\n",
            p.name, p.base_address
        ));
        for r in &p.registers {
            out.push_str(&format!(
                "// {}.{}: offset {:#06X}, {}bit, {}\n",
                p.name, r.name, r.offset, r.size, r.access
            ));
        }
        out.push('\n');
    }
    out
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

fn parse_hex_or_dec(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        u32::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_svd() {
        let xml = r#"
        <device>
            <peripheral>
                <name>GPIOA</name>
                <baseAddress>0x40020000</baseAddress>
                <register>
                    <name>ODR</name>
                    <addressOffset>0x14</addressOffset>
                    <size>32</size>
                    <access>read-write</access>
                </register>
                <register>
                    <name>IDR</name>
                    <addressOffset>0x10</addressOffset>
                    <size>32</size>
                    <access>read-only</access>
                </register>
            </peripheral>
        </device>
        "#;
        let peripherals = parse_svd(xml);
        assert_eq!(peripherals.len(), 1);
        assert_eq!(peripherals[0].name, "GPIOA");
        assert_eq!(peripherals[0].base_address, 0x40020000);
        assert_eq!(peripherals[0].registers.len(), 2);
        assert_eq!(peripherals[0].registers[0].name, "ODR");
        assert_eq!(peripherals[0].registers[0].offset, 0x14);
    }

    #[test]
    fn generate_board_from_svd() {
        let peripherals = vec![
            SvdPeripheral {
                name: "GPIO".into(),
                base_address: 0x60004000,
                registers: vec![SvdRegister {
                    name: "OUT".into(),
                    offset: 0x04,
                    size: 32,
                    access: "read-write".into(),
                }],
            },
            SvdPeripheral {
                name: "UART".into(),
                base_address: 0x60000000,
                registers: vec![],
            },
        ];
        let kov = generate_kov(&peripherals, "myboard");
        assert!(kov.contains("board myboard"));
        assert!(kov.contains("gpio: GPIO @ 0x60004000"));
        assert!(kov.contains("uart: UART @ 0x60000000"));
    }
}
