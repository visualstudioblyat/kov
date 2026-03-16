# kov roadmap

every real language goes through the same stages. this is what that looks like for kov, broken down to the level where each item is an implementable unit of work.

---

## phase 0: proof of concept ✓

the compiler exists end-to-end. source code becomes machine code that toggles GPIO registers in the built-in emulator.

- [x] hand-written lexer (75+ token types, hex/bin/oct literals, nested comments)
- [x] recursive descent parser with Pratt expression parsing (11 precedence levels)
- [x] AST with spans for error reporting
- [x] type checker with peripheral double-claim detection
- [x] SSA IR with block parameters (cranelift-style, no phi nodes)
- [x] IR lowering with MMIO resolution (method calls → volatile stores at board addresses)
- [x] RV32IM instruction encoder (all base integer + M extension)
- [x] code emitter with forward/backward label fixups
- [x] startup code generation (_start, stack init, .bss zero, call main, wfi halt)
- [x] interrupt handler wrappers (save/restore 16 caller-saved regs, mret)
- [x] ELF32 and flat binary output
- [x] built-in RV32IM emulator with MMIO logging
- [x] `kov build` and `kov run` commands
- [x] peripheral address resolution from board{} definition
- [x] 4 board configs (ESP32-C3, CH32V003, GD32VF103, FE310)
- [x] 4 examples, 80 tests, CI on 3 platforms

---

## phase 1: the language works

the language is currently a skeleton. you can blink an LED but you can't write a real driver. this phase fills in everything needed to write non-trivial embedded programs.

**memory model**
- [x] global variables: RAM address resolution, load/store codegen, .data and .bss placement
- [x] static variables inside functions: collected during lowering, allocated in global table
- [x] stack-allocated locals: register spilling to stack when all 22 registers exhausted
- [x] struct layout: field offset calculation with natural alignment, StructLit → StackAlloc + Store, field access → base + offset + Load
- [x] array allocation: ArrayLit → StackAlloc + Store per element, Index → base + (idx * elem_size) + Load
- [x] string literals: collected in pre-pass, stored in GlobalTable, StringLit → GlobalAddr pointer
- [ ] bounds checking: runtime index validation with panic on out-of-bounds

**control flow completeness**
- [x] match statements: chain of check blocks with int literal and wildcard patterns, exhaustiveness checking in type checker
- [x] break and continue: loop context stack, break → exit block, continue → header block
- [ ] labeled loops: `'outer: loop { ... break 'outer; }` for breaking out of nested loops
- [x] early return: jump to epilogue label with proper stack restoration

**functions**
- [x] calling convention: RISC-V psABI. two-pass codegen discovers used s-registers, prologue/epilogue saves only what's needed. 16-byte aligned frames.
- [ ] function calls with >8 arguments: spill to stack per ABI
- [ ] struct return: small structs in a0-a1, large structs via hidden pointer
- [ ] recursive functions: stack depth tracking with #[stack(N)] annotation
- [ ] function pointers: store function address, call via JALR

**error handling**
- [x] error unions: `!T` as tagged return — payload in a0, error tag in a1
- [x] try keyword: checks error tag, propagates via ReturnError if nonzero, unwraps payload if ok
- [x] panic handler: built-in `panic` label in startup code — disable interrupts + wfi halt
- [ ] bounds check failure: generates call to panic handler

**type system**
- [x] local type inference: `let x = 42;` infers u32, `let x = get();` infers from function return type
- [ ] integer promotion rules: u8 + u8 = u8 (not u32 like C)
- [x] struct types: field names and types resolved, struct literal validation (missing/unknown fields)
- [ ] enum types: variant tracking, discriminant values
- [x] function signatures: parameter count checked at call sites, return type inferred
- [x] unused variable warnings: _ prefix suppression, JSON warning output
- [x] match exhaustiveness: error if no wildcard arm on non-enum match

---

## phase 1.5: generics, traits, and the abstraction layer

without these, every data structure and peripheral driver is a one-off. this is what separates a language from a scripting tool.

**generics**
- [ ] monomorphization: `fn max<T: Ord>(a: T, b: T) -> T` generates separate machine code for `max::<u32>` and `max::<i16>`. no runtime dispatch, no vtables, zero cost.
- [ ] generic structs: `struct RingBuffer<T, const N: usize>` with compile-time size. instantiated per type+size combination.
- [ ] const generics: `[T; N]` where N is a compile-time value. essential for fixed-size buffers in embedded.
- [ ] type constraints: `T: Copy + Sized` style bounds. prevents passing non-copyable types to functions that duplicate values.

**traits**
- [ ] trait definitions: `trait Gpio { fn set_high(&mut self); fn set_low(&mut self); }` defines a peripheral interface.
- [ ] trait implementations: `impl Gpio for EspGpioPin { ... }` provides board-specific behavior.
- [ ] static dispatch: trait methods resolved at compile time via monomorphization. no vtable overhead.
- [ ] trait objects: `&dyn Gpio` for runtime polymorphism when needed. rare in embedded but necessary for driver abstraction.
- [ ] built-in traits: Copy, Clone, Drop, Sized, Default. Drop is how RAII cleanup works for peripheral handles.
- [ ] operator overloading via traits: Add, Sub, Mul for fixed-point types. `let c = a + b` where a and b are Fixed<16,16>.

**compile-time execution**
- [ ] const fn: functions that can run at compile time. `const fn clock_divider(freq: u32, baud: u32) -> u32 { freq / (16 * baud) }` computed during compilation, result embedded as immediate.
- [ ] const blocks: `const { ... }` evaluates arbitrary code at compile time. generates lookup tables, precomputes CRC polynomials, validates clock configurations.
- [ ] static assertions: `const_assert!(STACK_SIZE >= 512)` fails the build if false.
- [ ] board config validation: the clock tree math from the board{} definition is verified at compile time. PLL frequency out of range → compile error, not a runtime hang.

---

## phase 2: the compiler is good

the language works but produces inefficient code and cryptic errors. this phase makes the compiler competitive.

**register allocator**
- [x] callee-saved register tracking: two-pass codegen discovers used s-registers, saves/restores only what's needed
- [x] spill code generation: evict registers to stack when all 22 exhausted, pending spill/reload around instructions
- [x] live interval analysis: compute last-use for every value, expire dead values during allocation
- [x] linear scan allocation: free and reuse registers when values die, 60 bytes smaller output
- [x] register coalescing: copy propagation at IR level + param pre-assignment at register level

**optimizations (ordered by impact-to-effort ratio)**
- [x] dead code elimination: mark side-effecting ops, walk backward, delete unmarked
- [x] constant folding: evaluate const expressions at compile time, two-pass for chained folding
- [x] constant propagation: track constants through the IR, fold uses
- [x] common subexpression elimination: deduplicate pure ops, rewrite all uses
- [x] function inlining: inline tiny leaf functions (<=5 insts, called once) by substituting params
- [x] strength reduction: mul by 2 → add(x, x)
- [ ] copy propagation: if `y = x`, replace uses of y with x
- [ ] tail call optimization: reuse stack frame for tail calls
- [ ] compressed instruction pass: replace RV32I with RV32C equivalents (~25% size reduction)

**diagnostics**
- [x] source line display: show source line with carets under the error span
- [x] JSON error format: `--error-format=json` for editor/agent integration. file, line, column, severity, message.
- [x] warning system: unused variables (with _ prefix suppression), JSON warning output
- [x] error recovery: synchronize at top-level keywords, report multiple parse errors per compilation
- [x] `kov check` command: type check without compiling
- [ ] multi-span errors: "declared here" + "but used as X here"
- [ ] fix suggestions: structured suggestions in error output

---

## phase 3: the novel features

this is what makes kov different. these features don't exist together in any other compiler.

**WCET analysis**
- [ ] cycle cost table: map every RV32IM instruction to its cycle count for each supported target. most are 1 cycle. MUL is 1-5 depending on the core. DIV is 6-33. loads depend on cache (but embedded cores often have no cache — single cycle SRAM).
- [ ] basic block timing: sum instruction costs per basic block. trivial once the cost table exists.
- [ ] path enumeration: find the longest path through the function's CFG. for functions without loops, this is the max of all branch paths. for functions with loops, the loop bound annotation provides the iteration count.
- [ ] #[max_cycles(N)]: compute WCET for the annotated function. if WCET > N, emit a compile error: "function `on_tick` has worst-case execution time of 247 cycles, exceeding the limit of 200."
- [ ] WCET report: `kov build --wcet` prints a per-function breakdown. which basic block is the bottleneck. which instruction is the most expensive.
- [ ] call graph WCET: when function A calls function B, A's WCET includes B's WCET. recursive calls make WCET unbounded → error if #[max_cycles] is present.

**stack depth proofs**
- [ ] frame size calculation: for each function, compute stack usage = saved registers + local variables + alignment padding + outgoing arguments.
- [ ] call graph traversal: build the call graph. find the deepest call chain. sum frame sizes along the chain.
- [ ] interrupt stack: ISRs run on a separate stack (or the same stack in simple systems). the proof accounts for the worst case: main at maximum stack depth + ISR fires.
- [ ] #[stack(N)]: compute worst-case stack depth for the annotated function and everything it calls. if depth > N bytes, emit a compile error.
- [ ] recursion detection: if the call graph has a cycle, stack depth is unbounded. error if #[stack] is present on any function in the cycle.

**interrupt safety**
- [ ] context analysis: the compiler knows which functions are ISRs (from `interrupt(...)` annotation) and which run in main context.
- [ ] shared resource tracking: if a global variable is accessed from both main and an ISR, the compiler requires it to be wrapped in `Shared<T>` and accessed via `critical_section`.
- [ ] priority ceiling protocol: each shared resource has a ceiling = max priority of any ISR that accesses it. entering a critical section raises the current priority to the ceiling. this prevents priority inversion and is computed entirely at compile time.
- [ ] automatic critical section: the compiler could optionally insert interrupt disable/enable around shared access instead of requiring the programmer to write it. opt-in via attribute.

**DMA safety**
- [ ] typestate for buffers: `Buffer<OwnedByCpu>` can be read/written by CPU code. `Buffer<DmaActive>` cannot — any access is a compile error.
- [ ] transfer initiation: `let transfer = dma.start(buf);` moves `buf` into the transfer handle. the original variable is consumed — can't be used anymore.
- [ ] completion: `let buf = transfer.wait();` blocks until DMA completes, returns the buffer as `Buffer<OwnedByCpu>` again.
- [ ] this prevents the most common DMA bug: CPU reading a buffer while DMA is writing to it. the type system makes it impossible to express.

---

## phase 4: real hardware

proving the compiler works on actual chips, not just the emulator.

**board support from SVD**
- [ ] SVD parser: read XML System View Description files. extract every peripheral, register, and bitfield with correct addresses, sizes, and access types.
- [ ] code generation from SVD: produce Kov source files with typed register definitions. `GPIOA.ODR.write(1 << 13)` with the address and bitfield layout generated from the SVD.
- [ ] ESP32-C3: GPIO matrix, UART with FIFO, SPI with DMA, I2C, timer/counter, RTC, watchdog.
- [ ] CH32V003: GPIO ports A/C/D, USART1, SPI, I2C, TIM1/TIM2, watchdog, ADC. special handling for RV32EC (16 registers only — the register allocator must prefer x8-x15 for compressed instruction compatibility).
- [ ] GD32VF103: ECLIC interrupt controller (different from PLIC — hardware vectoring, fast context save), USART×3, SPI×2, I2C×2, DMA×7, ADC, DAC, timer×4.
- [ ] SiFive FE310: PLIC with 52 sources, GPIO, UART×2, SPI×3, PWM×3, I2C. QSPI flash XIP boot.

**flash and debug tooling**
- [ ] `kov flash`: link against probe-rs as a library. detect connected probe (CMSIS-DAP, ST-Link, J-Link, WCH-LinkE). identify target chip. erase, program, verify, reset. single command.
- [ ] `kov monitor`: open serial port to board, stream UART output to terminal. auto-detect port by USB VID/PID.
- [ ] DWARF debug info: emit `.debug_info`, `.debug_line`, `.debug_abbrev` sections in ELF. maps machine code addresses back to Kov source lines. GDB can step through Kov source, set breakpoints on Kov lines, print Kov variables.
- [ ] GDB integration: `kov debug` starts OpenOCD/probe-rs GDB server and connects GDB with the correct symbol file.

**validation milestones**
- [ ] blink LED on ESP32-C3 with Kov binary (video)
- [ ] blink LED on CH32V003 with Kov binary (cheapest RISC-V chip, ~$0.10)
- [ ] UART hello world on real hardware
- [ ] SPI sensor read on real hardware
- [ ] interrupt-driven timer on real hardware

**runtime builtins**
- [ ] delay_ms / delay_us: busy-wait loop calibrated to board clock speed. the board{} definition provides the clock frequency, the compiler generates the correct loop count.
- [ ] memcpy, memset: used by startup code for .data copy and .bss zero. hand-written in Kov or emitted as inline loops.
- [ ] panic handler: `fn panic(msg: &[u8]) -> !` that disables interrupts, optionally writes to UART, then halts or resets.
- [ ] minimal formatting: `write_u32(uart, value)` and `write_hex(uart, value)` for debug output without printf.

---

## phase 5: FFI and the C escape hatch

every embedded project has C libraries — vendor SDKs, CMSIS, FreeRTOS, lwIP. kov must call them.

- [ ] `extern "C"` function declarations: `extern "C" fn HAL_GPIO_WritePin(port: u32, pin: u32, state: u32);` tells the compiler this function exists in a linked C object.
- [ ] C type compatibility: kov's u32 is C's uint32_t. kov's `*u8` is C's `uint8_t*`. struct layout matches C when `#[repr(C)]` is used.
- [ ] linking C objects: `kov build --link vendor.o` links a precompiled C object file with the Kov output.
- [ ] C header parsing: optionally parse a `.h` file and generate Kov extern declarations. not a full C preprocessor — just function signatures and struct definitions.
- [ ] inline assembly: `asm!("csrr {rd}, mstatus", rd = out(reg) val)` emits raw RISC-V instructions. syntax similar to Rust's `asm!` macro.
- [ ] calling convention compatibility: kov functions can be called from C if declared `#[export] fn my_func(x: u32) -> u32`. generates a C-compatible symbol.

---

## phase 6: conditional compilation and build system

embedded code is inherently platform-specific. the language needs first-class support for this.

- [ ] `#[cfg(board = "esp32c3")]`: conditionally compile code based on the target board. different register addresses, different peripheral sets, different clock configurations.
- [ ] `#[cfg(feature = "uart")]`: feature flags in `kov.toml` that enable/disable code paths. `kov build --features uart,spi`.
- [ ] platform-specific modules: `import board::esp32c3::gpio` resolves to a different file than `import board::ch32v003::gpio`.
- [ ] build profiles: `kov build --release` enables optimizations and strips debug info. `kov build --debug` keeps debug info and enables bounds checks.
- [ ] `kov.toml`: project manifest with name, version, target board, dependencies, features, compiler flags.

---

## phase 7: self-hosting

the compiler compiles itself. every serious language does this eventually.

- [ ] language subset sufficient for the compiler: the compiler is ~5000 lines of Rust. the Kov subset needs: structs, enums, match, generics, traits, vectors (fixed-size), hashmaps (open addressing), string handling, file I/O (via FFI to libc or host syscalls).
- [ ] bootstrap chain: Rust-written compiler → compile Kov compiler source → self-compiled Kov compiler. the two outputs must produce identical binaries when compiling the same input (bit-for-bit reproducibility).
- [ ] stage verification: stage 1 (Rust-compiled) produces stage 2 (Kov-compiled). stage 2 compiles itself to produce stage 3. stage 2 and stage 3 must be identical.
- [ ] independence: once self-hosted, the only dependency is a previous Kov binary or a C compiler that can build a minimal bootstrap interpreter.

---

## phase 8: testing framework

firmware testing is painful in every language. kov makes it a language feature.

- [ ] `#[test]` attribute on functions: `kov test` discovers and runs all test functions.
- [ ] test execution in emulator: tests compile to RISC-V, run in the built-in emulator, report pass/fail based on the exit code.
- [ ] assertions: `assert(x == 42)`, `assert_eq(a, b)`. failure prints the values and source location via the panic handler.
- [ ] hardware-in-the-loop tests: `#[test(board = "esp32c3")]` compiles, flashes to the specified board, captures UART output, checks for pass/fail markers.
- [ ] test isolation: each test runs in a fresh emulator instance. global state is reset between tests.
- [ ] `kov test --filter gpio` runs only tests with "gpio" in the name.

---

## phase 9: ecosystem

a language without libraries is a toy.

**package manager**
- [ ] `kov.toml` manifest: name, version, target, dependencies (git URL + revision or tag).
- [ ] `kov add <package>`: adds a dependency, fetches source, updates lock file.
- [ ] lock file: exact revisions pinned for reproducible builds.
- [ ] board support packages: distributed as Kov source. `kov add esp32c3-hal` pulls GPIO/UART/SPI/I2C drivers.
- [ ] no central registry initially: dependencies are git repos. centralized registry comes later when there's a community.
- [ ] vendoring: `kov vendor` copies all dependencies into the project for offline/auditable builds.

**standard library**
- [ ] core::gpio: `trait InputPin { fn is_high(&self) -> bool; }` and `trait OutputPin { fn set_high(&mut self); fn set_low(&mut self); }`. every board HAL implements these.
- [ ] core::uart: `trait Read { fn read_byte(&mut self) -> !u8; }` and `trait Write { fn write_byte(&mut self, b: u8); }`.
- [ ] core::spi: `trait SpiMaster { fn transfer(&mut self, data: &mut [u8]); }`.
- [ ] core::i2c: `trait I2cMaster { fn write_read(&mut self, addr: u8, write: &[u8], read: &mut [u8]) -> !void; }`.
- [ ] core::timer: `trait Timer { fn delay_ms(&mut self, ms: u32); fn delay_us(&mut self, us: u32); }`.
- [ ] collections: `Vec<T, N>` (fixed-capacity, no heap), `RingBuffer<T, N>`, `BitSet<N>`.
- [ ] math: `Fixed<I, F>` arithmetic, integer formatting, CRC16/CRC32.
- [ ] fmt: lightweight format strings. `write!(uart, "temp: {} C\n", temperature)` without heap allocation or the code bloat of Rust's core::fmt.
- [ ] sync: `Mutex<T>` (interrupt-disabling), `CriticalSection` token, `Atomic<u32>`.

**documentation**
- [ ] language reference: every keyword, every syntax construct, every type, with examples.
- [ ] tutorial: "from zero to blinking LED" walkthrough.
- [ ] board guides: ESP32-C3, CH32V003 specific setup and examples.
- [ ] API docs: generated from `///` comments in source, hosted on kov.dev.
- [ ] cookbook: common patterns — debouncing a button, reading a sensor, driving a display, PID control loop.

---

## phase 10: tooling

the language is usable. now it needs to be pleasant to work with.

**editor support**
- [ ] language server (LSP): completion, go-to-definition, hover type info, rename, find references. built into the compiler binary (`kov lsp`).
- [ ] VS Code extension: syntax highlighting (TextMate grammar), LSP client, build tasks, debug launch config.
- [ ] error squiggles: red underlines appear as you type, before you save or build.

**browser playground**
- [ ] WASM build: compile the Kov compiler to WebAssembly using wasm-pack. the compiler is ~5000 lines — it'll be small.
- [ ] editor component: Monaco or CodeMirror with Kov syntax highlighting.
- [ ] live compilation: as you type, the compiler runs in a Web Worker, shows RISC-V assembly output on the right.
- [ ] emulator in browser: the RV32IM emulator also compiled to WASM. shows register state, memory contents, GPIO pin state as the code executes.
- [ ] share links: source code encoded in the URL. paste a link, see the code, run it.

**agent SDK**
- [ ] compiler as library: `kov::compile(source, target) -> Result<Binary, Vec<Diagnostic>>`. no subprocess, no file I/O.
- [ ] MCP server: expose compile/run/flash as Model Context Protocol tools. any LLM with MCP support can drive the compiler.
- [ ] deterministic builds: same source + same target = identical binary. no timestamps, no randomization. agents can verify their own output by compiling twice and diffing.
- [ ] autonomous loop: agent writes Kov → compiles → runs in emulator → checks output → modifies source → repeats. the JSON error format makes this a closed loop.

---

## phase 11: second backend (ARM Cortex-M)

adding ARM proves the IR is target-independent and opens the massive STM32/nRF/RP2040 ecosystem.

- [ ] Thumb-2 instruction encoder: 16-bit and 32-bit mixed instruction encoding. more complex than RISC-V but well-documented.
- [ ] ARM calling convention: AAPCS. r0-r3 arguments, r0-r1 return, r4-r11 callee-saved.
- [ ] ARM startup code: vector table at 0x00000000, stack pointer in first entry, reset handler in second entry. different from RISC-V.
- [ ] NVIC interrupt controller: priority-based, nested, with BASEPRI register for priority masking.
- [ ] board support: STM32F4 (common dev board), nRF52840 (Bluetooth), RP2040 (Raspberry Pi Pico).
- [ ] ARM-specific optimizations: IT blocks (conditional execution), CBZ/CBNZ (compare-and-branch), LDM/STM (multi-register load/store).

---

## phase 12: async and concurrency

embedded systems are inherently concurrent. interrupts fire, DMA completes, timers expire. the language needs a concurrency model.

- [ ] cooperative async/await: stackless state machines compiled from async functions. no heap, no allocator — all state is static.
- [ ] executor: a simple round-robin executor that polls tasks. built into the runtime, not a library.
- [ ] wakers: interrupt handlers signal the executor that a future is ready. the executor re-polls it.
- [ ] async I/O: `let data = uart.read(buf).await` yields until the DMA transfer completes.
- [ ] structured concurrency: spawned tasks are bound to a scope. no orphaned tasks, no leaked resources.
- [ ] no function coloring: sync functions can call async functions by blocking. this avoids the "viral async" problem that plagues Rust embedded.

---

## phase 13: allocator story

kov is heapless by default. but some programs need dynamic allocation.

- [ ] bump allocator: fast, no fragmentation, can't free individual objects. good for initialization-time allocation that lives forever.
- [ ] pool allocator: fixed-size blocks. O(1) alloc and free. good for network buffers, sensor readings.
- [ ] arena allocator: like bump but with a reset. allocate a batch of objects, process them, reset the arena. good for per-frame or per-request allocation.
- [ ] `#[allocator]` attribute: plug in a custom allocator per-module or per-scope. the compiler ensures no allocation happens in contexts where it's forbidden (ISRs, WCET-annotated functions).

---

## phase 14: maturity

the language is stable enough for production use.

- [ ] language specification: formal grammar (EBNF), type system rules, evaluation semantics, memory model. published as a document, not just "read the compiler source."
- [ ] stability guarantee: code that compiles today compiles in 5 years. no silent behavioral changes.
- [ ] edition system: opt-in breaking changes per edition (like Rust 2018/2021/2024). old code keeps working, new code gets new features.
- [ ] security audit: independent review of the compiler, the type checker, and the runtime builtins. focus on: can the type system be bypassed? can the emulator be escaped? are the MMIO accesses correct?
- [ ] formal verification: prove that the peripheral ownership checker is sound — if the compiler accepts a program, no two tasks can access the same peripheral simultaneously. Lean or Coq proof, not just tests.
- [ ] reproducible builds: verified across platforms. build on Linux, Windows, macOS — identical binary.
- [ ] ABI stability: separately compiled Kov modules can be linked together. the struct layout, calling convention, and name mangling are frozen.
- [ ] published benchmarks: code size vs C (gcc -Os) and Rust (release). compile time vs Zig. runtime performance on CoreMark. WCET accuracy vs aiT.
- [ ] real users: someone other than the author ships a product using Kov.

---

for reference: Rust took 6 years from first public release (2012) to mass adoption (~2018). Go about the same. Zig is at year 8 approaching 1.0. kov is at week 1, phase 0 complete. the gap between phase 0 and phase 14 is measured in years, not weeks. that's fine — every language starts here.
