//! SmileBASIC 4's file container, and the `PCBN` arrays inside DAT and
//! GRP files. docs/file-formats.md describes both, from files SmileBASIC 4
//! saved.
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0x00 | 2 | format version (4) |
//! | 0x02 | 2 | [`Kind`] |
//! | 0x04 | 2 | 1 if the body is zlib-compressed |
//! | 0x06 | 2 | a number that depends on the kind; see [`usage`] |
//! | 0x08 | 4 | stored body size |
//! | 0x0C | 8 | UTC date: u16 year, then month, day, hour, minute, second, 0xFF |
//! | 0x14 | 0x5C | author names, zero in files saved by programs |
//! | 0x70 | size | body |
//! | end | 20 | HMAC-SHA1 of everything before it ([`container`]) |
//!
//! META files have the same header but no footer.

use std::fmt;

use hmac::{Hmac, Mac};
use sha1::Sha1;

pub const HEADER_SIZE: usize = 0x70;
pub const FOOTER_SIZE: usize = 20;
/// Largest decompressed body accepted, so a corrupt file cannot exhaust
/// memory. Real arrays go well past a 2048x2048 page: IRCAMERA's demos hold
/// 115 MB.
pub const MAX_BODY: usize = 256 << 20;
/// The key of the footer's HMAC, the one SmileBASIC 3 used too.
const SIGNING_KEY: &[u8] = b"nqmby+e9S?{%U*-V]51n%^xZMk8>b{?x]&?(NmmV[,g85:%6Sqd\"'U\")/8u77UL2";

/// What a file is, by the number at 0x02.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    /// A DAT array. Older GRP files (the ones in SmileBASIC 4's own
    /// projects) are Data with [`usage::OLD_GRP`].
    Data,
    Grp,
    Meta,
    Jpg,
}

impl Kind {
    fn code(self) -> u16 {
        match self {
            Kind::Text => 0,
            Kind::Data => 1,
            Kind::Grp => 2,
            Kind::Meta => 4,
            Kind::Jpg => 5,
        }
    }
}

/// The numbers at 0x06 that SmileBASIC 4.4.9 writes.
pub mod usage {
    /// A text file from SAVEV.
    pub const TEXT: u16 = 0;
    /// A program from SAVE.
    pub const PROGRAM: u16 = 2;
    pub const DAT: u16 = 1;
    pub const GRP: u16 = 3;
    pub const JPG: u16 = 3;
    /// A GRP image stored as Data, as SmileBASIC 4's own projects have.
    pub const OLD_GRP: u16 = 2;
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

impl Date {
    /// The time now, for files being written.
    pub fn now() -> Date {
        Date::at(std::time::SystemTime::now())
    }

    /// `time` in UTC, which files hold (checked on hardware: SmileBASIC 4
    /// shows them in local time).
    pub fn at(time: std::time::SystemTime) -> Date {
        use chrono::{Datelike, Timelike};
        let t = chrono::DateTime::<chrono::Utc>::from(time);
        Date {
            year: t.year() as u16,
            month: t.month() as u8,
            day: t.day() as u8,
            hour: t.hour() as u8,
            minute: t.minute() as u8,
            second: t.second() as u8,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub kind: Kind,
    pub date: Date,
    /// The number at 0x06; see [`usage`].
    pub usage: u16,
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

/// Whether `bytes` start like a SmileBASIC 4 container.
pub fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(&[4, 0])
}

/// Reads a container. The footer is not checked.
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
        2 => Kind::Grp,
        4 => Kind::Meta,
        5 => Kind::Jpg,
        k => return Err(FileError::UnknownKind(k)),
    };
    let compressed = u16_at(bytes, 4) == 1;
    let size = u32_at(bytes, 8) as usize;
    let stored = bytes
        .get(HEADER_SIZE..HEADER_SIZE.saturating_add(size))
        .ok_or(FileError::TooShort)?;
    // Arrays are read by their own sizes. Some files' size leaves out the
    // array's header, so it goes on to the signature.
    let whole_array = !compressed && kind == Kind::Data && stored.starts_with(b"PCBN000");
    let stored = match bytes.get(HEADER_SIZE..bytes.len().saturating_sub(FOOTER_SIZE)) {
        Some(all) if whole_array && all.len() > stored.len() => all,
        _ => stored,
    };
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
    Ok(File {
        kind,
        date,
        usage: u16_at(bytes, 6),
        body,
    })
}

/// A container around `body`, as SmileBASIC 4 writes it: compressed if
/// asked, and signed unless it is META.
pub fn container(kind: Kind, usage: u16, date: Date, body: &[u8], compress: bool) -> Vec<u8> {
    let stored = match compress {
        true => miniz_oxide::deflate::compress_to_vec_zlib(body, 6),
        false => body.to_vec(),
    };
    let mut f = vec![0u8; HEADER_SIZE];
    f[0..2].copy_from_slice(&4u16.to_le_bytes());
    f[2..4].copy_from_slice(&kind.code().to_le_bytes());
    f[4..6].copy_from_slice(&u16::from(compress).to_le_bytes());
    f[6..8].copy_from_slice(&usage.to_le_bytes());
    f[8..12].copy_from_slice(&(stored.len() as u32).to_le_bytes());
    f[0x0C..0x0E].copy_from_slice(&date.year.to_le_bytes());
    f[0x0E..0x14].copy_from_slice(&[
        date.month,
        date.day,
        date.hour,
        date.minute,
        date.second,
        0xFF,
    ]);
    f.extend_from_slice(&stored);
    if kind != Kind::Meta {
        let signature = sign(&f);
        f.extend_from_slice(&signature);
    }
    f
}

/// The footer for a file whose header and body are `bytes`.
pub fn sign(bytes: &[u8]) -> [u8; FOOTER_SIZE] {
    let mut mac = Hmac::<Sha1>::new_from_slice(SIGNING_KEY).expect("any key length");
    mac.update(bytes);
    mac.finalize().into_bytes().into()
}

impl File {
    pub fn text(&self) -> Result<&str, FileError> {
        std::str::from_utf8(&self.body).map_err(|_| FileError::NotUtf8)
    }

    pub fn data(&self) -> Result<Data, FileError> {
        parse_data(&self.body)
    }

    /// Whether the body is an image's `PCBN` array, stored width first.
    pub fn is_image(&self) -> bool {
        self.kind == Kind::Grp || (self.kind == Kind::Data && self.usage == usage::OLD_GRP)
    }
}

/// The array in a DAT or GRP file, which may be in a container or be the
/// bare `PCBN` body the emulator saves; with whether it is an image,
/// stored width first.
pub fn read_data(bytes: &[u8]) -> Result<(Data, bool), FileError> {
    if bytes.starts_with(DATA_MAGIC) {
        return Ok((parse_data(bytes)?, false));
    }
    let file = parse(bytes)?;
    Ok((file.data()?, file.is_image()))
}

/// An array as DAT and GRP files keep it, with up to four dimensions.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    /// As stored: outermost first for a DAT file, as `DIM` takes them,
    /// but width then height for a GRP image.
    pub dims: Vec<usize>,
    pub values: DataValues,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataValues {
    /// Element type 4: 32-bit integers, also used for ARGB pixels.
    Int(Vec<i32>),
    /// Element type 5: doubles.
    Real(Vec<f64>),
    /// Element type 6: each string's length in UTF-8 bytes, then its
    /// UTF-8 text. A length under 128 takes one byte; a longer one two,
    /// big-endian with the top bit set (300 is `81 2C`).
    Str(Vec<Vec<u16>>),
}

const STR_TYPE: u16 = 6;
/// SmileBASIC 3's 16-bit pixels, in GRP files SmileBASIC 4 converted.
const PIXEL16_TYPE: u16 = 3;

/// A 16-bit pixel, 5 bits each of red, green and blue from the top and
/// the lowest bit opaque, as an ARGB colour. Each 5 bits become the top
/// of 8, the low 3 left 0 (checked on hardware: 31 is &HF8).
fn argb_from_5551(p: u16) -> i32 {
    let wide = |v: u16| u32::from(v & 31) << 3;
    let alpha = if p & 1 == 1 { 0xFF00_0000 } else { 0 };
    (alpha | wide(p >> 11) << 16 | wide(p >> 6) << 8 | wide(p >> 1)) as i32
}

pub const DATA_MAGIC: &[u8; 8] = b"PCBN0004";
pub(crate) const DATA_HEADER: usize = 28;
/// The longest string a DAT file can hold, in UTF-8 bytes.
pub const MAX_STR_BYTES: usize = 0x7FFF;
/// The most elements an array in a file holds, as for DIM.
const MAX_ELEMENTS: usize = 1 << 28;

/// `PCBN0004`, u16 element type, u16 dimension count, u32 x 4 dimensions
/// (unused ones may hold anything), then the elements, little-endian.
pub fn parse_data(body: &[u8]) -> Result<Data, FileError> {
    // Files SmileBASIC 4 converted from SmileBASIC 3 say PCBN0001.
    if body.len() < DATA_HEADER || !body.starts_with(b"PCBN000") {
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
    let count = checked_elements(&dims)?;
    let payload = &body[DATA_HEADER..];
    let width = match elem {
        PIXEL16_TYPE => 2,
        4 => 4,
        5 => 8,
        STR_TYPE => return parse_strings(payload, count).map(|v| Data { dims, values: v }),
        t => return Err(FileError::UnsupportedType(t)),
    };
    if payload.len() != count.checked_mul(width).ok_or(FileError::BadData)? {
        return Err(FileError::BadData);
    }
    let values = match elem {
        PIXEL16_TYPE => DataValues::Int(
            payload
                .chunks_exact(2)
                .map(|c| argb_from_5551(u16::from_le_bytes([c[0], c[1]])))
                .collect(),
        ),
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

/// How many elements an array of these sizes holds. A zero in any size
/// makes it empty; the others must still multiply to no more than an array
/// can hold, so that nothing later goes through `[0xFFFFFFFF, 0]`'s rows.
// Our own guard, for a file no SmileBASIC 4 program writes.
fn checked_elements(dims: &[usize]) -> Result<usize, FileError> {
    let mut product = 1usize;
    for &d in dims.iter().filter(|&&d| d != 0) {
        product = product
            .checked_mul(d)
            .filter(|&t| t <= MAX_ELEMENTS)
            .ok_or(FileError::BadData)?;
    }
    Ok(if dims.contains(&0) { 0 } else { product })
}

fn parse_strings(mut payload: &[u8], count: usize) -> Result<DataValues, FileError> {
    let mut items = Vec::new();
    for _ in 0..count {
        let (&first, mut rest) = payload.split_first().ok_or(FileError::BadData)?;
        let mut len = usize::from(first);
        if first & 0x80 != 0 {
            let (&second, after) = rest.split_first().ok_or(FileError::BadData)?;
            len = usize::from(first & 0x7F) << 8 | usize::from(second);
            rest = after;
        }
        let text = rest.get(..len).ok_or(FileError::BadData)?;
        items.push(String::from_utf8_lossy(text).encode_utf16().collect());
        payload = &rest[len..];
    }
    if !payload.is_empty() {
        return Err(FileError::BadData);
    }
    Ok(DataValues::Str(items))
}

/// The `PCBN` body holding `data`. Strings longer than a DAT file holds
/// are cut.
pub fn data_body(data: &Data) -> Vec<u8> {
    let mut b = DATA_MAGIC.to_vec();
    let elem = match data.values {
        DataValues::Int(_) => 4u16,
        DataValues::Real(_) => 5,
        DataValues::Str(_) => STR_TYPE,
    };
    b.extend_from_slice(&elem.to_le_bytes());
    b.extend_from_slice(&(data.dims.len() as u16).to_le_bytes());
    for i in 0..4 {
        let d = data.dims.get(i).copied().unwrap_or(0);
        b.extend_from_slice(&(d as u32).to_le_bytes());
    }
    match &data.values {
        DataValues::Int(v) => v.iter().for_each(|n| b.extend_from_slice(&n.to_le_bytes())),
        DataValues::Real(v) => v.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes())),
        DataValues::Str(v) => {
            for s in v {
                let mut text = String::from_utf16_lossy(s);
                while text.len() > MAX_STR_BYTES {
                    text.pop();
                }
                match text.len() {
                    n @ 0..0x80 => b.push(n as u8),
                    n => b.extend_from_slice(&(n as u16 | 0x8000).to_be_bytes()),
                }
                b.extend_from_slice(text.as_bytes());
            }
        }
    }
    b
}

/// A project's title, description and icon, as a META file keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectMeta {
    pub title: Vec<u16>,
    pub description: Vec<u16>,
    /// 40x40 pixels, row by row.
    pub icon: Vec<u32>,
}

const META_MAGIC: &[u8; 8] = b"PCPM0005";
const META_BODY: usize = 0x2B20;
const META_TITLE: usize = 0x08;
const META_DESCRIPTION: usize = 0x38;
const META_ICON: usize = 0x1218;
pub const ICON_SIDE: usize = 40;

/// Text from UTF-16 that ends at its first zero or at `end`.
fn utf16_field(body: &[u8], from: usize, end: usize) -> Vec<u16> {
    body[from..end]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .collect()
}

/// Reads a META file's body.
pub fn parse_meta(body: &[u8]) -> Result<ProjectMeta, FileError> {
    if body.len() < META_BODY || &body[..8] != META_MAGIC {
        return Err(FileError::BadData);
    }
    let side = u32_at(body, META_ICON) as usize;
    let pixels = &body[META_ICON + 4..META_ICON + 4 + 4 * ICON_SIDE * ICON_SIDE];
    if side != ICON_SIDE {
        return Err(FileError::BadData);
    }
    Ok(ProjectMeta {
        title: utf16_field(body, META_TITLE, META_DESCRIPTION),
        description: utf16_field(body, META_DESCRIPTION, META_ICON),
        icon: pixels
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("4")))
            .collect(),
    })
}

/// A META file's body. Text beyond what the fields hold is cut.
pub fn meta_body(meta: &ProjectMeta) -> Vec<u8> {
    let mut b = vec![0u8; META_BODY];
    b[..8].copy_from_slice(META_MAGIC);
    let mut put = |from: usize, end: usize, text: &[u16]| {
        for (i, c) in text.iter().take((end - from) / 2 - 1).enumerate() {
            b[from + 2 * i..from + 2 * i + 2].copy_from_slice(&c.to_le_bytes());
        }
    };
    put(META_TITLE, META_DESCRIPTION, &meta.title);
    put(META_DESCRIPTION, META_ICON, &meta.description);
    b[META_ICON..META_ICON + 4].copy_from_slice(&(ICON_SIDE as u32).to_le_bytes());
    for (i, p) in meta.icon.iter().take(ICON_SIDE * ICON_SIDE).enumerate() {
        let at = META_ICON + 4 + 4 * i;
        b[at..at + 4].copy_from_slice(&p.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The array is read by its own sizes, whatever the file's size says.
    #[test]
    fn a_size_without_the_arrays_header_still_reads() {
        let body = data_body(&Data {
            dims: vec![2],
            values: DataValues::Int(vec![7, 8]),
        });
        let mut bytes = container(Kind::Data, usage::DAT, DATE, &body, false);
        let short = (body.len() - DATA_HEADER) as u32;
        bytes[8..12].copy_from_slice(&short.to_le_bytes());
        let (data, _) = read_data(&bytes).unwrap();
        assert_eq!(data.values, DataValues::Int(vec![7, 8]));
    }

    #[test]
    fn sixteen_bit_pixels() {
        assert_eq!(argb_from_5551(0xFFFF) as u32, 0xFFF8_F8F8);
        assert_eq!(argb_from_5551(0x07FF) as u32, 0xFF00_F8F8);
        assert_eq!(argb_from_5551(0x07FE), 0x0000_F8F8);
        assert_eq!(argb_from_5551(0x8001) as u32, 0xFF80_0000);
        let mut body = b"PCBN0001".to_vec();
        body.extend_from_slice(&3u16.to_le_bytes());
        body.extend_from_slice(&1u16.to_le_bytes());
        body.extend_from_slice(&[1, 0, 0, 0]);
        body.resize(DATA_HEADER, 0);
        body.extend_from_slice(&0xF801u16.to_le_bytes());
        let data = parse_data(&body).unwrap();
        assert_eq!(data.values, DataValues::Int(vec![0xFFF8_0000_u32 as i32]));
    }

    const DATE: Date = Date {
        year: 2026,
        month: 10,
        day: 28,
        hour: 9,
        minute: 15,
        second: 58,
    };

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
        let bytes = container(
            Kind::Text,
            usage::PROGRAM,
            DATE,
            "PRINT \"あ\"\n".as_bytes(),
            false,
        );
        let f = parse(&bytes).unwrap();
        assert_eq!((f.kind, f.usage), (Kind::Text, usage::PROGRAM));
        assert_eq!(f.text().unwrap(), "PRINT \"あ\"\n");
        assert_eq!(f.date, DATE);
    }

    /// What SmileBASIC 4 saved for `DIM A%[2,3]` holding 1 to 6, and for
    /// the string array `["AB", ""]`.
    #[test]
    fn files_saved_by_smilebasic() {
        let payload: Vec<u8> = (1..=6i32).flat_map(|n| n.to_le_bytes()).collect();
        let body = dat(4, &[2, 3], &payload);
        let bytes = container(Kind::Data, usage::DAT, DATE, &body, true);
        let d = parse(&bytes).unwrap().data().unwrap();
        assert_eq!(d.dims, vec![2, 3], "outermost first");
        assert_eq!(d.values, DataValues::Int((1..=6).collect()));

        let s = parse_data(&dat(6, &[2], b"\x02AB\x00")).unwrap();
        assert_eq!(s.values, DataValues::Str(vec![vec![0x41, 0x42], vec![]]));
        // UTF-8, with a two-byte length from 128 bytes on.
        let mut long = b"\x09\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\x81\x2c".to_vec();
        long.extend(std::iter::repeat_n(b'1', 300));
        let s = parse_data(&dat(6, &[2], &long)).unwrap();
        let DataValues::Str(items) = &s.values else {
            panic!("strings")
        };
        assert_eq!(String::from_utf16(&items[0]).unwrap(), "あいう");
        assert_eq!(items[1].len(), 300);
        assert_eq!(&data_body(&s)[28..], &long[..]);
    }

    #[test]
    fn signature_covers_header_and_body() {
        let bytes = container(Kind::Text, usage::TEXT, DATE, b"HI", true);
        let (signed, footer) = bytes.split_at(bytes.len() - FOOTER_SIZE);
        assert_eq!(footer, sign(signed));
        let meta = container(Kind::Meta, 0, DATE, b"PCPM0005", false);
        assert_eq!(meta.len(), HEADER_SIZE + 8, "META has no footer");
    }

    #[test]
    fn meta_reads_back() {
        let meta = ProjectMeta {
            title: "てすと05".encode_utf16().collect(),
            description: "Line 1\nLine 2".encode_utf16().collect(),
            icon: (0..1600).map(|i| 0xFF00_0000 | i).collect(),
        };
        let body = meta_body(&meta);
        assert_eq!(body.len(), 0x2B20);
        assert_eq!(&body[0x1218..0x121C], &[40, 0, 0, 0]);
        let file = parse(&container(Kind::Meta, 0, DATE, &body, false)).unwrap();
        assert_eq!(parse_meta(&file.body).unwrap(), meta);
        assert!(parse_meta(&body[..100]).is_err());
    }

    #[test]
    fn written_data_reads_back() {
        for values in [
            DataValues::Int(vec![1, 2, 3, 4, 5, 6]),
            DataValues::Real(vec![0.5; 6]),
            DataValues::Str((0..6).map(|i| vec![0x41 + i; i as usize]).collect()),
        ] {
            let data = Data {
                dims: vec![2, 3],
                values,
            };
            let f = parse(&container(
                Kind::Data,
                usage::DAT,
                DATE,
                &data_body(&data),
                true,
            ))
            .unwrap();
            assert_eq!((f.kind, f.usage, f.date), (Kind::Data, usage::DAT, DATE));
            assert_eq!(f.data().unwrap(), data);
        }
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(parse(&[0; 10]), Err(FileError::TooShort));
        let mut f = container(Kind::Text, 0, DATE, b"ABC", false);
        f[8] = 200;
        assert_eq!(parse(&f), Err(FileError::TooShort));
        let mut bad = container(Kind::Data, usage::DAT, DATE, b"zz", false);
        bad[HEADER_SIZE] = b'x';
        assert_eq!(parse(&bad).unwrap().data().unwrap_err(), FileError::BadData);
        assert_eq!(
            parse_data(&dat(4, &[3], &[0; 8])).unwrap_err(),
            FileError::BadData
        );
        assert_eq!(
            parse_data(&dat(6, &[1], b"\x05AB")).unwrap_err(),
            FileError::BadData
        );
        assert_eq!(
            parse_data(&dat(2, &[1], &[0; 2])).unwrap_err(),
            FileError::UnsupportedType(2)
        );
        assert_eq!(
            parse_data(&dat(4, &[u32::MAX, u32::MAX, u32::MAX], &[])).unwrap_err(),
            FileError::BadData
        );
    }

    #[test]
    fn arrays_are_limited_as_dim_limits_them() {
        // Their products are 0, and so fit a payload of nothing, but what
        // goes through their rows would not fit.
        for dims in [
            &[u32::MAX, 0][..],
            &[0, u32::MAX],
            &[(1 << 28) + 1, 0],
            &[0, 1 << 15, 1 << 15],
            &[1 << 14, 1 << 14, 2, 0],
        ] {
            for elem in [4, 5, 6] {
                let refused = parse_data(&dat(elem, dims, &[]));
                assert_eq!(refused, Err(FileError::BadData), "{dims:?}");
            }
        }
        assert_eq!(checked_elements(&[(1 << 28) + 1]), Err(FileError::BadData));
        let too_many = checked_elements(&[1 << 14, (1 << 14) + 1]);
        assert_eq!(too_many, Err(FileError::BadData));
        assert_eq!(checked_elements(&[1 << 28]), Ok(1 << 28));
        assert_eq!(checked_elements(&[1 << 14, 1 << 14]), Ok(1 << 28));

        // Empty arrays keep their sizes.
        for dims in [
            &[0][..],
            &[0, 3],
            &[3, 0],
            &[16, 0],
            &[0, 0],
            &[2, 0, 2],
            &[1 << 28, 0],
        ] {
            for elem in [4, 5, 6] {
                let kept: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
                assert_eq!(parse_data(&dat(elem, dims, &[])).unwrap().dims, kept);
            }
        }
    }

    #[test]
    fn garbage_never_panics() {
        let base = container(
            Kind::Data,
            usage::DAT,
            DATE,
            &dat(4, &[1], &[1, 2, 3, 4]),
            true,
        );
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
