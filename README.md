# kov

![CI](https://github.com/visualstudioblyat/kov/actions/workflows/ci.yml/badge.svg)

A systems language and compiler for RISC-V bare metal. No LLVM, no runtime, no external toolchain. Source code goes in, firmware binary comes out.

## why

Rust's embedded ecosystem gets peripheral ownership right, but it's a library pattern on top of a general-purpose language with a 2GB LLVM toolchain. RTIC and Embassy are good frameworks, but they're macros, not language features. WCET analysis exists as separate tools (aiT, WCC) that run after compilation, not during it. Zig has a self-hosted backend but no hardware awareness. Every embedded developer I've talked to spends more time fighting toolchains than writing application logic.

I want all of these things in one language, designed together from the start: peripheral ownership as a language primitive, cycle counting and stack proofs integrated into the compiler, interrupts as syntax not macros, board definitions as source code not linker scripts, and a compiler small enough to embed in a browser or hand to an AI agent.

No single tool does all of this, so I'm building one that does. Mostly for myself, because I want it to exist. If other people find it useful, that's a bonus.

## what it does (eventually)

- Compiles a Rust-like language directly to RISC-V machine code
- Peripheral ownership as a language primitive, not a library pattern
- Cycle counting integrated into the compiler, not a separate analysis tool
- Stack depth proofs across the whole call graph, not per-function
- Interrupt handlers as language syntax with priority ceiling enforcement
- Board definitions as part of the grammar, not external config files
- No LLVM, no GCC, no external assembler, no external linker
- Sub-5ms compile times
- Agent-native: library API, JSON errors, deterministic output

## where it is now

The compiler works end-to-end. It compiles Kov source to RISC-V machine code and runs it in a built-in emulator that verifies GPIO register writes.

```
$ kov run examples/blink.kov
  compiled: 416 bytes in 0.3ms
  executed: 2000 cycles in 0.2ms
  io:       215 writes
            [0x60004004] ← 0x4    ← GPIO pin 2 HIGH
            [0x60004008] ← 0x4    ← GPIO pin 2 LOW
            ...repeating
```

80 tests, 4 example programs, ~4,500 lines of Rust.

## try it

```
cargo build
cargo run -- run examples/blink.kov
cargo run -- run examples/counter.kov
cargo run -- run examples/pattern.kov
cargo run -- build examples/blink.kov -o firmware.elf
cargo test
```

## example

```
board esp32c3 {
    gpio: GPIO @ 0x6000_4000,
    uart: UART @ 0x6000_0000,
    clock: 160_000_000,
}

#[stack(512)]
fn main(b: &mut esp32c3) {
    let led = b.gpio.pin(2, .output);
    let tx = b.uart.open(115200);

    loop {
        led.high();
        delay_ms(500);
        led.low();
        delay_ms(500);
        tx.write("blink\n");
    }
}

interrupt(timer0, priority = 2) fn on_tick() {
    static mut counter: u32 = 0;
    counter += 1;
}
```

## license

Apache-2.0
