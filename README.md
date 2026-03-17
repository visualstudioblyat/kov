# kov

![CI](https://github.com/visualstudioblyat/kov/actions/workflows/ci.yml/badge.svg)

a risc-v compiler. no llvm, no gcc, no runtime. source goes in, firmware comes out.

**[try it in your browser](https://kov.dev/playground)** -- no install needed.

## what is this

i wanted a language where the compiler knows about hardware. where claiming a gpio pin twice is a compile error. where the compiler can tell you your interrupt handler takes 47 cycles before you flash it. where the whole toolchain fits in 400KB of wasm and runs in a browser tab.

nothing did all of that, so i built it.

## what it actually does

```
$ kov run examples/blink.kov
  compiled: 492 bytes (400 compressed) in 0.6ms
  executed: 10000 cycles in 1.0ms
  io:       1214 writes
            [0x60004004] <- 0x4    GPIO pin 2 HIGH
            [0x60004008] <- 0x4    GPIO pin 2 LOW
            ...repeating
```

```
$ kov wcet examples/blink.kov
  wcet analysis (2 functions):
  main(): 23 cycles
  on_tick(): 3 cycles
  stack analysis:
  main(): 16 bytes (frame: 16)
  on_tick(): 16 bytes (frame: 16)
  energy estimate:
  main(): 0.003 uJ (3400 pJ)
  on_tick(): 0.000 uJ (300 pJ)
```

```
$ kov repl
kov> 3 + 4
  = 7 (0x7)
kov> 100 / 3
  = 33 (0x21)
```

## the language

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

peripheral ownership is a language feature. claim `gpio.pin(2)` twice and the compiler rejects it. the board definition is syntax, not a linker script. `#[stack(512)]` is a compile error if the call graph exceeds 512 bytes.

## features that exist right now

**language:** structs, enums with data, generics, traits, impl blocks, match with exhaustiveness, error unions + try, labeled loops, inline assembly, cast expressions, short-circuit && ||

**compiler:** 8 optimizer passes (constant folding, dce, cse, copy propagation, strength reduction, function inlining, tail call, rv32c compression), linear scan register allocator with liveness analysis, reproducible builds

**safety:** `#[stack(N)]` and `#[max_cycles(N)]` enforced as compile errors, peripheral double-claim detection, interrupt safety (shared global detection), dma buffer typestate, no implicit integer promotion, match exhaustiveness

**analysis:** wcet per function, stack depth across call graph, automatic loop bound derivation, energy estimation in microjoules

**tooling:** 17 cli commands, built-in rv32im emulator, time-travel debugger (per-cycle trace), risc-v disassembler, lsp server, vs code extension, wasm playground (400KB), repl, c header import, svd parser, package manager

**targets:** esp32c3, ch32v003, gd32vf103, fe310, stm32f4, nrf52840, rp2040

## try it

```
cargo build
cargo run -- run examples/blink.kov
cargo run -- asm examples/blink.kov
cargo run -- wcet examples/blink.kov
cargo run -- repl
cargo run -- check examples/blink.kov
cargo test
```

or just go to [kov.dev/playground](https://kov.dev/playground).

## numbers

210 tests. 15,500 lines of rust. 400KB wasm. 0.6ms compile time. 492 bytes for blink. 7 board targets. 17 cli commands.

## license

Apache-2.0
