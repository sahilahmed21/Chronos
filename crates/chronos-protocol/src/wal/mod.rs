//! Checksummed append-only WAL. Shared by sim disk and production disk.
//!
//! Spec: `docs/02-architecture.md` D9, D14, D17.

pub mod record;

pub use record::{decode_record, encode_record, scan, CodecError, LogEntry, LogPayload, WalRecord};
