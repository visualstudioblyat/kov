# CLI Reference

kov has 17 commands. here's what each does.

## kov build

compile to binary.

```
kov build blink.kov -o firmware.elf
kov build blink.kov -o firmware.bin
kov build hello.kov -o hello --target x86-64
```

## kov run

compile and execute in the built-in emulator.

```
kov run blink.kov
kov run blink.kov -c 50000
```

`-c` sets max cycles. default is 10,000.

## kov asm

show generated RISC-V assembly with labels.

```
kov asm blink.kov
```

## kov trace

output per-cycle JSON trace for the time-travel debugger.

```
kov trace blink.kov -c 500
```

## kov wcet

worst-case execution time, stack depth, loop bounds, and energy analysis.

```
kov wcet blink.kov
```

## kov check

type check without compiling. supports JSON error output.

```
kov check blink.kov
kov check blink.kov --error-format=json
```

## kov test

run functions marked with `#[test]`.

```
kov test tests.kov
```

## kov repl

interactive compile and evaluate.

```
kov repl
kov> 3 + 4
  = 7 (0x7)
```

## kov flash

compile and flash to hardware via probe-rs.

```
kov flash blink.kov --chip esp32c3
```

## kov boards

list supported board targets.

## kov init

create a new project with kov.toml and main.kov.

```
kov init myproject --board esp32c3
```

## kov add

add a dependency to kov.toml.

```
kov add esp32c3-hal --git https://github.com/example/hal
```

## kov svd

generate board definitions from SVD files.

```
kov svd esp32c3.svd --name esp32c3
```

## kov import-c

generate extern declarations from C headers.

```
kov import-c hal.h
```

## kov wcet-elf

analyze any RISC-V ELF binary for WCET and stack depth.

```
kov wcet-elf firmware.elf
```

## kov lsp

start the language server for editor integration.

## kov lex

dump tokens for debugging.
