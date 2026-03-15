# kov roadmap

every real language goes through the same stages. this is what that looks like for kov.

---

## phase 0: proof of concept (done)

the compiler exists. it lexes, parses, type checks, lowers to SSA IR, generates RISC-V machine code, and runs it in a built-in emulator. GPIO registers toggle. the LED blinks.

- [x] hand-written lexer
- [x] recursive descent parser with Pratt expressions
- [x] type checker with peripheral ownership
- [x] SSA IR with block parameters
- [x] RV32IM instruction encoder
- [x] MMIO codegen (methods → volatile stores at real addresses)
- [x] startup code generation + interrupt wrappers
- [x] ELF and flat binary output
- [x] built-in RV32IM emulator
- [x] `kov build` and `kov run` commands
- [x] 4 examples, 80 tests, CI

---

## phase 1: the language works

right now the language is a skeleton. this phase makes it a real language where you can write non-trivial programs.

**memory model**
- [ ] global and static variables (allocate in .data/.bss, load/store codegen)
- [ ] stack-allocated local variables (proper frame layout, not just registers)
- [ ] array allocation and indexing with bounds checks
- [ ] struct layout (field offsets, alignment, sizeof)
- [ ] string literals in .rodata with pointer codegen

**control flow**
- [ ] match statements (pattern matching, exhaustiveness checking)
- [ ] nested if/else with proper SSA phi resolution
- [ ] break and continue in loops
- [ ] early return from nested blocks

**functions**
- [ ] proper calling convention (save/restore callee-saved regs)
- [ ] multiple return values or struct returns
- [ ] function pointers
- [ ] recursive functions (with stack depth tracking)

**error handling**
- [ ] error unions (!T) with try keyword
- [ ] panic handler (halt, reset, or log)
- [ ] bounds check failure paths

**type system**
- [ ] local type inference (let x = 42 infers u32)
- [ ] struct field access type resolution
- [ ] enum variant type checking and matching
- [ ] function signature validation
- [ ] implicit integer widening/narrowing rules

---

## phase 2: the compiler is good

the language works but the compiler is naive. this phase makes the output code actually efficient and the errors actually helpful.

**register allocator**
- [ ] linear scan with live interval analysis
- [ ] spill code generation (load/store to stack)
- [ ] register coalescing (eliminate unnecessary moves)
- [ ] callee-saved register preservation

**optimizations**
- [ ] dead code elimination
- [ ] constant folding and propagation
- [ ] common subexpression elimination
- [ ] function inlining (small functions, single-call-site)
- [ ] strength reduction (multiply by power of 2 → shift)
- [ ] tail call optimization
- [ ] compressed instruction (RV32C) post-pass for code density

**diagnostics**
- [ ] source line display with caret pointing at error
- [ ] multi-span errors ("declared here", "used here")
- [ ] fix suggestions ("did you mean...", "add mut")
- [ ] JSON error format (--error-format=json) for tooling
- [ ] unused variable/import warnings

---

## phase 3: the novel features

this is what makes kov different from "yet another language." these features don't exist together in any other compiler.

**WCET analysis**
- [ ] instruction cycle cost model per target (ESP32-C3, CH32V003, etc.)
- [ ] path analysis through CFG (longest path = worst case)
- [ ] loop bound enforcement (#[bound(N)] required for WCET)
- [ ] #[max_cycles(N)] annotation that fails the build if exceeded
- [ ] WCET report in compiler output

**stack depth proofs**
- [ ] per-function stack frame size calculation
- [ ] call graph construction (detect recursion → error)
- [ ] whole-program stack depth sum along deepest call path
- [ ] #[stack(N)] annotation that fails the build if exceeded
- [ ] interrupt stack separate analysis

**interrupt safety**
- [ ] priority ceiling protocol implementation
- [ ] shared resource analysis (which vars accessed from which contexts)
- [ ] automatic critical section insertion
- [ ] compile error on unprotected shared access between ISR and main

**DMA safety**
- [ ] Buffer<OwnedByCpu> → Buffer<DmaActive> typestate transitions
- [ ] compile error on CPU access to DMA-active buffer
- [ ] await completion returns buffer to CPU ownership

---

## phase 4: real hardware

the compiler produces correct code. now it needs to run on actual chips, not just the emulator.

**board support**
- [ ] ESP32-C3: full GPIO, UART, SPI, I2C, timer register maps from SVD
- [ ] CH32V003: 16-register RV32EC mode, 2KB RAM constraints
- [ ] GD32VF103: ECLIC interrupt controller, DMA channels
- [ ] SiFive FE310: PLIC, QSPI flash XIP

**flash and debug**
- [ ] `kov flash` command via probe-rs library
- [ ] auto-detect connected board (USB VID/PID)
- [ ] `kov monitor` for UART output capture
- [ ] GDB server integration (debug via OpenOCD)

**validation**
- [ ] run blink on real ESP32-C3 (video proof)
- [ ] run blink on real CH32V003
- [ ] QEMU system emulation as alternative to built-in emulator
- [ ] hardware-in-the-loop test framework

**builtins**
- [ ] delay_ms / delay_us (busy-wait calibrated to clock speed)
- [ ] memcpy, memset (used by startup code)
- [ ] minimal printf-like formatting for UART output
- [ ] panic handler with UART backtrace

---

## phase 5: self-hosting

the compiler compiles itself. this is the milestone that proves the language is real. Rust did this. Go did this. Zig did this. every serious language eventually self-hosts.

- [ ] subset of Kov sufficient to express the compiler's logic
- [ ] bootstrap path: Rust compiler → first Kov compiler → self-compiled Kov compiler
- [ ] verify bit-identical output between Rust-compiled and Kov-compiled compiler
- [ ] remove Rust dependency (Kov builds from source with only a C compiler or previous Kov binary)

---

## phase 6: ecosystem

a language without libraries is a toy. this phase builds the ecosystem that makes kov usable for real projects.

**package manager**
- [ ] `kov.toml` project manifest
- [ ] dependency resolution from git repos
- [ ] board support packages as Kov source (not binary blobs)
- [ ] versioning and lock files
- [ ] `kov add <package>` command

**standard library**
- [ ] core: GPIO, UART, SPI, I2C traits
- [ ] collections: fixed-capacity Vec, RingBuffer, BitSet
- [ ] math: fixed-point utilities, integer formatting
- [ ] fmt: lightweight format strings (no heap, no alloc)
- [ ] time: delay, timer, stopwatch
- [ ] sync: Mutex, CriticalSection, Atomic

**documentation**
- [ ] language reference (every syntax construct documented)
- [ ] tutorial (from zero to blinking LED)
- [ ] board-specific guides (ESP32-C3, CH32V003)
- [ ] API docs generated from source comments

---

## phase 7: tooling

the language is usable. now it needs to be pleasant.

**editor support**
- [ ] language server (LSP) — completion, go-to-definition, hover types
- [ ] syntax highlighting grammar (TextMate/VS Code)
- [ ] error squiggles in real-time

**browser playground**
- [ ] compile Kov compiler to WASM
- [ ] embed editor + emulator on kov.dev
- [ ] show RISC-V assembly output in real-time
- [ ] show register state as code executes
- [ ] share links (source code in URL)

**agent SDK**
- [ ] compiler as Rust library API
- [ ] MCP server for Claude/GPT integration
- [ ] structured JSON diagnostics with fix suggestions
- [ ] deterministic builds (same source → same binary, always)
- [ ] autonomous compile → emulate → flash → monitor loop

---

## phase 8: second backend

kov targets RISC-V. adding ARM Cortex-M proves the IR is truly target-independent and opens up the STM32/nRF/RP2040 ecosystem.

- [ ] ARM Thumb-2 instruction encoder
- [ ] ARM calling convention and startup code
- [ ] NVIC interrupt controller support
- [ ] STM32F4 / nRF52840 / RP2040 board support

---

## phase 9: maturity

the language is stable. backward compatibility starts to matter and the community grows.

- [ ] language specification (formal grammar, semantics, memory model)
- [ ] stability guarantee (code that compiles today compiles in 5 years)
- [ ] edition system for opt-in breaking changes
- [ ] security audit of compiler
- [ ] formal verification of type system soundness
- [ ] published benchmarks vs C and Rust
- [ ] real users shipping real products

---

for reference, Rust took 6 years from first public release to mass adoption. Go about the same. Zig is at year 8 approaching 1.0. kov is at week 1.
