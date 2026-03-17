# kov benchmarks

measured on the blink.kov example (ESP32-C3 target).

## code size

| metric | value |
|--------|-------|
| uncompressed | 492 bytes |
| RV32C compressed | 400 bytes |
| startup + builtins overhead | ~200 bytes |
| user code only | ~200 bytes |

for comparison, a minimal C blink with gcc -Os for RISC-V is typically 400-800 bytes depending on the HAL.

## compile time

| metric | value |
|--------|-------|
| lex + parse + check + lower + optimize + codegen | 0.6ms |
| WASM compile in browser | ~2ms |

for comparison, gcc takes 2-10 seconds. rustc takes 10-60 seconds. zig takes 1-5 seconds.

## optimizer impact

| pass | effect |
|------|--------|
| constant folding | 3+4 → 7 at compile time |
| dead code elimination | removes unused values |
| CSE | deduplicates repeated computations |
| strength reduction | x*2 → x+x |
| copy propagation | eliminates identity moves |
| function inlining | small leaf functions inlined |
| tail call opt | return f(x) → jump |
| RV32C compression | 16% code size reduction |
| linear scan regalloc | 60 bytes smaller than first-fit |

## test coverage

| category | count |
|----------|-------|
| lexer | 10 |
| parser | 15+ |
| type checker | 15+ |
| IR lowering | 15+ |
| codegen | 15+ |
| optimizer | 8 |
| emulator | 11 |
| hardware validation | 6 |
| other (builtins, SVD, etc) | 100+ |
| **total** | **197** |

## targets

7 boards supported: ESP32-C3, CH32V003, GD32VF103, FE310, STM32F4, nRF52840, RP2040.
