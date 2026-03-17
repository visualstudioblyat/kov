# Language Reference

## types

kov has explicit types with no implicit promotion. `u8 + u32` is a compile error.

| type | size | description |
|------|------|-------------|
| `u8` | 1 byte | unsigned 8-bit |
| `u16` | 2 bytes | unsigned 16-bit |
| `u32` | 4 bytes | unsigned 32-bit |
| `u64` | 8 bytes | unsigned 64-bit |
| `i8` to `i64` | varies | signed integers |
| `bool` | 1 byte | true or false |
| `void` | 0 | no value |
| `!T` | 8 bytes | error union |

## variables

```kov
let x = 42;
let y: u32 = 100;
let mut counter = 0;
counter = counter + 1;
```

type inference works. `let x = 42` infers `u32`.

## functions

```kov
fn add(a: u32, b: u32) u32 {
    return a + b;
}
```

## generics

```kov
fn max<T>(a: T, b: T) T {
    if a > b { return a; }
    return b;
}
```

generics are monomorphized. `max(3, 4)` generates `max_u32` at compile time.

## structs

```kov
struct Point { x: u32, y: u32 }

impl Point {
    fn sum(px: u32, py: u32) u32 {
        return px + py;
    }
}

fn main() {
    let p = Point { x: 3, y: 4 };
    let s = p.sum();
}
```

fields are laid out with natural alignment.

## enums

```kov
enum Option { None, Some(u32) }

fn unwrap(opt: Option) u32 {
    match opt {
        Some(val) => val,
        _ => 0,
    }
}
```

enums with data are tagged unions. the compiler checks exhaustiveness.

## error unions

```kov
fn read_sensor() !u32 {
    if error_condition { return 0; }
    return 42;
}

fn main() !u32 {
    let val = try read_sensor();
    return val;
}
```

`try` checks the error tag and propagates if nonzero.

## control flow

```kov
if x > 0 {
    // ...
} else if x == 0 {
    // ...
} else {
    // ...
}

loop { break; }
while x < 10 { x = x + 1; }
for i in 0..10 { }

'outer: loop {
    loop { break 'outer; }
}

match value {
    0 => handle_zero(),
    1 => handle_one(),
    _ => handle_default(),
}
```

`&&` and `||` are short-circuit.

## board definitions

```kov
board esp32c3 {
    gpio: GPIO @ 0x6000_4000,
    uart: UART @ 0x6000_0000,
    clock: 160_000_000,
}
```

the board definition is part of the grammar. peripheral addresses come from the hardware datasheet.

## attributes

```kov
#[stack(512)]     // compile error if stack exceeds 512 bytes
#[max_cycles(200)] // compile error if WCET exceeds 200 cycles
#[test]           // marks a test function
#[cfg(esp32c3)]   // conditional compilation
```
