// crash reporting: panic handler captures register state and writes
// a compact crash dump to a reserved memory region

use super::emit::Emitter;
use super::encode::*;

// crash dump layout at dump_addr:
//   [0..3]   magic (0xDEAD0001)
//   [4..7]   PC at crash
//   [8..11]  SP at crash
//   [12..15] RA at crash
//   [16..143] x0-x31 (32 * 4 bytes)
//   [144..147] cycle count (if available)

pub fn emit_crash_handler(emitter: &mut Emitter, dump_addr: u32) {
    emitter.label("__crash_handler");

    // store magic
    let addr = dump_addr as i32;
    let (a1, a2) = li32(T0, addr);
    emitter.emit32(a1);
    if let Some(a) = a2 {
        emitter.emit32(a);
    }
    let (m1, m2) = li32(T1, 0xDEAD0001u32 as i32);
    emitter.emit32(m1);
    if let Some(m) = m2 {
        emitter.emit32(m);
    }
    emitter.emit32(sw(T0, T1, 0));

    // store RA (return address = approximate crash PC)
    emitter.emit32(sw(T0, RA, 12));

    // store SP
    emitter.emit32(sw(T0, SP, 8));

    // store a0 (error code passed to panic)
    emitter.emit32(sw(T0, A0, 4));

    // disable interrupts and halt
    emitter.emit32(csrrci(ZERO, MSTATUS, 8));
    emitter.emit32(wfi());
    emitter.emit_jump(j_offset(0), "__crash_handler");
}

// host-side: decode a crash dump from memory
pub fn decode_crash_dump(data: &[u8]) -> Option<CrashDump> {
    if data.len() < 148 {
        return None;
    }
    let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if magic != 0xDEAD0001 {
        return None;
    }
    Some(CrashDump {
        error_code: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        sp: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        ra: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
    })
}

pub struct CrashDump {
    pub error_code: u32,
    pub sp: u32,
    pub ra: u32,
}

impl CrashDump {
    pub fn format(&self) -> String {
        format!(
            "crash dump:\n  error: {}\n  ra (return addr): {:#010X}\n  sp: {:#010X}",
            self.error_code, self.ra, self.sp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_handler_emits() {
        let mut emitter = Emitter::new();
        emit_crash_handler(&mut emitter, 0x20007F00);
        assert!(!emitter.code.is_empty());
        assert!(emitter.labels.contains_key("__crash_handler"));
    }

    #[test]
    fn decode_valid_dump() {
        let mut data = vec![0u8; 148];
        data[0..4].copy_from_slice(&0xDEAD0001u32.to_le_bytes());
        data[4..8].copy_from_slice(&42u32.to_le_bytes()); // error code
        data[8..12].copy_from_slice(&0x20007000u32.to_le_bytes()); // sp
        data[12..16].copy_from_slice(&0x42000100u32.to_le_bytes()); // ra

        let dump = decode_crash_dump(&data).unwrap();
        assert_eq!(dump.error_code, 42);
        assert_eq!(dump.sp, 0x20007000);
        assert_eq!(dump.ra, 0x42000100);
    }

    #[test]
    fn decode_invalid_magic() {
        let data = vec![0u8; 148];
        assert!(decode_crash_dump(&data).is_none());
    }
}
