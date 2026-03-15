# kov roadmap

what's done, what's next, roughly in order.

## done

- [x] hand-written lexer (75+ token types)
- [x] recursive descent parser with Pratt expression parsing
- [x] type checker with peripheral ownership enforcement
- [x] SSA IR with block parameters
- [x] MMIO codegen (peripheral methods → volatile register writes)
- [x] RV32IM instruction encoder
- [x] code emitter with label/fixup system
- [x] startup code generation (stack init, bss zero, call main, halt)
- [x] interrupt handler wrappers (save/restore, mret)
- [x] ELF and flat binary output
- [x] built-in RV32IM emulator with MMIO logging
- [x] `kov build` and `kov run` commands
- [x] 4 example programs (blink, counter, uart_hello, pattern)
- [x] CI (GitHub Actions)

## near-term

- [ ] global and static variable support (memory allocation, load/store)
- [ ] linear scan register allocator (current one is trivial first-fit)
- [ ] better error messages with source line display
- [ ] string data section (.rodata)
- [ ] delay_ms as a builtin (busy-wait loop based on clock speed)
- [ ] while loop codegen fix (condition re-evaluation)
- [ ] match statement codegen

## language features

- [ ] struct field access codegen
- [ ] enum variant matching
- [ ] array indexing with bounds checks
- [ ] fixed-point arithmetic (Fixed<I, F>)
- [ ] error unions (!T) and try keyword
- [ ] defer statement
- [ ] critical_section blocks
- [ ] const evaluation (comptime expressions)

## type system

- [ ] full type inference (local, not Hindley-Milner)
- [ ] struct type checking with field resolution
- [ ] enum variant type checking
- [ ] function signature validation (param/return types)
- [ ] board peripheral type tracking across function boundaries
- [ ] DMA buffer typestate (Buffer<OwnedByCpu> → Buffer<DmaActive>)

## compiler quality

- [ ] #[max_cycles(N)] enforcement (WCET analysis)
- [ ] #[stack(N)] enforcement (whole-program stack depth proof)
- [ ] dead code elimination
- [ ] constant folding and propagation
- [ ] common subexpression elimination
- [ ] function inlining
- [ ] compressed instruction (C extension) post-pass

## codegen

- [ ] proper calling convention (callee-saved register preservation)
- [ ] stack frame layout with local variables
- [ ] indirect function calls
- [ ] tail call optimization
- [ ] LUI+ADDI sign extension compensation (already done for li32)
- [ ] branch range checking with trampoline insertion

## targets

- [ ] ESP32-C3 (RV32IMC) — full peripheral support
- [ ] CH32V003 (RV32EC) — 16-register mode
- [ ] GD32VF103 (RV32IMAC) — with ECLIC interrupts
- [ ] SiFive FE310 (RV32IMAC) — with PLIC interrupts
- [ ] ARM Cortex-M (second backend, target-independent IR already supports this)

## tooling

- [ ] `kov flash` command (via probe-rs)
- [ ] `kov test` command (compile + run + check assertions)
- [ ] JSON error output (--error-format=json)
- [ ] compiler as library API (for agents)
- [ ] WASM build (browser playground)
- [ ] language server (LSP) for editor support
- [ ] disassembler (`kov disasm firmware.bin`)

## agent SDK

- [ ] MCP server exposing compile/run/flash as tools
- [ ] structured diagnostic output for agent consumption
- [ ] deterministic build verification
- [ ] probe-rs integration for autonomous flashing
- [ ] serial monitor for output capture
- [ ] emulator with assertion checking

## long-term

- [ ] self-hosting (kov compiles itself)
- [ ] package manager for board support packages
- [ ] formal verification of peripheral ownership soundness
- [ ] visual debugger (step through source, see registers update)
- [ ] browser playground on kov.dev
