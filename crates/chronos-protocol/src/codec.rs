//! Little-endian integer helpers and CRC-32/ISO-HDLC (zlib poly `0xEDB88320`).
//!
//! Spec: `docs/02-architecture.md` D14. No serde. No `f64`.

const fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = 0xEDB88320 ^ (crc >> 1);
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC32_TABLE: [u32; 256] = crc32_table();

/// CRC-32/ISO-HDLC of `data`. Init and xor-out `0xFFFFFFFF`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let idx = ((crc ^ u32::from(b)) & 0xFF) as usize;
        crc = CRC32_TABLE[idx] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

pub fn put_u32_le(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u64_le(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}

pub fn read_u32_le(data: &[u8], pos: &mut usize) -> Option<u32> {
    let end = pos.checked_add(4)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

pub fn read_u64_le(data: &[u8], pos: &mut usize) -> Option<u64> {
    let end = pos.checked_add(8)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

pub fn read_u8(data: &[u8], pos: &mut usize) -> Option<u8> {
    let b = *data.get(*pos)?;
    *pos += 1;
    Some(b)
}

pub fn read_bytes<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = pos.checked_add(n)?;
    let slice = data.get(*pos..end)?;
    *pos = end;
    Some(slice)
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn crc32_empty_matches_zlib() {
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn crc32_known_vector() {
        // ISO-HDLC / zlib CRC of "123456789" is 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
