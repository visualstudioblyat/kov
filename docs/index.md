# Kov Language

Kov is a **systems programming language** that compiles to RISC-V, x86-64, and ARM.

## Getting Started

Install the compiler and run your first program:

```
kov run hello.kov
```

The compiler produces native machine code with **zero runtime overhead**.

## Features

- Direct hardware access via `write_mmio` and `read_mmio`
- Inline assembly with `asm!`
- **Zero-cost abstractions** for embedded development
- Stack-based allocation with no garbage collector
- Import system for multi-file projects

## Example

Here is a simple program that prints to the console:

```
import io;

fn main() {
    println("hello from kov!");
}
```

---

## Why Kov?

Most embedded languages force you to choose between **safety** and **performance**. Kov gives you both.

- Compile-time memory safety
- WCET analysis for real-time systems
- Energy-aware compilation
- DMA safety via typestate tracking

Built from scratch. No LLVM. No runtime. Just code.
