// embedded allocators — no OS, no malloc, just raw memory management

use super::emit::Emitter;
use super::encode::*;

// bump allocator: fast, no fragmentation, can't free individual objects
// good for init-time allocation that lives forever
pub fn emit_bump_allocator(emitter: &mut Emitter, heap_start: u32, heap_size: u32) {
    // global: heap pointer starts at heap_start
    // alloc(size) → returns pointer, advances heap_ptr
    // no free — memory is never reclaimed

    emitter.label("__heap_start");
    emitter.label("bump_alloc");

    // a0 = requested size
    // load current heap pointer from __heap_ptr
    let heap_ptr_addr = heap_start as i32;
    let (i1, i2) = li32(T0, heap_ptr_addr);
    emitter.emit32(i1);
    if let Some(i) = i2 {
        emitter.emit32(i);
    }
    emitter.emit32(lw(T1, T0, 0)); // t1 = current heap ptr

    // result = current heap ptr
    emitter.emit32(mv(A0, T1));

    // advance: heap_ptr += size (align to 4)
    emitter.emit32(addi(T2, A0, 3)); // align up
    let mask = !3i32;
    let (m1, m2) = li32(T2, mask);
    emitter.emit32(m1);
    if let Some(m) = m2 {
        emitter.emit32(m);
    }
    emitter.emit32(and(T2, T2, T2)); // aligned size
    emitter.emit32(add(T1, T1, T2)); // new heap ptr

    // bounds check
    let heap_end = (heap_start + heap_size) as i32;
    let (e1, e2) = li32(T2, heap_end);
    emitter.emit32(e1);
    if let Some(e) = e2 {
        emitter.emit32(e);
    }
    emitter.emit_branch(blt(T1, T2, 0), "_bump_ok");

    // out of memory — call panic
    emitter.emit_jump(j_offset(0), "panic");

    emitter.label("_bump_ok");
    // store new heap ptr
    emitter.emit32(sw(T0, T1, 0));
    emitter.emit32(ret());
}

// pool allocator: fixed-size blocks, O(1) alloc/free
pub struct PoolConfig {
    pub block_size: u32,
    pub num_blocks: u32,
    pub base: u32,
}

impl PoolConfig {
    pub fn total_size(&self) -> u32 {
        // each block has a next-free pointer (4 bytes) + data
        (self.block_size + 4) * self.num_blocks + 4 // +4 for free list head
    }
}

// arena allocator: bump with reset
pub fn emit_arena_reset(emitter: &mut Emitter, heap_start: u32) {
    emitter.label("arena_reset");
    let addr = heap_start as i32;
    let (i1, i2) = li32(T0, addr);
    emitter.emit32(i1);
    if let Some(i) = i2 {
        emitter.emit32(i);
    }
    // reset heap pointer to start
    let (s1, s2) = li32(T1, addr + 4); // skip the pointer itself
    emitter.emit32(s1);
    if let Some(s) = s2 {
        emitter.emit32(s);
    }
    emitter.emit32(sw(T0, T1, 0));
    emitter.emit32(ret());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_allocator_emits() {
        let mut emitter = Emitter::new();
        emit_bump_allocator(&mut emitter, 0x20004000, 4096);
        assert!(emitter.code.len() > 0);
        assert!(emitter.labels.contains_key("bump_alloc"));
    }

    #[test]
    fn arena_reset_emits() {
        let mut emitter = Emitter::new();
        emit_arena_reset(&mut emitter, 0x20004000);
        assert!(emitter.code.len() > 0);
        assert!(emitter.labels.contains_key("arena_reset"));
    }

    #[test]
    fn pool_sizing() {
        let pool = PoolConfig {
            block_size: 32,
            num_blocks: 16,
            base: 0x20004000,
        };
        assert_eq!(pool.total_size(), (32 + 4) * 16 + 4);
    }
}
