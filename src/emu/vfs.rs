// virtual filesystem for the emulator
// MMIO addresses that map to host file operations
// this lets Kov programs read/write files when running in the emulator

use std::collections::HashMap;

const VFS_BASE: u32 = 0xF000_0000;
const VFS_OPEN: u32 = VFS_BASE; // write: ptr to path → read: fd
const VFS_READ: u32 = VFS_BASE + 4; // write: fd | read: byte (-1 = EOF)
const VFS_WRITE: u32 = VFS_BASE + 8; // write: fd << 8 | byte
const VFS_CLOSE: u32 = VFS_BASE + 12; // write: fd
const VFS_READDIR: u32 = VFS_BASE + 16; // write: ptr to path → fills buffer
const VFS_STDOUT: u32 = VFS_BASE + 20; // write: byte → prints to stdout
const VFS_STRLEN: u32 = VFS_BASE + 24; // write: ptr → read: length

pub struct VirtualFS {
    pub files: HashMap<u32, FileState>,
    next_fd: u32,
    pub stdout: Vec<u8>,
    pub last_result: u32,
}

struct FileState {
    data: Vec<u8>,
    pos: usize,
}

impl VirtualFS {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            next_fd: 3, // 0=stdin, 1=stdout, 2=stderr
            stdout: Vec::new(),
            last_result: 0,
        }
    }

    pub fn is_vfs_addr(addr: u32) -> bool {
        addr >= VFS_BASE && addr < VFS_BASE + 0x100
    }

    // preload a file into the VFS (for testing or providing input)
    pub fn preload(&mut self, path: &str, data: Vec<u8>) -> u32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, FileState { data, pos: 0 });
        fd
    }

    pub fn handle_write(&mut self, addr: u32, value: u32, memory: &[u8]) {
        match addr {
            VFS_OPEN => {
                // value is pointer to null-terminated path in memory
                let path = read_cstring(memory, value);
                match std::fs::read(&path) {
                    Ok(data) => {
                        let fd = self.next_fd;
                        self.next_fd += 1;
                        self.files.insert(fd, FileState { data, pos: 0 });
                        self.last_result = fd;
                    }
                    Err(_) => {
                        self.last_result = 0; // 0 = error
                    }
                }
            }
            VFS_READ => {
                let fd = value;
                if let Some(file) = self.files.get_mut(&fd) {
                    if file.pos < file.data.len() {
                        self.last_result = file.data[file.pos] as u32;
                        file.pos += 1;
                    } else {
                        self.last_result = 0xFFFF_FFFF; // EOF
                    }
                } else {
                    self.last_result = 0xFFFF_FFFF;
                }
            }
            VFS_WRITE => {
                let fd = value >> 8;
                let byte = (value & 0xFF) as u8;
                if fd == 1 {
                    self.stdout.push(byte);
                }
                // for other fds, we'd append to a write buffer
            }
            VFS_CLOSE => {
                self.files.remove(&value);
            }
            VFS_STDOUT => {
                self.stdout.push(value as u8);
                if value as u8 == b'\n' {
                    let line = String::from_utf8_lossy(&self.stdout);
                    print!("{}", line);
                    self.stdout.clear();
                }
            }
            _ => {}
        }
    }

    pub fn handle_read(&self, addr: u32) -> u32 {
        match addr {
            VFS_OPEN | VFS_READ | VFS_STRLEN => self.last_result,
            _ => 0,
        }
    }
}

fn read_cstring(memory: &[u8], addr: u32) -> String {
    let mut s = Vec::new();
    let mut i = addr as usize;
    while i < memory.len() && memory[i] != 0 {
        s.push(memory[i]);
        i += 1;
    }
    String::from_utf8_lossy(&s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vfs_stdout() {
        let mut vfs = VirtualFS::new();
        vfs.handle_write(VFS_STDOUT, b'h' as u32, &[]);
        vfs.handle_write(VFS_STDOUT, b'i' as u32, &[]);
        assert_eq!(&vfs.stdout, b"hi");
    }

    #[test]
    fn vfs_preload_read() {
        let mut vfs = VirtualFS::new();
        let fd = vfs.preload("test.txt", b"abc".to_vec());
        vfs.handle_write(VFS_READ, fd, &[]);
        assert_eq!(vfs.last_result, b'a' as u32);
        vfs.handle_write(VFS_READ, fd, &[]);
        assert_eq!(vfs.last_result, b'b' as u32);
        vfs.handle_write(VFS_READ, fd, &[]);
        assert_eq!(vfs.last_result, b'c' as u32);
        vfs.handle_write(VFS_READ, fd, &[]);
        assert_eq!(vfs.last_result, 0xFFFF_FFFF); // EOF
    }

    #[test]
    fn vfs_addr_detection() {
        assert!(VirtualFS::is_vfs_addr(0xF000_0000));
        assert!(VirtualFS::is_vfs_addr(0xF000_0020));
        assert!(!VirtualFS::is_vfs_addr(0x2000_0000));
    }
}
