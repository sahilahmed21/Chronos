//! Append-only Raft log and log-matching.
//!
//! Dummy entry at index 0. Spec: `docs/02-architecture.md` § Raft vs KV.

use crate::types::{ClientId, Index, RequestId, Term};
use crate::wal::record::{LogEntry, LogPayload};

#[derive(Clone, Debug)]
pub struct Log {
    /// `entries[0]` is the dummy at index 0 (term 0, NoOp).
    entries: Vec<LogEntry>,
}

impl Log {
    pub fn new() -> Self {
        Self {
            entries: vec![LogEntry {
                term: Term(0),
                payload: LogPayload::NoOp,
            }],
        }
    }

    pub fn last_index(&self) -> Index {
        Index(self.entries.len() as u64 - 1)
    }

    pub fn last_term(&self) -> Term {
        self.entries.last().map(|e| e.term).unwrap_or(Term(0))
    }

    pub fn term_at(&self, index: Index) -> Option<Term> {
        self.entry(index).map(|e| e.term)
    }

    pub fn entry(&self, index: Index) -> Option<&LogEntry> {
        self.entries.get(index.0 as usize)
    }

    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    pub fn matches(&self, prev_index: Index, prev_term: Term) -> bool {
        self.term_at(prev_index) == Some(prev_term)
    }

    pub fn append(&mut self, entry: LogEntry) -> Index {
        self.entries.push(entry);
        self.last_index()
    }

    /// Keep indexes `0..=index`. Returns whether the log actually shrank.
    pub fn truncate_after(&mut self, index: Index) -> bool {
        let keep = index.0 as usize + 1;
        if keep < self.entries.len() {
            self.entries.truncate(keep);
            true
        } else {
            false
        }
    }

    pub fn suffix_from(&self, index: Index) -> Vec<LogEntry> {
        let start = index.0 as usize;
        if start >= self.entries.len() {
            Vec::new()
        } else {
            self.entries[start..].to_vec()
        }
    }

    pub fn find_client(&self, client: ClientId, request: RequestId) -> Option<Index> {
        self.entries
            .iter()
            .enumerate()
            .find_map(|(i, e)| match &e.payload {
                LogPayload::Client {
                    client: c,
                    request: r,
                    ..
                } if *c == client && *r == request => Some(Index(i as u64)),
                _ => None,
            })
    }
}

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ClientId, Cmd};

    fn client_put(term: u64) -> LogEntry {
        LogEntry {
            term: Term(term),
            payload: LogPayload::Client {
                client: ClientId(1),
                request: RequestId(1),
                cmd: Cmd::Put {
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                },
            },
        }
    }

    #[test]
    fn dummy_at_index_zero() {
        let log = Log::new();
        assert_eq!(log.last_index(), Index(0));
        assert_eq!(log.last_term(), Term(0));
        assert!(log.matches(Index(0), Term(0)));
        assert!(!log.matches(Index(0), Term(1)));
        assert!(!log.matches(Index(1), Term(0)));
    }

    #[test]
    fn append_increments_index() {
        let mut log = Log::new();
        let i = log.append(LogEntry {
            term: Term(1),
            payload: LogPayload::NoOp,
        });
        assert_eq!(i, Index(1));
        assert_eq!(log.last_term(), Term(1));
        assert!(log.matches(Index(1), Term(1)));
    }

    #[test]
    fn truncate_conflict_keeps_prefix() {
        let mut log = Log::new();
        log.append(client_put(1));
        log.append(LogEntry {
            term: Term(1),
            payload: LogPayload::NoOp,
        });
        log.truncate_after(Index(1));
        assert_eq!(log.last_index(), Index(1));
        assert_eq!(log.term_at(Index(1)), Some(Term(1)));
        assert!(log.entry(Index(2)).is_none());
    }

    #[test]
    fn find_client_in_log() {
        let mut log = Log::new();
        log.append(client_put(1));
        assert_eq!(log.find_client(ClientId(1), RequestId(1)), Some(Index(1)));
        assert_eq!(log.find_client(ClientId(1), RequestId(2)), None);
    }
}
