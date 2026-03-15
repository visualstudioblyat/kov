use std::collections::HashMap;
use crate::parser::ast::{Program, TopItem, BoardField, Expr};

pub struct PeripheralMap {
    // "gpio" → 0x6000_4000
    pub addresses: HashMap<String, u32>,
    pub board_name: Option<String>,
}

// register offsets for common peripheral operations
// these would come from SVD files in a full implementation
pub const GPIO_OUTPUT_SET: u32 = 0x04;
pub const GPIO_OUTPUT_CLEAR: u32 = 0x08;
pub const GPIO_ENABLE: u32 = 0x0C;
pub const GPIO_PIN_OFFSET: u32 = 0x10; // per-pin config stride

pub const UART_DATA: u32 = 0x00;
pub const UART_STATUS: u32 = 0x04;
pub const UART_BAUD: u32 = 0x08;

impl PeripheralMap {
    pub fn from_program(program: &Program) -> Self {
        let mut addresses = HashMap::new();
        let mut board_name = None;

        for item in &program.items {
            if let TopItem::Board(b) = item {
                board_name = Some(b.name.clone());
                for field in &b.fields {
                    if let Some(addr_expr) = &field.address {
                        if let Expr::IntLit(addr, _) = addr_expr {
                            addresses.insert(field.name.clone(), *addr as u32);
                        }
                    }
                }
            }
        }

        Self { addresses, board_name }
    }

    pub fn get_address(&self, peripheral: &str) -> Option<u32> {
        self.addresses.get(peripheral).copied()
    }
}

// given a method call like led.high(), resolve what MMIO operations to emit
pub struct MmioOp {
    pub address: u32,
    pub value: MmioValue,
    pub is_write: bool,
    pub width: u32, // 1, 2, or 4 bytes
}

pub enum MmioValue {
    Constant(u32),
    Register(u32), // physical register holding the value
}

// resolve a method call on a peripheral to MMIO operations
pub fn resolve_method(
    peripheral: &str,
    method: &str,
    base_addr: u32,
    pin_num: Option<u32>,
) -> Option<Vec<MmioOp>> {
    match (peripheral, method) {
        ("gpio", "high") | (_, "set_high") => {
            let pin = pin_num.unwrap_or(0);
            Some(vec![MmioOp {
                address: base_addr + GPIO_OUTPUT_SET,
                value: MmioValue::Constant(1 << pin),
                is_write: true,
                width: 4,
            }])
        }
        ("gpio", "low") | (_, "set_low") => {
            let pin = pin_num.unwrap_or(0);
            Some(vec![MmioOp {
                address: base_addr + GPIO_OUTPUT_CLEAR,
                value: MmioValue::Constant(1 << pin),
                is_write: true,
                width: 4,
            }])
        }
        ("gpio", "pin") => {
            // pin configuration: enable output mode
            let pin = pin_num.unwrap_or(0);
            Some(vec![MmioOp {
                address: base_addr + GPIO_ENABLE,
                value: MmioValue::Constant(1 << pin),
                is_write: true,
                width: 4,
            }])
        }
        ("uart", "open") => {
            // baud rate config — the actual value comes from the argument
            Some(vec![MmioOp {
                address: base_addr + UART_BAUD,
                value: MmioValue::Constant(0), // placeholder, set from arg
                is_write: true,
                width: 4,
            }])
        }
        ("uart", "write") => {
            Some(vec![MmioOp {
                address: base_addr + UART_DATA,
                value: MmioValue::Constant(0), // each byte written separately
                is_write: true,
                width: 1,
            }])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    #[test]
    fn extract_peripheral_addresses() {
        let src = r#"
            board esp32c3 {
                gpio: GPIO @ 0x6000_4000,
                uart: UART @ 0x6000_0000,
                clock: 160_000_000,
            }
        "#;
        let tokens = Lexer::tokenize(src).unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let map = PeripheralMap::from_program(&program);

        assert_eq!(map.get_address("gpio"), Some(0x6000_4000));
        assert_eq!(map.get_address("uart"), Some(0x6000_0000));
        assert_eq!(map.board_name, Some("esp32c3".into()));
    }

    #[test]
    fn resolve_gpio_high() {
        let ops = resolve_method("gpio", "high", 0x6000_4000, Some(2)).unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].address, 0x6000_4000 + GPIO_OUTPUT_SET);
        assert!(ops[0].is_write);
        match ops[0].value {
            MmioValue::Constant(v) => assert_eq!(v, 1 << 2),
            _ => panic!("expected constant"),
        }
    }

    #[test]
    fn resolve_gpio_low() {
        let ops = resolve_method("gpio", "low", 0x6000_4000, Some(2)).unwrap();
        assert_eq!(ops[0].address, 0x6000_4000 + GPIO_OUTPUT_CLEAR);
        match ops[0].value {
            MmioValue::Constant(v) => assert_eq!(v, 1 << 2),
            _ => panic!("expected constant"),
        }
    }

    #[test]
    fn resolve_gpio_pin_config() {
        let ops = resolve_method("gpio", "pin", 0x6000_4000, Some(2)).unwrap();
        assert_eq!(ops[0].address, 0x6000_4000 + GPIO_ENABLE);
    }

    #[test]
    fn resolve_uart_write() {
        let ops = resolve_method("uart", "write", 0x6000_0000, None).unwrap();
        assert_eq!(ops[0].address, 0x6000_0000 + UART_DATA);
    }

    #[test]
    fn unknown_method_returns_none() {
        assert!(resolve_method("gpio", "nonexistent", 0, None).is_none());
    }
}
