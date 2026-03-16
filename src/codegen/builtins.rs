use super::emit::Emitter;
use super::encode::*;

// emit built-in runtime functions that embedded programs need
pub fn emit_builtins(emitter: &mut Emitter, clock_hz: u32) {
    emit_delay_ms(emitter, clock_hz);
    emit_delay_us(emitter, clock_hz);
    emit_memset(emitter);
    emit_memcpy(emitter);
}

// delay_ms: busy-wait loop calibrated to clock speed
// a0 = milliseconds to wait
// loop iterations = (clock_hz / 1000) * ms / ~4 (4 cycles per loop iteration)
fn emit_delay_ms(emitter: &mut Emitter, clock_hz: u32) {
    emitter.label("delay_ms");
    // t0 = cycles per ms / 4 (each loop iteration is ~4 cycles)
    let cycles_per_ms = clock_hz / 1000 / 4;
    let (i1, i2) = li32(T0, cycles_per_ms as i32);
    emitter.emit32(i1);
    if let Some(i) = i2 {
        emitter.emit32(i);
    }
    // t1 = a0 * t0 (total iterations)
    emitter.emit32(mul(T1, A0, T0));

    emitter.label("_delay_ms_loop");
    emitter.emit32(addi(T1, T1, -1));
    emitter.emit_branch(bne(T1, ZERO, 0), "_delay_ms_loop");
    emitter.emit32(ret());
}

// delay_us: same idea, microsecond granularity
fn emit_delay_us(emitter: &mut Emitter, clock_hz: u32) {
    emitter.label("delay_us");
    let cycles_per_us = (clock_hz / 1_000_000 / 4).max(1);
    let (i1, i2) = li32(T0, cycles_per_us as i32);
    emitter.emit32(i1);
    if let Some(i) = i2 {
        emitter.emit32(i);
    }
    emitter.emit32(mul(T1, A0, T0));

    emitter.label("_delay_us_loop");
    emitter.emit32(addi(T1, T1, -1));
    emitter.emit_branch(bne(T1, ZERO, 0), "_delay_us_loop");
    emitter.emit32(ret());
}

// memset: a0 = dest, a1 = byte value, a2 = count
fn emit_memset(emitter: &mut Emitter) {
    emitter.label("memset");
    // t0 = dest + count (end pointer)
    emitter.emit32(add(T0, A0, A2));

    emitter.label("_memset_loop");
    emitter.emit_branch(bge(A0, T0, 0), "_memset_done");
    emitter.emit32(sb(A0, A1, 0));
    emitter.emit32(addi(A0, A0, 1));
    emitter.emit_jump(j_offset(0), "_memset_loop");

    emitter.label("_memset_done");
    emitter.emit32(ret());
}

// memcpy: a0 = dest, a1 = src, a2 = count
fn emit_memcpy(emitter: &mut Emitter) {
    emitter.label("memcpy");
    emitter.emit32(add(T0, A0, A2)); // t0 = end

    emitter.label("_memcpy_loop");
    emitter.emit_branch(bge(A0, T0, 0), "_memcpy_done");
    emitter.emit32(lbu(T1, A1, 0));
    emitter.emit32(sb(A0, T1, 0));
    emitter.emit32(addi(A0, A0, 1));
    emitter.emit32(addi(A1, A1, 1));
    emitter.emit_jump(j_offset(0), "_memcpy_loop");

    emitter.label("_memcpy_done");
    emitter.emit32(ret());
}

#[cfg(test)]
mod tests {
    use super::super::emit::Emitter;
    use super::*;

    #[test]
    fn builtins_emit() {
        let mut emitter = Emitter::new();
        emit_builtins(&mut emitter, 160_000_000);
        assert!(emitter.code.len() > 0);
        assert!(emitter.labels.contains_key("delay_ms"));
        assert!(emitter.labels.contains_key("delay_us"));
        assert!(emitter.labels.contains_key("memset"));
        assert!(emitter.labels.contains_key("memcpy"));
    }

    #[test]
    fn delay_label_resolves() {
        let mut emitter = Emitter::new();
        emit_builtins(&mut emitter, 160_000_000);
        // the internal loop labels should exist
        assert!(emitter.labels.contains_key("_delay_ms_loop"));
        assert!(emitter.labels.contains_key("_delay_us_loop"));
    }
}
