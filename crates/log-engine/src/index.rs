//! Line-index data model and on-disk format.
//!
//! The engine doesn't *build* indexes (callers stream from disk and feed
//! `find_newlines` byte chunks). It owns the **format**: header layout,
//! magic, version, encode/decode. Defining it once here means a future
//! out-of-process server can read indexes written by the VSCode extension
//! and vice versa (RFC §5.4, §11.5).
//!
//! Format (little-endian):
//!
//! ```text
//! +-----------------+
//! | magic "LGIX"    |  4 B
//! | version u32     |  4 B
//! | stride u32      |  4 B
//! | total_lines u64 |  8 B
//! | file_size u64   |  8 B
//! | mtime_ms u64    |  8 B
//! | path_hash [16]B | 16 B   sha1(absoluteFsPath) prefix (sanity check)
//! | anchor_count u64|  8 B
//! | anchors[]: u64  |  anchor_count * 8 B
//! +-----------------+
//! ```

use memchr::memchr_iter;

pub const INDEX_MAGIC: &[u8; 4] = b"LGIX";
pub const INDEX_VERSION: u32 = 1;
/// Header bytes that precede the anchor array.
pub const INDEX_HEADER_SIZE: usize = 4 + 4 + 4 + 8 + 8 + 8 + 16 + 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexHeader {
    pub stride: u32,
    pub total_lines: u64,
    pub file_size: u64,
    pub mtime_ms: u64,
    /// First 16 bytes of sha1(absoluteFsPath). Sanity check only — not a
    /// security boundary.
    pub path_hash: [u8; 16],
    pub anchor_count: u64,
}

#[derive(Debug)]
pub enum DecodeError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u32),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "index file is truncated"),
            DecodeError::BadMagic => write!(f, "index file has bad magic"),
            DecodeError::UnsupportedVersion(v) => {
                write!(f, "unsupported index file version: {}", v)
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Serialize the header to its on-disk bytes.
pub fn encode_header(h: &IndexHeader) -> Vec<u8> {
    let mut out = Vec::with_capacity(INDEX_HEADER_SIZE);
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&INDEX_VERSION.to_le_bytes());
    out.extend_from_slice(&h.stride.to_le_bytes());
    out.extend_from_slice(&h.total_lines.to_le_bytes());
    out.extend_from_slice(&h.file_size.to_le_bytes());
    out.extend_from_slice(&h.mtime_ms.to_le_bytes());
    out.extend_from_slice(&h.path_hash);
    out.extend_from_slice(&h.anchor_count.to_le_bytes());
    debug_assert_eq!(out.len(), INDEX_HEADER_SIZE);
    out
}

/// Parse the header. Caller is responsible for reading
/// `anchor_count * 8` more bytes if it wants the anchor array.
pub fn decode_header(bytes: &[u8]) -> Result<IndexHeader, DecodeError> {
    if bytes.len() < INDEX_HEADER_SIZE {
        return Err(DecodeError::Truncated);
    }
    if &bytes[0..4] != INDEX_MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != INDEX_VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let stride = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let total_lines = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
    let file_size = u64::from_le_bytes(bytes[20..28].try_into().unwrap());
    let mtime_ms = u64::from_le_bytes(bytes[28..36].try_into().unwrap());
    let mut path_hash = [0u8; 16];
    path_hash.copy_from_slice(&bytes[36..52]);
    let anchor_count = u64::from_le_bytes(bytes[52..60].try_into().unwrap());
    Ok(IndexHeader {
        stride,
        total_lines,
        file_size,
        mtime_ms,
        path_hash,
        anchor_count,
    })
}

/// Return positions of every `\n` in `bytes`. Uses `memchr` SIMD.
pub fn find_newlines(bytes: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    find_newlines_into(bytes, &mut out);
    out
}

/// Same as `find_newlines` but appends into the caller's buffer (useful
/// when the caller maintains a reusable scratch vector across chunks).
pub fn find_newlines_into(bytes: &[u8], out: &mut Vec<u32>) {
    for off in memchr_iter(b'\n', bytes) {
        out.push(off as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn header_round_trips() {
        let h = IndexHeader {
            stride: 1024,
            total_lines: 12_345_678,
            file_size: 9_876_543_210,
            mtime_ms: 1_700_000_000_000,
            path_hash: [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            ],
            anchor_count: 12_064,
        };
        let bytes = encode_header(&h);
        assert_eq!(bytes.len(), INDEX_HEADER_SIZE);
        let back = decode_header(&bytes).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn header_rejects_truncated() {
        assert!(matches!(
            decode_header(&[0u8; 4]),
            Err(DecodeError::Truncated)
        ));
    }

    #[test]
    fn header_rejects_bad_magic() {
        let mut bytes = vec![0u8; INDEX_HEADER_SIZE];
        bytes[0] = b'X';
        assert!(matches!(decode_header(&bytes), Err(DecodeError::BadMagic)));
    }

    #[test]
    fn header_rejects_unknown_version() {
        let h = IndexHeader {
            stride: 1,
            total_lines: 0,
            file_size: 0,
            mtime_ms: 0,
            path_hash: [0u8; 16],
            anchor_count: 0,
        };
        let mut bytes = encode_header(&h);
        // Bump the version field to something we don't support.
        let bad_version: u32 = 9_999;
        bytes[4..8].copy_from_slice(&bad_version.to_le_bytes());
        assert!(matches!(
            decode_header(&bytes),
            Err(DecodeError::UnsupportedVersion(9_999))
        ));
    }

    #[test]
    fn find_newlines_basic() {
        let bytes = b"a\nbb\nccc\n";
        assert_eq!(find_newlines(bytes), vec![1u32, 4, 8]);
    }

    #[test]
    fn find_newlines_none() {
        assert!(find_newlines(b"no newline here").is_empty());
    }
}
