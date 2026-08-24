//! Structured `TraceRecord`s. Hash is SHA-256 of little-endian encodings. No host fields.
//!
//! Hand-rolled SHA-256 so sim does not need a crate with a build script (`link.exe`).
//! Spec: `docs/02-architecture.md` D13. Do not use `std::hash::Hasher`.

use chronos_protocol::codec::put_u64_le;
use chronos_protocol::Timestamp;

use crate::scheduler::WorldEvent;

pub struct Trace {
    records: Vec<Vec<u8>>,
}

impl Trace {
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    pub fn record(&mut self, time: Timestamp, seq: u64, event: &WorldEvent) {
        self.records.push(encode_world(time, seq, event));
    }

    pub fn digest(&self) -> [u8; 32] {
        sha256(&self.concat())
    }

    pub fn concat(&self) -> Vec<u8> {
        let mut all = Vec::new();
        for rec in &self.records {
            all.extend_from_slice(rec);
        }
        all
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self::new()
    }
}

pub fn digest_hex(digest: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn encode_world(time: Timestamp, seq: u64, event: &WorldEvent) -> Vec<u8> {
    let mut buf = Vec::new();
    put_u64_le(&mut buf, time.0);
    put_u64_le(&mut buf, seq);
    match event {
        WorldEvent::TimerFired {
            node,
            timer,
            generation,
        } => {
            buf.push(0);
            buf.push(node.0);
            put_u64_le(&mut buf, timer.0);
            put_u64_le(&mut buf, *generation);
        }
        WorldEvent::MessageDeliver {
            from,
            to,
            msg_id,
            msg: _,
        } => {
            buf.push(1);
            buf.push(from.0);
            buf.push(to.0);
            put_u64_le(&mut buf, msg_id.0);
        }
        WorldEvent::IoComplete {
            node,
            id,
            result,
            sync_len: _,
            life,
        } => {
            buf.push(2);
            buf.push(node.0);
            put_u64_le(&mut buf, id.incarnation);
            put_u64_le(&mut buf, id.local);
            buf.push(u8::from(result.is_ok()));
            put_u64_le(&mut buf, *life);
        }
        WorldEvent::Crash { node, torn_extra } => {
            buf.push(3);
            buf.push(node.0);
            put_u64_le(&mut buf, torn_extra.unwrap_or(u64::MAX));
        }
        WorldEvent::Partition {
            from,
            to,
            connected,
            asymmetric,
        } => {
            buf.push(4);
            buf.push(from.0);
            buf.push(to.0);
            buf.push(u8::from(*connected));
            buf.push(u8::from(*asymmetric));
        }
        WorldEvent::ClientInject { node, req } => {
            buf.push(5);
            buf.push(node.0);
            put_u64_le(&mut buf, req.client.0);
            put_u64_le(&mut buf, req.request.0);
        }
        WorldEvent::Recover { node } => {
            buf.push(6);
            buf.push(node.0);
        }
        WorldEvent::Dropped {
            from,
            to,
            msg_id,
            reason,
        } => {
            buf.push(7);
            buf.push(from.0);
            buf.push(to.0);
            put_u64_le(&mut buf, msg_id.0);
            buf.push(match reason {
                crate::scheduler::DropReason::Loss => 0,
                crate::scheduler::DropReason::Partition => 1,
            });
        }
        WorldEvent::FailNextFsync { node } => {
            buf.push(8);
            buf.push(node.0);
        }
    }
    buf
}

pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut padded = data.to_vec();
    let bit_len = (data.len() as u64).saturating_mul(8);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.as_chunks::<64>().0 {
        sha256_block(&mut h, chunk);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

fn sha256_block(h: &mut [u32; 8], chunk: &[u8]) {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut w = [0u32; 64];
    for i in 0..16 {
        let mut word = [0u8; 4];
        word.copy_from_slice(&chunk[i * 4..(i + 1) * 4]);
        w[i] = u32::from_be_bytes(word);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let mut a = h[0];
    let mut b = h[1];
    let mut c = h[2];
    let mut d = h[3];
    let mut e = h[4];
    let mut f = h[5];
    let mut g = h[6];
    let mut hh = h[7];
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

#[cfg(test)]
mod tests {
    use super::{sha256, Trace};
    use crate::scheduler::WorldEvent;
    use chronos_protocol::{NodeId, Timestamp};

    #[test]
    fn sha256_empty_known_vector() {
        assert_eq!(
            sha256(b""),
            [
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55,
            ]
        );
    }

    #[test]
    fn sha256_abc_known_vector() {
        assert_eq!(
            sha256(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn sha256_multiblock_known_vector() {
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            [
                0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e,
                0x60, 0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4,
                0x19, 0xdb, 0x06, 0xc1,
            ]
        );
    }

    #[test]
    fn same_records_same_digest() {
        let mut a = Trace::new();
        let mut b = Trace::new();
        let ev = WorldEvent::Crash {
            node: NodeId(0),
            torn_extra: Some(0),
        };
        a.record(Timestamp(1), 0, &ev);
        b.record(Timestamp(1), 0, &ev);
        assert_eq!(a.digest(), b.digest());
    }
}
