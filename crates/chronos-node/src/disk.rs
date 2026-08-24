//! Real WAL: append + `FlushFileBuffers` (Windows) / `fdatasync` (Linux).
//! Same record bytes as sim. Spec: `docs/02-architecture.md` § Production node.
//!
//! Crash recover scans CRC-valid records in the file. That can include writes
//! the kernel made visible without fsync. Sim crash uses `durable_len` only.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use chronos_protocol::{scan, Effect, Event, IoError, IoOp};

pub struct FileDisk {
    file: File,
}

impl FileDisk {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Ok(Self { file })
    }

    /// Read the file, CRC-scan, truncate to `valid_len`, return that prefix.
    pub fn load_and_truncate(&mut self) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        self.file.read_to_end(&mut bytes)?;
        let (_records, valid_len) = scan(&bytes);
        self.file.set_len(valid_len as u64)?;
        self.file.seek(SeekFrom::Start(valid_len as u64))?;
        bytes.truncate(valid_len);
        Ok(bytes)
    }

    /// Every `IoSubmit` becomes an `IoComplete`. Syscall failure is `Err`, not an aborted loop.
    pub fn submit(&mut self, effects: &[Effect]) -> Vec<Event> {
        let mut completions = Vec::new();
        for effect in effects {
            match effect {
                Effect::IoSubmit {
                    id,
                    op: IoOp::Append { bytes },
                } => {
                    let result = append(&mut self.file, bytes);
                    completions.push(Event::IoComplete { id: *id, result });
                }
                Effect::IoSubmit {
                    id,
                    op: IoOp::Fsync,
                } => {
                    let result = match fsync(&self.file) {
                        Ok(()) => Ok(()),
                        Err(_) => Err(IoError::FsyncFailed),
                    };
                    completions.push(Event::IoComplete { id: *id, result });
                }
                Effect::ArmTimer { .. } | Effect::CancelTimer { .. } | Effect::Send { .. } => {}
                Effect::Reply { .. } => {}
            }
        }
        completions
    }
}

fn append(file: &mut File, bytes: &[u8]) -> Result<(), IoError> {
    let start = file.stream_position().map_err(|_| IoError::IoFailed)?;
    if file.write_all(bytes).is_err() {
        let _ = file.set_len(start);
        let _ = file.seek(SeekFrom::Start(start));
        return Err(IoError::IoFailed);
    }
    Ok(())
}

#[cfg(unix)]
fn fsync(file: &File) -> io::Result<()> {
    file.sync_data()
}

#[cfg(windows)]
fn fsync(file: &File) -> io::Result<()> {
    file.sync_all()
}

#[cfg(not(any(unix, windows)))]
fn fsync(file: &File) -> io::Result<()> {
    file.sync_all()
}
