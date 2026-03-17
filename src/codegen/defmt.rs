use super::emit::Emitter;
use super::encode::*;
use std::collections::HashMap;

// deferred formatting: format strings never touch the device.
// log!("temp: {}", value) compiles to:
//   1. store format string index (u8) to log buffer
//   2. store raw argument values to log buffer
//   3. bump log write pointer
// host reads the buffer, matches index to format string table, reconstructs message.

pub struct DefmtTable {
    pub strings: Vec<String>,
    string_map: HashMap<String, u8>,
}

impl DefmtTable {
    pub fn new() -> Self {
        Self {
            strings: Vec::new(),
            string_map: HashMap::new(),
        }
    }

    pub fn intern(&mut self, fmt: &str) -> u8 {
        if let Some(&id) = self.string_map.get(fmt) {
            return id;
        }
        let id = self.strings.len() as u8;
        self.string_map.insert(fmt.to_string(), id);
        self.strings.push(fmt.to_string());
        id
    }

    // generate the format string table for the host tool
    pub fn to_json(&self) -> String {
        let mut out = String::from("[");
        for (i, s) in self.strings.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{}\"", s.replace('"', "\\\"")));
        }
        out.push(']');
        out
    }
}

// emit log buffer infrastructure into the firmware
pub fn emit_log_buffer(emitter: &mut Emitter, buffer_addr: u32, buffer_size: u32) {
    // log buffer layout:
    //   [0..3]   write pointer (u32)
    //   [4..N]   ring buffer of log entries
    // each entry: [format_id: u8] [arg_count: u8] [args: u32...]

    emitter.label("__log_buffer");
    emitter.label("log_write");

    // a0 = format string id (u8)
    // a1 = arg0 (u32)
    // a2 = arg1 (u32) (optional)

    // load write pointer
    let addr = buffer_addr as i32;
    let (i1, i2) = li32(T0, addr);
    emitter.emit32(i1);
    if let Some(i) = i2 {
        emitter.emit32(i);
    }
    emitter.emit32(lw(T1, T0, 0)); // t1 = write ptr

    // store format id
    emitter.emit32(sb(T1, A0, 0));
    emitter.emit32(addi(T1, T1, 1));

    // store arg0
    emitter.emit32(sw(T1, A1, 0));
    emitter.emit32(addi(T1, T1, 4));

    // wrap if past end
    let end = (buffer_addr + buffer_size) as i32;
    let (e1, e2) = li32(T2, end);
    emitter.emit32(e1);
    if let Some(e) = e2 {
        emitter.emit32(e);
    }
    emitter.emit_branch(blt(T1, T2, 0), "_log_no_wrap");
    // wrap: reset to start + 4 (skip write pointer)
    let (s1, s2) = li32(T1, addr + 4);
    emitter.emit32(s1);
    if let Some(s) = s2 {
        emitter.emit32(s);
    }

    emitter.label("_log_no_wrap");
    // update write pointer
    emitter.emit32(sw(T0, T1, 0));
    emitter.emit32(ret());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_strings() {
        let mut table = DefmtTable::new();
        let id1 = table.intern("temp: {}");
        let id2 = table.intern("humidity: {}");
        let id3 = table.intern("temp: {}"); // duplicate
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 0); // same as first
        assert_eq!(table.strings.len(), 2);
    }

    #[test]
    fn json_table() {
        let mut table = DefmtTable::new();
        table.intern("hello: {}");
        table.intern("world: {}");
        let json = table.to_json();
        assert!(json.contains("hello"));
        assert!(json.contains("world"));
    }

    #[test]
    fn log_buffer_emits() {
        let mut emitter = Emitter::new();
        emit_log_buffer(&mut emitter, 0x20006000, 256);
        assert!(!emitter.code.is_empty());
        assert!(emitter.labels.contains_key("log_write"));
    }
}
