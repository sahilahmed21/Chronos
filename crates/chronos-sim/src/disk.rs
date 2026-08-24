//! Simulated disk: `bytes` vs `durable_len`.
//!
//! P1 `submit`/`pop` stay delay-0 VecDeque for `drive()`. P2 Cluster uses
//! `apply_op` + `complete` and schedules completions on the heap.
//! Crash: keep durable prefix; optional torn suffix past `durable_len`.
//! Spec: `docs/02-architecture.md` § Disk.

use std::collections::VecDeque;

use chronos_protocol::{scan, Effect, Event, IoError, IoId, IoOp};

use crate::rng::Rng;

struct Queued {
    id: IoId,
    result: Result<(), IoError>,
    /// Set for Fsync: length-at-submit. Applied to `durable_len` when popped Ok.
    sync_len: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
pub struct PendingIo {
    pub id: IoId,
    pub result: Result<(), IoError>,
    pub sync_len: Option<usize>,
}

#[derive(Default)]
pub struct SimDisk {
    pub bytes: Vec<u8>,
    pub durable_len: usize,
    pub fail_next_fsync: bool,
    /// D10: Fsync completes Ok but `durable_len` does not move. Default off.
    pub fsync_ok_but_not_durable: bool,
    completions: VecDeque<Queued>,
}

impl SimDisk {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn durable_prefix(&self) -> &[u8] {
        let n = self.durable_len.min(self.bytes.len());
        &self.bytes[..n]
    }

    /// Honest crash with no lucky tail: drop in-flight I/O, keep durable prefix.
    pub fn crash(&mut self) {
        self.tear(0);
    }

    /// Seed-chosen prefix of `bytes[durable_len..]`. Does not shrink `durable_len`.
    pub fn crash_torn(&mut self, rng: &mut Rng) {
        let tail = self.bytes.len().saturating_sub(self.durable_len);
        let extra = if tail == 0 {
            0
        } else {
            rng.delay_ns(0, tail as u64) as usize
        };
        self.tear(extra);
    }

    /// Deterministic torn suffix length for scripted tests.
    pub fn crash_torn_len(&mut self, extra: usize) {
        self.tear(extra);
    }

    fn tear(&mut self, extra: usize) {
        let cap = self.bytes.len().saturating_sub(self.durable_len);
        let keep = extra.min(cap);
        self.bytes.truncate(self.durable_len.saturating_add(keep));
        self.completions.clear();
        self.fail_next_fsync = false;
    }

    /// CRC scan, truncate to last good record, set `durable_len = valid_len`.
    pub fn recover_scan(&mut self) -> Vec<u8> {
        let (_, valid_len) = scan(&self.bytes);
        self.bytes.truncate(valid_len);
        self.durable_len = valid_len;
        self.bytes.clone()
    }

    /// Append mutates `bytes` immediately. Fsync snapshots `bytes.len()`.
    pub fn apply_op(&mut self, id: IoId, op: &IoOp) -> PendingIo {
        match op {
            IoOp::Append { bytes } => {
                self.bytes.extend_from_slice(bytes);
                PendingIo {
                    id,
                    result: Ok(()),
                    sync_len: None,
                }
            }
            IoOp::Fsync => {
                let fail = self.fail_next_fsync;
                self.fail_next_fsync = false;
                PendingIo {
                    id,
                    result: if fail {
                        Err(IoError::FsyncFailed)
                    } else {
                        Ok(())
                    },
                    sync_len: Some(self.bytes.len()),
                }
            }
        }
    }

    pub fn complete(&mut self, sync_len: Option<usize>, result: Result<(), IoError>) {
        if let (Some(sync_len), Ok(())) = (sync_len, result) {
            if !self.fsync_ok_but_not_durable {
                self.durable_len = self.durable_len.max(sync_len);
            }
        }
    }

    /// Interpret effects in emission order. Completions queued at delay 0.
    pub fn submit(&mut self, effects: &[Effect]) {
        for effect in effects {
            match effect {
                Effect::IoSubmit { id, op } => {
                    let p = self.apply_op(*id, op);
                    self.completions.push_back(Queued {
                        id: p.id,
                        result: p.result,
                        sync_len: p.sync_len,
                    });
                }
                Effect::ArmTimer { .. } | Effect::CancelTimer { .. } | Effect::Send { .. } => {}
                Effect::Reply { .. } => {}
            }
        }
    }

    pub fn pop(&mut self) -> Option<Event> {
        let q = self.completions.pop_front()?;
        self.complete(q.sync_len, q.result);
        Some(Event::IoComplete {
            id: q.id,
            result: q.result,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SimDisk;
    use chronos_protocol::{
        encode_record, scan, IoId, IoOp, LogEntry, LogPayload, Term, WalRecord,
    };

    fn id(local: u64) -> IoId {
        IoId {
            incarnation: 1,
            local,
        }
    }

    fn entry_bytes() -> Vec<u8> {
        encode_record(&WalRecord::Entry(LogEntry {
            term: Term(1),
            payload: LogPayload::NoOp,
        }))
        .unwrap()
    }

    #[test]
    fn crash_before_complete_loses_unsynced_tail() {
        let mut disk = SimDisk::new();
        disk.apply_op(
            id(0),
            &IoOp::Append {
                bytes: b"abc".to_vec(),
            },
        );
        assert_eq!(disk.bytes, b"abc");
        assert_eq!(disk.durable_len, 0);
        disk.crash();
        assert!(disk.bytes.is_empty());
        assert!(disk.durable_prefix().is_empty());
    }

    #[test]
    fn fsync_ok_complete_advances_durable_len() {
        let mut disk = SimDisk::new();
        disk.apply_op(
            id(0),
            &IoOp::Append {
                bytes: b"xyz".to_vec(),
            },
        );
        let fsync = disk.apply_op(id(1), &IoOp::Fsync);
        assert_eq!(disk.durable_len, 0);
        disk.complete(fsync.sync_len, fsync.result);
        assert_eq!(disk.durable_len, 3);
        assert_eq!(disk.durable_prefix(), b"xyz");
    }

    #[test]
    fn two_appends_keep_emission_order() {
        let mut disk = SimDisk::new();
        disk.apply_op(
            id(0),
            &IoOp::Append {
                bytes: b"ab".to_vec(),
            },
        );
        disk.apply_op(
            id(1),
            &IoOp::Append {
                bytes: b"cd".to_vec(),
            },
        );
        assert_eq!(disk.bytes, b"abcd");
    }

    #[test]
    fn torn_mid_record_does_not_shrink_durable_len() {
        let rec = entry_bytes();
        let mut disk = SimDisk::new();
        disk.apply_op(id(0), &IoOp::Append { bytes: rec.clone() });
        let fsync = disk.apply_op(id(1), &IoOp::Fsync);
        disk.complete(fsync.sync_len, Ok(()));
        let durable = disk.durable_len;
        assert!(durable > 0);
        disk.apply_op(id(2), &IoOp::Append { bytes: rec.clone() });
        disk.crash_torn_len(3);
        assert_eq!(disk.durable_len, durable);
        assert!(disk.bytes.len() > durable);
        let recovered = disk.recover_scan();
        assert_eq!(recovered.len(), durable);
        assert_eq!(disk.durable_len, durable);
        let (records, valid) = scan(&recovered);
        assert_eq!(valid, durable);
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn fsync_lie_ok_does_not_advance_durable_len() {
        let mut disk = SimDisk::new();
        disk.fsync_ok_but_not_durable = true;
        disk.apply_op(
            id(0),
            &IoOp::Append {
                bytes: b"xyz".to_vec(),
            },
        );
        let fsync = disk.apply_op(id(1), &IoOp::Fsync);
        assert!(fsync.result.is_ok());
        disk.complete(fsync.sync_len, fsync.result);
        assert_eq!(disk.durable_len, 0);
        disk.crash();
        assert!(disk.bytes.is_empty());
    }

    #[test]
    fn crash_clears_fail_next_fsync() {
        let mut disk = SimDisk::new();
        disk.fail_next_fsync = true;
        disk.crash();
        assert!(
            !disk.fail_next_fsync,
            "fsync-err must not leak into the next incarnation"
        );
    }
}
