//! SmileBASIC 4 file container, as stored for TXT and DAT files.
//!
//! Layout, worked out from files saved by SmileBASIC 4:
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0x00 | 2 | format version (4) |
//! | 0x02 | 2 | kind: 0 text, 1 data |
//! | 0x04 | 2 | 1 if the body is zlib-compressed |
//! | 0x06 | 2 | unknown |
//! | 0x08 | 4 | stored body size |
//! | 0x0C | 8 | date: u16 year, then month, day, hour, minute, second, unknown |
//! | 0x14 | 0x5C | author names and other metadata |
//! | 0x70 | size | body |
//! | end | 20 | footer, probably a signature; not checked |
//!
//! Text bodies are UTF-8. Data bodies start with a `PCBN0004` header.

use std::fmt;

pub const HEADER_SIZE: usize = 0x70;
pub const FOOTER_SIZE: usize = 20;
/// Largest decompressed body accepted: a 2048x2048 page of 32-bit pixels
/// plus headroom, so a corrupt file cannot exhaust memory.
pub const MAX_BODY: usize = 64 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub kind: Kind,
    pub date: Date,
    /// Decompressed body.
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    TooShort,
    UnknownVersion(u16),
    UnknownKind(u16),
    Decompress,
    NotUtf8,
    BadData,
    /// A DAT element type this emulator does not read yet.
    UnsupportedType(u16),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::TooShort => write!(f, "file is truncated"),
            FileError::UnknownVersion(v) => write!(f, "unknown file version {v}"),
            FileError::UnknownKind(k) => write!(f, "unknown file kind {k}"),
            FileError::Decompress => write!(f, "compressed body is corrupt"),
            FileError::NotUtf8 => write!(f, "text is not valid UTF-8"),
            FileError::BadData => write!(f, "malformed DAT body"),
            FileError::UnsupportedType(t) => write!(f, "unsupported DAT element type {t}"),
        }
    }
}

impl std::error::Error for FileError {}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

pub fn parse(bytes: &[u8]) -> Result<File, FileError> {
    if bytes.len() < HEADER_SIZE {
        return Err(FileError::TooShort);
    }
    let version = u16_at(bytes, 0);
    if version != 4 {
        return Err(FileError::UnknownVersion(version));
    }
    let kind = match u16_at(bytes, 2) {
        0 => Kind::Text,
        1 => Kind::Data,
        k => return Err(FileError::UnknownKind(k)),
    };
    let compressed = u16_at(bytes, 4) == 1;
    let size = u32_at(bytes, 8) as usize;
    let stored = bytes
        .get(HEADER_SIZE..HEADER_SIZE.saturating_add(size))
        .ok_or(FileError::TooShort)?;
    let body = if compressed {
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(stored, MAX_BODY)
            .map_err(|_| FileError::Decompress)?
    } else {
        stored.to_vec()
    };
    let date = Date {
        year: u16_at(bytes, 0x0C),
        month: bytes[0x0E],
        day: bytes[0x0F],
        hour: bytes[0x10],
        minute: bytes[0x11],
        second: bytes[0x12],
    };
    Ok(File { kind, date, body })
}

impl File {
    pub fn text(&self) -> Result<&str, FileError> {
        std::str::from_utf8(&self.body).map_err(|_| FileError::NotUtf8)
    }

    pub fn data(&self) -> Result<Data, FileError> {
        parse_data(&self.body)
    }
}

/// Contents of a DAT file: an array with up to four dimensions.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    pub dims: Vec<usize>,
    pub values: DataValues,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataValues {
    /// Element type 4: 32-bit integers, also used for ARGB pixels.
    Int(Vec<i32>),
    /// Element type 5: doubles.
    Real(Vec<f64>),
}

const DATA_MAGIC: &[u8; 8] = b"PCBN0004";
const DATA_HEADER: usize = 28;

/// `PCBN0004`, u16 element type, u16 dimension count, u32 x 4 dimensions,
/// then the elements, little-endian.
pub fn parse_data(body: &[u8]) -> Result<Data, FileError> {
    if body.len() < DATA_HEADER || &body[..8] != DATA_MAGIC {
        return Err(FileError::BadData);
    }
    let elem = u16_at(body, 8);
    let ndims = usize::from(u16_at(body, 10));
    if !(1..=4).contains(&ndims) {
        return Err(FileError::BadData);
    }
    let dims: Vec<usize> = (0..ndims)
        .map(|i| u32_at(body, 12 + 4 * i) as usize)
        .collect();
    let count = dims
        .iter()
        .try_fold(1usize, |a, &d| a.checked_mul(d))
        .ok_or(FileError::BadData)?;
    let payload = &body[DATA_HEADER..];
    // UNVERIFIED: element types other than 4 and 5 (SB3 used 3 for 16-bit
    // pixels) have not been seen in SB4 files.
    let width = match elem {
        4 => 4,
        5 => 8,
        t => return Err(FileError::UnsupportedType(t)),
    };
    if payload.len() != count.checked_mul(width).ok_or(FileError::BadData)? {
        return Err(FileError::BadData);
    }
    let values = match elem {
        4 => DataValues::Int(
            payload
                .chunks_exact(4)
                .map(|c| i32::from_le_bytes(c.try_into().expect("4")))
                .collect(),
        ),
        _ => DataValues::Real(
            payload
                .chunks_exact(8)
                .map(|c| f64::from_le_bytes(c.try_into().expect("8")))
                .collect(),
        ),
    };
    Ok(Data { dims, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(kind: u16, compressed: bool, body: &[u8]) -> Vec<u8> {
        let stored = if compressed {
            miniz_oxide::deflate::compress_to_vec_zlib(body, 6)
        } else {
            body.to_vec()
        };
        let mut f = vec![0u8; HEADER_SIZE];
        f[0..2].copy_from_slice(&4u16.to_le_bytes());
        f[2..4].copy_from_slice(&kind.to_le_bytes());
        f[4..6].copy_from_slice(&u16::from(compressed).to_le_bytes());
        f[8..12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
        f[0x0C..0x0E].copy_from_slice(&2024u16.to_le_bytes());
        f[0x0E] = 5;
        f.extend_from_slice(&stored);
        f.extend_from_slice(&[0; FOOTER_SIZE]);
        f
    }

    fn dat(elem: u16, dims: &[u32], payload: &[u8]) -> Vec<u8> {
        let mut b = DATA_MAGIC.to_vec();
        b.extend_from_slice(&elem.to_le_bytes());
        b.extend_from_slice(&(dims.len() as u16).to_le_bytes());
        for i in 0..4 {
            b.extend_from_slice(&dims.get(i).copied().unwrap_or(0).to_le_bytes());
        }
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn text_file() {
        let f = parse(&container(0, false, "PRINT \"あ\"\n".as_bytes())).unwrap();
        assert_eq!(f.kind, Kind::Text);
        assert_eq!(f.text().unwrap(), "PRINT \"あ\"\n");
        assert_eq!((f.date.year, f.date.month), (2024, 5));
    }

    #[test]
    fn compressed_data_file() {
        let payload: Vec<u8> = [1i32, -1, 7, 8, 9, 10]
            .iter()
            .flat_map(|n| n.to_le_bytes())
            .collect();
        let f = parse(&container(1, true, &dat(4, &[2, 3], &payload))).unwrap();
        let d = f.data().unwrap();
        assert_eq!(d.dims, vec![2, 3]);
        assert_eq!(d.values, DataValues::Int(vec![1, -1, 7, 8, 9, 10]));
    }

    #[test]
    fn real_data() {
        let payload: Vec<u8> = [0.5f64, -2.0]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        let d = parse_data(&dat(5, &[2], &payload)).unwrap();
        assert_eq!(d.values, DataValues::Real(vec![0.5, -2.0]));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse(&[0; 10]), Err(FileError::TooShort));
        let mut f = container(0, false, b"ABC");
        f[8] = 200;
        assert_eq!(parse(&f), Err(FileError::TooShort));
        assert_eq!(
            parse(&container(0, true, b"x")[..HEADER_SIZE + 2]).unwrap_err(),
            FileError::TooShort
        );
        let mut bad = container(1, false, b"zz");
        bad[HEADER_SIZE] = b'x';
        assert_eq!(parse(&bad).unwrap().data().unwrap_err(), FileError::BadData);
        assert_eq!(
            parse_data(&dat(4, &[3], &[0; 8])).unwrap_err(),
            FileError::BadData
        );
        assert_eq!(
            parse_data(&dat(3, &[1], &[0; 2])).unwrap_err(),
            FileError::UnsupportedType(3)
        );
        assert_eq!(
            parse_data(&dat(4, &[u32::MAX, u32::MAX, u32::MAX], &[])).unwrap_err(),
            FileError::BadData
        );
    }

    #[test]
    fn garbage_never_panics() {
        let base = container(1, true, &dat(4, &[1], &[1, 2, 3, 4]));
        for i in 0..base.len() {
            for v in [0u8, 0xFF, 0x7F] {
                let mut b = base.clone();
                b[i] = v;
                if let Ok(f) = parse(&b) {
                    let _ = f.data();
                    let _ = f.text();
                }
            }
        }
    }
}
