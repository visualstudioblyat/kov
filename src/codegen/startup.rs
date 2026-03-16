use super::emit::Emitter;
use super::encode::*;

pub struct BoardConfig {
    pub ram_start: u32,
    pub ram_size: u32,
    pub flash_start: u32,
}

impl BoardConfig {
    // known boards — later this comes from the board{} block in source
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "esp32c3" => Some(Self {
                flash_start: 0x4200_0000,
                ram_start: 0x3FC8_0000,
                ram_size: 400 * 1024,
            }),
            "ch32v003" => Some(Self {
                flash_start: 0x0800_0000,
                ram_start: 0x2000_0000,
                ram_size: 2 * 1024,
            }),
            "gd32vf103" => Some(Self {
                flash_start: 0x0800_0000,
                ram_start: 0x2000_0000,
                ram_size: 32 * 1024,
            }),
            "fe310" => Some(Self {
                flash_start: 0x2001_0000,
                ram_start: 0x8000_0000,
                ram_size: 16 * 1024,
            }),
            _ => None,
        }
    }

    pub fn stack_top(&self) -> u32 {
        self.ram_start + self.ram_size
    }
}

// emit _start: stack init → .bss zero → call main → halt
pub fn emit_startup(emitter: &mut Emitter, board: &BoardConfig) {
    emitter.label("_start");

    // disable interrupts: csrci mstatus, 0x8
    emitter.emit32(csrrci(ZERO, MSTATUS, 8));

    // set stack pointer to top of RAM
    let sp_val = board.stack_top() as i32;
    let (inst1, inst2) = li32(SP, sp_val);
    emitter.emit32(inst1);
    if let Some(i2) = inst2 {
        emitter.emit32(i2);
    }

    // zero .bss — for now just zero 256 bytes at ram_start
    // (proper .bss bounds come from linker info later)
    let bss_start = board.ram_start as i32;
    let (b1, b2) = li32(T0, bss_start);
    emitter.emit32(b1);
    if let Some(b) = b2 {
        emitter.emit32(b);
    }

    // t1 = bss_start + 256
    emitter.emit32(addi(T1, T0, 256));

    emitter.label("_zero_bss");
    emitter.emit_branch(bge(T0, T1, 0), "_bss_done");
    emitter.emit32(sw(T0, ZERO, 0));
    emitter.emit32(addi(T0, T0, 4));
    emitter.emit_jump(j_offset(0), "_zero_bss");

    emitter.label("_bss_done");

    // call main
    emitter.emit_jump(jal(RA, 0), "main");

    // halt: wfi loop
    emitter.label("_halt");
    emitter.emit32(csrrci(ZERO, MSTATUS, 8)); // disable interrupts
    emitter.emit32(wfi());
    emitter.emit_jump(j_offset(0), "_halt");

    // panic handler: disable interrupts, halt
    // called by bounds checks, failed assertions, etc.
    // a0 = error code (optional)
    emitter.label("panic");
    emitter.emit32(csrrci(ZERO, MSTATUS, 8));
    emitter.emit32(wfi());
    emitter.emit_jump(j_offset(0), "panic");
}

// emit interrupt vector table — vectored mode, jumps to handlers
pub fn emit_vector_table(
    emitter: &mut Emitter,
    handlers: &[(u32, String)], // (cause_number, handler_label)
    max_vectors: u32,
) {
    emitter.label("_vector_table");

    for i in 0..max_vectors {
        if let Some((_, label)) = handlers.iter().find(|(n, _)| *n == i) {
            emitter.emit_jump(j_offset(0), label);
        } else {
            emitter.emit_jump(j_offset(0), "_default_handler");
        }
    }

    // default handler just returns
    emitter.label("_default_handler");
    emitter.emit32(mret());
}

// emit interrupt handler wrapper — save/restore caller-saved regs
pub fn emit_interrupt_wrapper(emitter: &mut Emitter, name: &str) {
    let wrapper_label = format!("_isr_{}", name);
    emitter.label(&wrapper_label);

    // save 16 caller-saved registers
    emitter.emit32(addi(SP, SP, -64));
    emitter.emit32(sw(SP, RA, 0));
    emitter.emit32(sw(SP, T0, 4));
    emitter.emit32(sw(SP, T1, 8));
    emitter.emit32(sw(SP, T2, 12));
    emitter.emit32(sw(SP, A0, 16));
    emitter.emit32(sw(SP, A1, 20));
    emitter.emit32(sw(SP, A2, 24));
    emitter.emit32(sw(SP, A3, 28));
    emitter.emit32(sw(SP, A4, 32));
    emitter.emit32(sw(SP, A5, 36));
    emitter.emit32(sw(SP, A6, 40));
    emitter.emit32(sw(SP, A7, 44));
    emitter.emit32(sw(SP, 28, 48)); // t3
    emitter.emit32(sw(SP, 29, 52)); // t4
    emitter.emit32(sw(SP, 30, 56)); // t5
    emitter.emit32(sw(SP, 31, 60)); // t6

    // call the user handler
    emitter.emit_jump(jal(RA, 0), name);

    // restore
    emitter.emit32(lw(RA, SP, 0));
    emitter.emit32(lw(T0, SP, 4));
    emitter.emit32(lw(T1, SP, 8));
    emitter.emit32(lw(T2, SP, 12));
    emitter.emit32(lw(A0, SP, 16));
    emitter.emit32(lw(A1, SP, 20));
    emitter.emit32(lw(A2, SP, 24));
    emitter.emit32(lw(A3, SP, 28));
    emitter.emit32(lw(A4, SP, 32));
    emitter.emit32(lw(A5, SP, 36));
    emitter.emit32(lw(A6, SP, 40));
    emitter.emit32(lw(A7, SP, 44));
    emitter.emit32(lw(28, SP, 48));
    emitter.emit32(lw(29, SP, 52));
    emitter.emit32(lw(30, SP, 56));
    emitter.emit32(lw(31, SP, 60));
    emitter.emit32(addi(SP, SP, 64));
    emitter.emit32(mret());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_emits_code() {
        let mut e = Emitter::new();
        let board = BoardConfig::from_name("gd32vf103").unwrap();
        emit_startup(&mut e, &board);
        // should have _start label and some instructions
        assert!(e.pos() > 20);
    }

    #[test]
    fn vector_table_correct_size() {
        let mut e = Emitter::new();
        let handlers = vec![(7, "on_tick".to_string())];
        emit_vector_table(&mut e, &handlers, 16);
        // 16 jump instructions + 1 mret = 17 * 4 = 68 bytes
        assert_eq!(e.pos(), 68);
    }

    #[test]
    fn interrupt_wrapper_saves_restores() {
        let mut e = Emitter::new();
        emit_interrupt_wrapper(&mut e, "on_tick");
        // 1 addi + 16 sw + 1 jal + 16 lw + 1 addi + 1 mret = 36 instructions = 144 bytes
        assert_eq!(e.pos(), 144);
    }

    #[test]
    fn known_boards() {
        assert!(BoardConfig::from_name("esp32c3").is_some());
        assert!(BoardConfig::from_name("ch32v003").is_some());
        assert!(BoardConfig::from_name("gd32vf103").is_some());
        assert!(BoardConfig::from_name("fe310").is_some());
        assert!(BoardConfig::from_name("nonexistent").is_none());
    }

    #[test]
    fn stack_top_correct() {
        let board = BoardConfig::from_name("ch32v003").unwrap();
        assert_eq!(board.stack_top(), 0x2000_0000 + 2048);
    }
}
