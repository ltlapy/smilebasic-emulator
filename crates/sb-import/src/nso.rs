//! Reading the segments of a Switch executable (NSO), so that the importer
//! can find SmileBASIC 4's built-in tables in the user's own copy.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum NsoError {
    NotNso,
    Truncated,
    /// A compressed segment does not decompress to its stated size.
    BadCompression,
}

impl fmt::Display for NsoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NsoError::NotNso => write!(f, "not an NSO executable"),
            NsoError::Truncated => write!(f, "executable is truncated"),
            NsoError::BadCompression => write!(f, "executable segment is damaged"),
        }
    }
}

impl std::error::Error for NsoError {}

fn u32_at(b: &[u8], at: usize) -> Result<usize, NsoError> {
    let bytes = b.get(at..at + 4).ok_or(NsoError::Truncated)?;
    Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")) as usize)
}

/// The text, read-only and data segments, decompressed.
pub fn segments(nso: &[u8]) -> Result<[Vec<u8>; 3], NsoError> {
    if nso.get(..4) != Some(b"NSO0") {
        return Err(NsoError::NotNso);
    }
    let flags = u32_at(nso, 0x0C)?;
    let mut out: [Vec<u8>; 3] = Default::default();
    for (i, segment) in out.iter_mut().enumerate() {
        let offset = u32_at(nso, 0x10 + i * 0x10)?;
        let size = u32_at(nso, 0x18 + i * 0x10)?;
        let stored = u32_at(nso, 0x60 + i * 4)?;
        let bytes = nso
            .get(offset..offset + stored)
            .ok_or(NsoError::Truncated)?;
        *segment = if flags & (1 << i) != 0 {
            lz4_block(bytes, size)?
        } else {
            bytes.to_vec()
        };
    }
    Ok(out)
}

/// Decompresses one LZ4 block of known decompressed size.
fn lz4_block(src: &[u8], size: usize) -> Result<Vec<u8>, NsoError> {
    let mut out = Vec::with_capacity(size);
    let mut i = 0;
    let byte = |i: usize| src.get(i).copied().ok_or(NsoError::BadCompression);
    // A length nibble of 15 continues in following bytes until one < 255.
    let length = |nibble: usize, i: &mut usize| -> Result<usize, NsoError> {
        let mut len = nibble;
        if nibble == 15 {
            loop {
                let b = byte(*i)?;
                *i += 1;
                len += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        Ok(len)
    };
    while i < src.len() {
        let token = usize::from(byte(i)?);
        i += 1;
        let literals = length(token >> 4, &mut i)?;
        let run = src.get(i..i + literals).ok_or(NsoError::BadCompression)?;
        out.extend_from_slice(run);
        i += literals;
        if i == src.len() {
            break;
        }
        let back = usize::from(byte(i)?) | usize::from(byte(i + 1)?) << 8;
        i += 2;
        let len = length(token & 15, &mut i)? + 4;
        if back == 0 || back > out.len() {
            return Err(NsoError::BadCompression);
        }
        // Copies may overlap the bytes they produce.
        let from = out.len() - back;
        for k in 0..len {
            out.push(out[from + k]);
        }
        if out.len() > size {
            return Err(NsoError::BadCompression);
        }
    }
    if out.len() == size {
        Ok(out)
    } else {
        Err(NsoError::BadCompression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_literals_and_overlapping_copies() {
        // "ab" then copy 6 bytes from 2 back: "abababab".
        let block = [0x22, b'a', b'b', 2, 0, 0x00];
        assert_eq!(lz4_block(&block[..5], 8), Ok(b"abababab".to_vec()));
        // A final block of only literals.
        assert_eq!(lz4_block(&[0x30, b'x', b'y', b'z'], 3), Ok(b"xyz".to_vec()));
        assert_eq!(
            lz4_block(&[0x22, b'a', b'b', 9, 0], 8),
            Err(NsoError::BadCompression)
        );
    }

    #[test]
    fn reads_an_uncompressed_nso() {
        let mut nso = vec![0u8; 0x100];
        nso[..4].copy_from_slice(b"NSO0");
        for (i, text) in [&b"TEXT"[..], b"RO", b"D"].iter().enumerate() {
            let offset = nso.len() as u32;
            nso[0x10 + i * 0x10..][..4].copy_from_slice(&offset.to_le_bytes());
            nso[0x18 + i * 0x10..][..4].copy_from_slice(&(text.len() as u32).to_le_bytes());
            nso[0x60 + i * 4..][..4].copy_from_slice(&(text.len() as u32).to_le_bytes());
            nso.extend_from_slice(text);
        }
        let [text, ro, data] = segments(&nso).unwrap();
        assert_eq!(
            (&text[..], &ro[..], &data[..]),
            (&b"TEXT"[..], &b"RO"[..], &b"D"[..])
        );
        assert_eq!(segments(b"ELF"), Err(NsoError::NotNso));
    }
}
