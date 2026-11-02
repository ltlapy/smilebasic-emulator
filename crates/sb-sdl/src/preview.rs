//! What the projects window shows of a file before it is opened: a
//! scaled picture of a GRP or JPG file, the first lines of a TXT file,
//! and the shape and first values of a DAT file. Each is read once, when
//! the file is chosen, not as it is drawn.

use std::hash::{DefaultHasher, Hash, Hasher};

use sb_core::value::{Value, format_number};
use sb_runtime::image;
use sb_runtime::sbfile::{self, Data, DataValues};
use sb_runtime::storage::{FileKind, MAX_FILE, Storage};

/// Pixels to draw with [`crate::ui::image`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub pixels: Box<[u32]>,
    /// Tells pictures apart for dry runs of drawing.
    pub key: u64,
}

impl Picture {
    fn new(w: u32, h: u32, pixels: Box<[u32]>) -> Picture {
        let mut hash = DefaultHasher::new();
        (w, h, &pixels).hash(&mut hash);
        Picture {
            w,
            h,
            pixels,
            key: hash.finish(),
        }
    }

    /// `pixels`, `w` x `h`, scaled by the nearest pixel to exactly
    /// `to_w` x `to_h`.
    pub fn scaled(pixels: &[u32], (w, h): (u32, u32), (to_w, to_h): (u32, u32)) -> Picture {
        let mut out = Vec::with_capacity((to_w * to_h) as usize);
        for y in 0..to_h {
            let row = (u64::from(y) * u64::from(h) / u64::from(to_h.max(1))) as u32;
            for x in 0..to_w {
                let col = (u64::from(x) * u64::from(w) / u64::from(to_w.max(1))) as u32;
                out.push(pixels.get((row * w + col) as usize).copied().unwrap_or(0));
            }
        }
        Picture::new(to_w, to_h, out.into())
    }

    /// `pixels` as large as fits in `room`, keeping its shape: grown a
    /// whole number of times, so that its pixels stay square, or else
    /// shrunk.
    pub fn fitted(pixels: &[u32], (w, h): (u32, u32), room: (u32, u32)) -> Picture {
        Picture::scaled(pixels, (w, h), fit((w, h), room))
    }
}

/// The size `(w, h)` is shown at in `room`, as [`Picture::fitted`] does.
pub fn fit((w, h): (u32, u32), (room_w, room_h): (u32, u32)) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    let times = (room_w / w).min(room_h / h);
    if times >= 1 {
        return (w * times, h * times);
    }
    // Shrunk by the side that is shorter for its size.
    if u64::from(w) * u64::from(room_h) >= u64::from(h) * u64::from(room_w) {
        let to_h = (u64::from(h) * u64::from(room_w) / u64::from(w)) as u32;
        (room_w.max(1), to_h.max(1))
    } else {
        let to_w = (u64::from(w) * u64::from(room_h) / u64::from(h)) as u32;
        (to_w.max(1), room_h.max(1))
    }
}

/// A byte count to read at a glance.
pub fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..0x10_0000 => format!("{}.{} KB", bytes / 1024, bytes * 10 / 1024 % 10),
        _ => {
            let tenths = bytes * 10 / 0x10_0000;
            format!("{}.{} MB", tenths / 10, tenths % 10)
        }
    }
}

/// What a file holds, as read for showing it.
pub enum Content {
    /// The text's lines, tabs made spaces, and how many there are.
    Text {
        lines: Vec<String>,
        count: usize,
    },
    /// The image's size and pixels.
    Image {
        w: u32,
        h: u32,
        pixels: Vec<u32>,
    },
    Data(Data),
    Failed(String),
}

/// Reads a file of `project`. Choosing a file should not take long or say
/// much: one too large to read just shows nothing.
pub fn read(storage: &Storage, project: &str, kind: FileKind, name: &str) -> Content {
    let size = storage.file_info(project, kind, name).map(|info| info.0);
    if size.is_some_and(|size| size > MAX_FILE) {
        return Content::Failed(String::new());
    }
    let bytes = match storage.read_file_in(project, &format!("{}{name}", kind.prefix())) {
        Ok(bytes) => bytes,
        Err(e) => return Content::Failed(format!("Could not read it: {e}")),
    };
    match kind {
        FileKind::Txt => match sb_runtime::program_text(&bytes) {
            Ok(text) => {
                let lines: Vec<String> = text.lines().map(|l| l.replace('\t', "    ")).collect();
                Content::Text {
                    count: lines.len(),
                    lines,
                }
            }
            Err(e) => Content::Failed(format!("Not text: {e}")),
        },
        FileKind::Grp | FileKind::Jpg => match image::decode(&bytes) {
            Ok(img) => Content::Image {
                w: img.width as u32,
                h: img.height as u32,
                pixels: img.pixels,
            },
            Err(e) => Content::Failed(format!("Not an image: {e}")),
        },
        FileKind::Dat => match sbfile::read_data(&bytes) {
            Ok((data, _)) => Content::Data(data),
            Err(e) => Content::Failed(format!("Not an array: {e}")),
        },
    }
}

/// `int`, `real` or `string`.
pub fn element_type(data: &Data) -> &'static str {
    match data.values {
        DataValues::Int(_) => "int",
        DataValues::Real(_) => "real",
        DataValues::Str(_) => "string",
    }
}

/// `10x20`, outermost first, as DIM takes them.
pub fn dims_text(data: &Data) -> String {
    let dims: Vec<String> = data.dims.iter().map(usize::to_string).collect();
    dims.join("x")
}

pub fn value_count(data: &Data) -> usize {
    match &data.values {
        DataValues::Int(v) => v.len(),
        DataValues::Real(v) => v.len(),
        DataValues::Str(v) => v.len(),
    }
}

/// Value `i`, as PRINT shows numbers; strings are quoted.
pub fn value_text(data: &Data, i: usize) -> String {
    match &data.values {
        DataValues::Int(v) => v.get(i).map_or(String::new(), |n| n.to_string()),
        DataValues::Real(v) => v.get(i).map_or(String::new(), |&x| {
            String::from_utf16_lossy(&format_number(&Value::Real(x)))
        }),
        DataValues::Str(v) => v.get(i).map_or(String::new(), |s| {
            format!("\"{}\"", String::from_utf16_lossy(s))
        }),
    }
}

/// The first values of `data` as lines of up to `cols` characters, at
/// most `rows` of them: a row of the array to a line when it has two
/// dimensions or more, or else as many as fit.
pub fn data_lines(data: &Data, cols: usize, rows: usize) -> Vec<String> {
    let count = value_count(data);
    let row_len = match data.dims.len() {
        1 => count.max(1),
        _ => data.dims.last().copied().unwrap_or(1).max(1),
    };
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut i = 0;
    while i < count && lines.len() < rows {
        let text = value_text(data, i);
        let ends_row = (i + 1) % row_len == 0;
        let fits = line.is_empty() || line.chars().count() + 1 + text.chars().count() <= cols;
        if !fits {
            if data.dims.len() > 1 {
                // The rest of the row does not fit: on to the next.
                lines.push(std::mem::take(&mut line));
                i = (i / row_len + 1) * row_len;
                continue;
            }
            lines.push(std::mem::take(&mut line));
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&text);
        i += 1;
        if ends_row && data.dims.len() > 1 {
            lines.push(std::mem::take(&mut line));
        }
    }
    if !line.is_empty() && lines.len() < rows {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_fit_and_scale() {
        assert_eq!(fit((40, 40), (64, 64)), (40, 40), "grows only whole times");
        assert_eq!(fit((40, 40), (80, 100)), (80, 80));
        assert_eq!(fit((512, 256), (128, 128)), (128, 64));
        assert_eq!(fit((256, 512), (128, 128)), (64, 128));
        let p = Picture::scaled(&[1, 2, 3, 4], (2, 2), (4, 4));
        assert_eq!(&p.pixels[..4], [1, 1, 2, 2]);
        assert_eq!(&p.pixels[12..], [3, 3, 4, 4]);
        let other = Picture::scaled(&[1, 2, 3, 5], (2, 2), (4, 4));
        assert_ne!(p.key, other.key);
    }

    #[test]
    fn a_file_too_large_shows_nothing() {
        let dir = std::env::temp_dir().join(format!("sb-preview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Storage::new(dir.clone());
        storage.create_project("GAME").unwrap();
        let file = std::fs::File::create(dir.join("GAME/GHUGE")).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        let content = read(&storage, "GAME", FileKind::Grp, "HUGE");
        assert!(matches!(content, Content::Failed(e) if e.is_empty()));
        // A smaller file that is no image still says so.
        file.set_len(16).unwrap();
        let content = read(&storage, "GAME", FileKind::Grp, "HUGE");
        assert!(matches!(content, Content::Failed(e) if !e.is_empty()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes_read_at_a_glance() {
        assert_eq!(size_text(500), "500 bytes");
        assert_eq!(size_text(6 * 1024 + 512), "6.5 KB");
        assert_eq!(size_text(3 * 0x10_0000), "3.0 MB");
    }

    #[test]
    fn data_shows_its_first_values() {
        let data = Data {
            dims: vec![2, 3],
            values: DataValues::Int(vec![1, 2, 3, 4, 5, 6]),
        };
        assert_eq!(data_lines(&data, 20, 5), ["1 2 3", "4 5 6"]);
        assert_eq!(data_lines(&data, 3, 5), ["1 2", "4 5"]);
        let data = Data {
            dims: vec![5],
            values: DataValues::Real(vec![0.5, 1.0, 2.25, 3.0, 4.0]),
        };
        assert_eq!(data_lines(&data, 8, 2), ["0.5 1", "2.25 3 4"]);
        assert_eq!(
            (element_type(&data), dims_text(&data)),
            ("real", "5".into())
        );
        let data = Data {
            dims: vec![1],
            values: DataValues::Str(vec!["HI".encode_utf16().collect()]),
        };
        assert_eq!(value_text(&data, 0), "\"HI\"");
    }
}
