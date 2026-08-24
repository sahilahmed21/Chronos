//! Record layout: `[u32 le payload_len][u32 le crc32][payload]`.
//! CRC-32/ISO-HDLC of `len_le || payload`. `Meta`, `Entry`, and `Truncate` records, never in-place.
//!
//! Spec: `docs/02-architecture.md` D9, D14, D17.

use crate::codec::{crc32, put_u32_le, put_u64_le, read_bytes, read_u32_le, read_u64_le, read_u8};
use crate::types::{ClientId, Cmd, Index, NodeId, RequestId, Term};

const TAG_META: u8 = 0;
const TAG_ENTRY: u8 = 1;
const TAG_TRUNCATE: u8 = 2;
const CMD_GET: u8 = 0;
const CMD_PUT: u8 = 1;
const VOTED_NONE: u8 = 0;
const VOTED_SOME: u8 = 1;
const PAYLOAD_NOOP: u8 = 0;
const PAYLOAD_CLIENT: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    Truncated,
    BadCrc,
    BadPayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogPayload {
    NoOp,
    Client {
        client: ClientId,
        request: RequestId,
        cmd: Cmd,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub term: Term,
    pub payload: LogPayload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalRecord {
    Meta {
        term: Term,
        voted_for: Option<NodeId>,
    },
    Entry(LogEntry),
    /// Raft suffix cut: after this record the log is indexes `0..=index`.
    Truncate {
        index: Index,
    },
}

pub fn encode_record(record: &WalRecord) -> Option<Vec<u8>> {
    let payload = encode_payload(record)?;
    let len = u32::try_from(payload.len()).ok()?;
    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(&len.to_le_bytes());
    crc_input.extend_from_slice(&payload);
    let crc = crc32(&crc_input);

    let mut out = Vec::with_capacity(8 + payload.len());
    put_u32_le(&mut out, len);
    put_u32_le(&mut out, crc);
    out.extend_from_slice(&payload);
    Some(out)
}

pub fn decode_record(bytes: &[u8]) -> Result<(WalRecord, usize), CodecError> {
    if bytes.len() < 8 {
        return Err(CodecError::Truncated);
    }
    let mut pos = 0;
    let len = read_u32_le(bytes, &mut pos).ok_or(CodecError::Truncated)?;
    let crc = read_u32_le(bytes, &mut pos).ok_or(CodecError::Truncated)?;
    let payload_len = len as usize;
    let total = 8usize
        .checked_add(payload_len)
        .ok_or(CodecError::Truncated)?;
    if bytes.len() < total {
        return Err(CodecError::Truncated);
    }
    let payload = &bytes[8..total];

    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.extend_from_slice(&len.to_le_bytes());
    crc_input.extend_from_slice(payload);
    if crc32(&crc_input) != crc {
        return Err(CodecError::BadCrc);
    }

    let record = decode_payload(payload)?;
    Ok((record, total))
}

/// Walk `bytes`. Stop at the first header that does not fit or whose CRC fails.
/// `valid_len` is the truncation point (end of the last good record).
pub fn scan(bytes: &[u8]) -> (Vec<WalRecord>, usize) {
    let mut records = Vec::new();
    let mut offset = 0;
    while let Ok((record, consumed)) = decode_record(&bytes[offset..]) {
        records.push(record);
        offset += consumed;
    }
    (records, offset)
}

pub fn encode_log_entry(entry: &LogEntry, buf: &mut Vec<u8>) -> Option<()> {
    put_u64_le(buf, entry.term.0);
    match &entry.payload {
        LogPayload::NoOp => buf.push(PAYLOAD_NOOP),
        LogPayload::Client {
            client,
            request,
            cmd,
        } => {
            buf.push(PAYLOAD_CLIENT);
            put_u64_le(buf, client.0);
            put_u64_le(buf, request.0);
            encode_cmd(buf, cmd)?;
        }
    }
    Some(())
}

pub fn decode_log_entry(data: &[u8], pos: &mut usize) -> Option<LogEntry> {
    let term = Term(read_u64_le(data, pos)?);
    let tag = read_u8(data, pos)?;
    let payload = match tag {
        PAYLOAD_NOOP => LogPayload::NoOp,
        PAYLOAD_CLIENT => {
            let client = ClientId(read_u64_le(data, pos)?);
            let request = RequestId(read_u64_le(data, pos)?);
            let cmd = decode_cmd(data, pos)?;
            LogPayload::Client {
                client,
                request,
                cmd,
            }
        }
        _ => return None,
    };
    Some(LogEntry { term, payload })
}

fn encode_cmd(buf: &mut Vec<u8>, cmd: &Cmd) -> Option<()> {
    match cmd {
        Cmd::Get { key } => {
            buf.push(CMD_GET);
            put_u32_le(buf, u32::try_from(key.len()).ok()?);
            buf.extend_from_slice(key);
        }
        Cmd::Put { key, value } => {
            buf.push(CMD_PUT);
            put_u32_le(buf, u32::try_from(key.len()).ok()?);
            buf.extend_from_slice(key);
            put_u32_le(buf, u32::try_from(value.len()).ok()?);
            buf.extend_from_slice(value);
        }
    }
    Some(())
}

fn decode_cmd(data: &[u8], pos: &mut usize) -> Option<Cmd> {
    let cmd_tag = read_u8(data, pos)?;
    match cmd_tag {
        CMD_GET => {
            let klen = read_u32_le(data, pos)? as usize;
            let key = read_bytes(data, pos, klen)?.to_vec();
            Some(Cmd::Get { key })
        }
        CMD_PUT => {
            let klen = read_u32_le(data, pos)? as usize;
            let key = read_bytes(data, pos, klen)?.to_vec();
            let vlen = read_u32_le(data, pos)? as usize;
            let value = read_bytes(data, pos, vlen)?.to_vec();
            Some(Cmd::Put { key, value })
        }
        _ => None,
    }
}

fn encode_payload(record: &WalRecord) -> Option<Vec<u8>> {
    let mut p = Vec::new();
    match record {
        WalRecord::Meta { term, voted_for } => {
            p.push(TAG_META);
            put_u64_le(&mut p, term.0);
            match voted_for {
                None => p.push(VOTED_NONE),
                Some(id) => {
                    p.push(VOTED_SOME);
                    p.push(id.0);
                }
            }
        }
        WalRecord::Entry(entry) => {
            p.push(TAG_ENTRY);
            encode_log_entry(entry, &mut p)?;
        }
        WalRecord::Truncate { index } => {
            p.push(TAG_TRUNCATE);
            put_u64_le(&mut p, index.0);
        }
    }
    Some(p)
}

fn decode_payload(payload: &[u8]) -> Result<WalRecord, CodecError> {
    let mut pos = 0;
    let tag = read_u8(payload, &mut pos).ok_or(CodecError::BadPayload)?;
    match tag {
        TAG_META => {
            let term = Term(read_u64_le(payload, &mut pos).ok_or(CodecError::BadPayload)?);
            let flag = read_u8(payload, &mut pos).ok_or(CodecError::BadPayload)?;
            let voted_for = match flag {
                VOTED_NONE => None,
                VOTED_SOME => Some(NodeId(
                    read_u8(payload, &mut pos).ok_or(CodecError::BadPayload)?,
                )),
                _ => return Err(CodecError::BadPayload),
            };
            if pos != payload.len() {
                return Err(CodecError::BadPayload);
            }
            Ok(WalRecord::Meta { term, voted_for })
        }
        TAG_ENTRY => {
            let entry = decode_log_entry(payload, &mut pos).ok_or(CodecError::BadPayload)?;
            if pos != payload.len() {
                return Err(CodecError::BadPayload);
            }
            Ok(WalRecord::Entry(entry))
        }
        TAG_TRUNCATE => {
            let index = Index(read_u64_le(payload, &mut pos).ok_or(CodecError::BadPayload)?);
            if pos != payload.len() {
                return Err(CodecError::BadPayload);
            }
            Ok(WalRecord::Truncate { index })
        }
        _ => Err(CodecError::BadPayload),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::put_u32_le;
    use crate::types::{ClientId, Index, NodeId, RequestId, Term};

    fn meta() -> WalRecord {
        WalRecord::Meta {
            term: Term(3),
            voted_for: Some(NodeId(2)),
        }
    }

    fn entry_put() -> WalRecord {
        WalRecord::Entry(LogEntry {
            term: Term(1),
            payload: LogPayload::Client {
                client: ClientId(1),
                request: RequestId(7),
                cmd: Cmd::Put {
                    key: b"k".to_vec(),
                    value: b"v".to_vec(),
                },
            },
        })
    }

    fn entry_noop() -> WalRecord {
        WalRecord::Entry(LogEntry {
            term: Term(4),
            payload: LogPayload::NoOp,
        })
    }

    #[test]
    fn scan_empty() {
        let (records, valid_len) = scan(&[]);
        assert!(records.is_empty());
        assert_eq!(valid_len, 0);
    }

    #[test]
    fn scan_one_meta() {
        let bytes = encode_record(&meta()).unwrap();
        let (records, valid_len) = scan(&bytes);
        assert_eq!(records, vec![meta()]);
        assert_eq!(valid_len, bytes.len());
    }

    #[test]
    fn scan_two_records() {
        let mut bytes = encode_record(&meta()).unwrap();
        bytes.extend_from_slice(&encode_record(&entry_put()).unwrap());
        let (records, valid_len) = scan(&bytes);
        assert_eq!(records, vec![meta(), entry_put()]);
        assert_eq!(valid_len, bytes.len());
    }

    #[test]
    fn scan_torn_last_three_bytes() {
        let mut bytes = encode_record(&meta()).unwrap();
        let good = bytes.len();
        bytes.extend_from_slice(&[1, 2, 3]);
        let (records, valid_len) = scan(&bytes);
        assert_eq!(records, vec![meta()]);
        assert_eq!(valid_len, good);
    }

    #[test]
    fn scan_flipped_crc() {
        let mut bytes = encode_record(&meta()).unwrap();
        bytes[4] ^= 0xFF;
        let (records, valid_len) = scan(&bytes);
        assert!(records.is_empty());
        assert_eq!(valid_len, 0);
    }

    #[test]
    fn scan_huge_len_does_not_fit() {
        let mut bytes = Vec::new();
        put_u32_le(&mut bytes, u32::MAX);
        put_u32_le(&mut bytes, 0);
        bytes.extend_from_slice(&[0, 1, 2, 3]);
        let (records, valid_len) = scan(&bytes);
        assert!(records.is_empty());
        assert_eq!(valid_len, 0);
    }

    #[test]
    fn roundtrip_meta_none() {
        let rec = WalRecord::Meta {
            term: Term(0),
            voted_for: None,
        };
        let bytes = encode_record(&rec).unwrap();
        let (got, n) = decode_record(&bytes).unwrap();
        assert_eq!(got, rec);
        assert_eq!(n, bytes.len());
    }

    #[test]
    fn roundtrip_entry_noop() {
        let rec = entry_noop();
        let bytes = encode_record(&rec).unwrap();
        let (got, n) = decode_record(&bytes).unwrap();
        assert_eq!(got, rec);
        assert_eq!(n, bytes.len());
    }

    #[test]
    fn scan_truncate_drops_suffix_on_recover_order() {
        let rec = WalRecord::Truncate { index: Index(0) };
        let bytes = encode_record(&rec).unwrap();
        let (got, n) = decode_record(&bytes).unwrap();
        assert_eq!(got, rec);
        assert_eq!(n, bytes.len());

        let mut wal = encode_record(&entry_put()).unwrap();
        wal.extend_from_slice(&encode_record(&WalRecord::Truncate { index: Index(0) }).unwrap());
        wal.extend_from_slice(&encode_record(&entry_noop()).unwrap());
        let (records, valid_len) = scan(&wal);
        assert_eq!(valid_len, wal.len());
        assert_eq!(
            records,
            vec![
                entry_put(),
                WalRecord::Truncate { index: Index(0) },
                entry_noop(),
            ]
        );
    }
}
