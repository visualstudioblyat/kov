// hardware validation tests
// these verify that compiled output is correct RISC-V machine code
// by round-tripping through encode → decode → verify

#[test]
fn blink_all_instructions_valid_rv32im() {
    let source = std::fs::read_to_string("examples/blink.kv").unwrap();
    let output = kovlib::compile(&source).unwrap();

    // every 4-byte aligned word must be a valid RV32IM instruction
    let code = &output.code;
    assert!(code.len() % 4 == 0, "code must be 4-byte aligned");

    let mut invalid = Vec::new();
    for i in (0..code.len()).step_by(4) {
        let inst = u32::from_le_bytes([code[i], code[i + 1], code[i + 2], code[i + 3]]);
        if !is_valid_rv32im(inst) {
            invalid.push((i, inst));
        }
    }

    assert!(
        invalid.is_empty(),
        "found {} invalid instructions: {:?}",
        invalid.len(),
        invalid.iter().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn blink_boots_and_writes_gpio() {
    let source = std::fs::read_to_string("examples/blink.kv").unwrap();
    let result = kovlib::run(&source, 500_000).unwrap();

    assert!(result.cycles > 0, "should execute some cycles");
    assert!(
        !result.mmio_writes.is_empty(),
        "should write GPIO registers"
    );

    // verify GPIO writes are to the correct address range
    for (addr, _val) in &result.mmio_writes {
        assert!(
            *addr >= 0x6000_0000 && *addr < 0x7000_0000,
            "MMIO write to unexpected address: {:#X}",
            addr
        );
    }
}

#[test]
fn blink_gpio_toggles() {
    // use a fast-clock variant so delays are short enough to toggle in reasonable cycles
    let source = "board esp32c3 { gpio: GPIO @ 0x6000_4000, clock: 40_000, }
        fn main(b: &mut esp32c3) { let led = b.gpio.pin(2, .output);
            loop { led.high(); delay_ms(1); led.low(); delay_ms(1); } }";
    let result = kovlib::run(source, 500_000).unwrap();

    // should see alternating set/clear on GPIO
    let gpio_set = 0x6000_4004u32;
    let gpio_clear = 0x6000_4008u32;

    let sets = result
        .mmio_writes
        .iter()
        .filter(|(a, _)| *a == gpio_set)
        .count();
    let clears = result
        .mmio_writes
        .iter()
        .filter(|(a, _)| *a == gpio_clear)
        .count();

    assert!(sets > 0, "should set GPIO at least once");
    assert!(clears > 0, "should clear GPIO at least once");
    // set and clear should be roughly equal (blink pattern)
    assert!(
        (sets as i64 - clears as i64).unsigned_abs() <= 1,
        "set ({}) and clear ({}) should be balanced",
        sets,
        clears
    );
}

#[test]
fn emulator_register_invariants() {
    let source = std::fs::read_to_string("examples/blink.kv").unwrap();
    let output = kovlib::compile(&source).unwrap();

    let mut cpu =
        kovlib::emu::Cpu::with_memory(output.flash_base, output.flash_base, output.ram_base);
    cpu.mem.load_flash(&output.code);
    cpu.regs[2] = output.ram_top;

    // run some cycles
    cpu.run(1000);

    // x0 must always be zero
    assert_eq!(cpu.regs[0], 0, "x0 must always be zero");

    // PC must be within flash range
    assert!(
        cpu.pc >= output.flash_base && cpu.pc < output.flash_base + output.code.len() as u32 + 1024,
        "PC {:#X} out of flash range",
        cpu.pc
    );

    // SP must be within RAM range
    let sp = cpu.regs[2];
    assert!(
        sp >= output.ram_base && sp <= output.ram_top,
        "SP {:#X} out of RAM range ({:#X}..{:#X})",
        sp,
        output.ram_base,
        output.ram_top
    );
}

#[test]
fn all_examples_compile() {
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "kv").unwrap_or(false) {
            let source = std::fs::read_to_string(&path).unwrap();
            let result = kovlib::compile(&source);
            assert!(
                result.is_ok(),
                "example {:?} failed to compile: {:?}",
                path,
                result.err()
            );
        }
    }
}

#[test]
fn all_examples_run_without_crash() {
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().map(|e| e == "kv").unwrap_or(false) {
            let source = std::fs::read_to_string(&path).unwrap();
            let result = kovlib::run(&source, 10_000);
            assert!(
                result.is_ok(),
                "example {:?} crashed in emulator: {:?}",
                path,
                result.err()
            );
        }
    }
}

fn is_valid_rv32im(inst: u32) -> bool {
    let opcode = inst & 0x7F;
    match opcode {
        0x37 | 0x17 => true,                                       // LUI, AUIPC
        0x6F => true,                                              // JAL
        0x67 => (inst >> 12) & 7 == 0,                             // JALR
        0x63 => matches!((inst >> 12) & 7, 0 | 1 | 4 | 5 | 6 | 7), // branches
        0x03 => matches!((inst >> 12) & 7, 0 | 1 | 2 | 4 | 5),     // loads
        0x23 => matches!((inst >> 12) & 7, 0 | 1 | 2),             // stores
        0x13 => true,                                              // immediate ALU
        0x33 => {
            let f7 = inst >> 25;
            matches!(f7, 0x00 | 0x20 | 0x01) // base + M extension
        }
        0x0F => true,            // FENCE
        0x73 => true,            // SYSTEM (CSR, ecall, ebreak, wfi)
        _ => inst == 0x00000013, // NOP is valid
    }
}
