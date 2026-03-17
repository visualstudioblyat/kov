# Getting Started

kov is a systems language for RISC-V embedded. no LLVM, no runtime. source goes in, firmware comes out.

## try it in your browser

go to [kov.dev/playground](https://kov.dev/playground). type code on the left, see assembly on the right. no install needed.

## install

```
cargo install kov
```

or build from source:

```
git clone https://github.com/visualstudioblyat/kov
cd kov
cargo build --release
```

## your first program

create `blink.kov`:

```kov
board esp32c3 {
    gpio: GPIO @ 0x6000_4000,
    clock: 160_000_000,
}

#[stack(512)]
fn main(b: &mut esp32c3) {
    let led = b.gpio.pin(2, .output);
    loop {
        led.high();
        delay_ms(500);
        led.low();
        delay_ms(500);
    }
}
```

## compile and run

```
kov run blink.kov
```

this compiles your program, runs it in the built-in emulator, and shows GPIO register writes. no hardware needed.

## compile to binary

```
kov build blink.kov -o firmware.elf
```

## flash to hardware

```
kov flash blink.kov --chip esp32c3
```

requires [probe-rs](https://probe.rs/) installed.

## what just happened

the compiler:
1. lexed and parsed your source (0.6ms)
2. type-checked it (peripheral ownership, stack bounds)
3. lowered to SSA IR
4. ran 8 optimizer passes
5. generated RISC-V machine code
6. compressed with RV32C (16% smaller)
7. output a 400-byte binary

all from one command. no makefiles, no linker scripts, no external tools.
