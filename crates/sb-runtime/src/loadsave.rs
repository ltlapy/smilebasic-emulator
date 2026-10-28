//! Files of values and images: LOADV, SAVEV, LOADG and SAVEG.
//!
//! As with LOAD and SAVE, a file that cannot be read or written sets
//! RESULT() to 0 rather than stopping the program; a wrong argument is an
//! error.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::{Datelike, Timelike};
use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayData, ArrayRef, Value, new_str};

use crate::confirm::FileAction;
use crate::files::{program_text, str_arg};
use crate::graphics::{GRP_SIZE, PAGES};
use crate::image::{self, Image};
use crate::runtime::{Command, Runtime, illegal, opt_int};
use crate::sbfile::{self, Data, DataValues, Kind};
use crate::storage::{self, FileKind, StorageError, parse_file_name};

/// JPEG quality when SAVEG is not given one.
const JPEG_QUALITY: i32 = 60;

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn page_arg(v: &Value) -> Result<usize> {
    let n = v.to_int()?;
    usize::try_from(n)
        .ok()
        .filter(|&p| p < PAGES)
        .ok_or_else(|| out_of_range("graphic page", n))
}

/// A file name whose type cannot be left out.
fn typed_name(spec: &str) -> storage::Result<(FileKind, String)> {
    if !spec.contains(':') {
        return Err(StorageError::BadKind(String::new()));
    }
    parse_file_name(spec, FileKind::Txt)
}

fn bad_file(e: impl ToString) -> StorageError {
    StorageError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        e.to_string(),
    ))
}

fn now() -> sbfile::Date {
    let t = chrono::Local::now();
    sbfile::Date {
        year: t.year() as u16,
        month: t.month() as u8,
        day: t.day() as u8,
        hour: t.hour() as u8,
        minute: t.minute() as u8,
        second: t.second() as u8,
    }
}

fn array_value(array: Array) -> Value {
    Value::Array(Rc::new(RefCell::new(array)))
}

/// What LOADV gives when a file cannot be read: an empty string for TXT,
/// otherwise a number. The manual says an empty array, but on hardware
/// LEN(LOADV("DAT:NONE")) is a Type mismatch.
// UNVERIFIED: that the number is 0, and the empty string for TXT.
fn failed_value(spec: &str) -> Value {
    match typed_name(spec) {
        Ok((FileKind::Txt, _)) => Value::str(""),
        _ => Value::Int(0),
    }
}

fn data_array(data: Data) -> Array {
    let data_values = match data.values {
        DataValues::Int(v) => ArrayData::Int(v),
        DataValues::Real(v) => ArrayData::Real(v),
        DataValues::Str(v) => ArrayData::Str(v.into_iter().map(new_str).collect()),
    };
    Array {
        dims: data.dims,
        data: data_values,
    }
}

fn image_array(image: Image) -> Array {
    Array {
        dims: vec![image.height, image.width],
        data: ArrayData::Int(image.pixels.into_iter().map(|p| p as i32).collect()),
    }
}

/// Fills `target` with `source`, taking its size: numbers convert between
/// integer and real, but strings and numbers do not mix, and the number
/// of dimensions must agree.
fn fill(target: &ArrayRef, source: Array) -> Result<()> {
    let mut target = target.borrow_mut();
    if target.dims.len() != source.dims.len() {
        let detail = format!("{}D file", source.dims.len());
        return Err(Error::with_detail(ErrorKind::TypeMismatch, detail));
    }
    let data = match (&target.data, source.data) {
        (ArrayData::Str(_), data @ ArrayData::Str(_)) => data,
        (ArrayData::Str(_), _) | (_, ArrayData::Str(_)) => {
            return Err(ErrorKind::TypeMismatch.into());
        }
        (ArrayData::Int(_), ArrayData::Real(v)) => ArrayData::Int(
            v.into_iter()
                .map(sb_core::value::real_to_int)
                .collect::<Result<_>>()?,
        ),
        (ArrayData::Real(_), ArrayData::Int(v)) => {
            ArrayData::Real(v.into_iter().map(f64::from).collect())
        }
        (_, data) => data,
    };
    *target = Array {
        dims: source.dims,
        data,
    };
    Ok(())
}

/// The data SAVEV keeps for a DAT file.
fn array_data(array: &Array) -> Data {
    let values = match &array.data {
        ArrayData::Int(v) => DataValues::Int(v.clone()),
        ArrayData::Real(v) => DataValues::Real(v.clone()),
        ArrayData::Str(v) => DataValues::Str(v.iter().map(|s| s.borrow().clone()).collect()),
    };
    Data {
        dims: array.dims.clone(),
        values,
    }
}

/// A 2D numeric array as an image, for SAVEV's GRP files.
fn array_image(array: &Array) -> Result<Image> {
    let [height, width] = array.dims[..] else {
        return Err(Error::with_detail(
            ErrorKind::TypeMismatch,
            "not a 2D array",
        ));
    };
    if width == 0 || height == 0 || width > GRP_SIZE as usize || height > GRP_SIZE as usize {
        let detail = format!("{width}x{height} image");
        return Err(Error::with_detail(ErrorKind::OutOfRange, detail));
    }
    let pixels = match &array.data {
        ArrayData::Int(v) => v.iter().map(|&p| p as u32).collect(),
        ArrayData::Real(v) => (v.iter())
            .map(|&x| sb_core::value::real_to_int(x).map(|p| p as u32))
            .collect::<Result<_>>()?,
        ArrayData::Str(_) => return Err(ErrorKind::TypeMismatch.into()),
    };
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// SAVEG's rectangle: `x, y, width, height` on a 2048-pixel page.
fn rect_arg(args: &[Value]) -> Result<(i32, i32, i32, i32)> {
    let [x, y, w, h] = [0, 1, 2, 3].map(|i| args[i].to_int());
    let (x, y, w, h) = (x?, y?, w?, h?);
    for (what, n, range) in [
        ("X", x, 0..GRP_SIZE),
        ("Y", y, 0..GRP_SIZE),
        ("width", w, 1..GRP_SIZE + 1),
        ("height", h, 1..GRP_SIZE + 1),
    ] {
        if !range.contains(&n) {
            return Err(out_of_range(what, n));
        }
    }
    if x + w > GRP_SIZE || y + h > GRP_SIZE {
        return Err(Error::with_detail(ErrorKind::OutOfRange, "past the page"));
    }
    Ok((x, y, w, h))
}

impl Runtime {
    pub(crate) fn loadsave_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args) {
            (Command::Loadv, Want::Value, [spec]) => {
                let spec = str_arg(spec)?;
                if self.ask_load(&spec, FileKind::Txt) {
                    return Ok(vec![Value::Int(0)]);
                }
                let outcome = self.load_value(&spec);
                let value = self.finish("LOADV", outcome);
                Ok(vec![value.unwrap_or_else(|| failed_value(&spec))])
            }
            (Command::Loadv, Want::Outs(0), [spec, target]) => {
                let spec = str_arg(spec)?;
                let Value::Array(target) = target else {
                    return Err(ErrorKind::TypeMismatch.into());
                };
                if self.ask_load(&spec, FileKind::Txt) {
                    return Ok(vec![]);
                }
                let outcome =
                    match self.load_value(&spec) {
                        Ok(Value::Array(a)) => Ok(Rc::try_unwrap(a)
                            .map_or_else(|a| a.borrow().clone(), RefCell::into_inner)),
                        Ok(_) => Err(StorageError::BadKind("TXT".to_string())),
                        Err(e) => Err(e),
                    };
                if let Some(array) = self.finish("LOADV", outcome) {
                    fill(target, array)?;
                }
                Ok(vec![])
            }
            (Command::Savev, Want::Outs(0), [spec, value]) => {
                let spec = str_arg(spec)?;
                let file = typed_name(&spec);
                let bytes = match (&file, value) {
                    (Ok((FileKind::Txt, _)), Value::Str(s)) => {
                        String::from_utf16_lossy(&s.borrow()).into_bytes()
                    }
                    (Ok((FileKind::Dat, _)), Value::Array(a)) => {
                        let body = sbfile::data_body(&array_data(&a.borrow()));
                        sbfile::container(Kind::Data, sbfile::usage::DAT, now(), &body)
                    }
                    (Ok((FileKind::Grp, _)), Value::Array(a)) => {
                        image::encode_png(&array_image(&a.borrow())?)
                    }
                    (Ok((FileKind::Txt | FileKind::Dat | FileKind::Grp, _)), _) => {
                        return Err(ErrorKind::TypeMismatch.into());
                    }
                    _ => Vec::new(),
                };
                if self.ask_save(&spec, FileKind::Txt) {
                    return Ok(vec![]);
                }
                let outcome = file.and_then(|(kind, name)| match kind {
                    FileKind::Jpg => Err(StorageError::BadKind(kind.name().to_string())),
                    _ => (self.storage).write(kind, &name, &bytes, Some(kind.backup_name())),
                });
                self.finish("SAVEV", outcome);
                Ok(vec![])
            }
            (Command::Loadg, Want::Outs(0 | 2), [spec, page, rest @ ..])
                if matches!(rest.len(), 0 | 2) =>
            {
                let spec = str_arg(spec)?;
                let page = page_arg(page)?;
                let at = match rest {
                    [x, y] => (opt_int(x)?.unwrap_or(0), opt_int(y)?.unwrap_or(0)),
                    _ => (0, 0),
                };
                let sizes = |w: usize, h: usize| match want {
                    Want::Outs(2) => vec![Value::Int(w as i32), Value::Int(h as i32)],
                    _ => vec![],
                };
                if self.ask_load(&spec, FileKind::Grp) {
                    return Ok(sizes(0, 0));
                }
                let outcome = self.load_image(&spec)?;
                match self.finish("LOADG", outcome) {
                    Some(image) => {
                        self.grp.put_image(page, at, &image);
                        Ok(sizes(image.width, image.height))
                    }
                    None => Ok(sizes(0, 0)),
                }
            }
            (Command::Saveg, Want::Outs(0), [spec, page, rest @ ..]) => {
                let spec = str_arg(spec)?;
                let page = page_arg(page)?;
                let file = parse_file_name(&spec, FileKind::Grp);
                let jpeg = matches!(file, Ok((FileKind::Jpg, _)));
                let (rect, quality) = match (rest, jpeg) {
                    ([], _) => ((0, 0, GRP_SIZE, GRP_SIZE), None),
                    ([q], true) => ((0, 0, GRP_SIZE, GRP_SIZE), Some(q)),
                    ([_, _, _, _], _) => (rect_arg(rest)?, None),
                    ([_, _, _, _, q], true) => (rect_arg(rest)?, Some(q)),
                    _ => return Err(illegal()),
                };
                let quality = match quality.map(opt_int).transpose()?.flatten() {
                    None => JPEG_QUALITY,
                    Some(q @ 1..=100) => q,
                    Some(q) => return Err(out_of_range("compression", q)),
                };
                if self.ask_save(&spec, FileKind::Grp) {
                    return Ok(vec![]);
                }
                let image = Image {
                    width: rect.2 as usize,
                    height: rect.3 as usize,
                    pixels: self.grp.save_rect(page, rect),
                };
                let outcome = file.and_then(|(kind, name)| {
                    let bytes = match kind {
                        FileKind::Grp => image::encode_png(&image),
                        FileKind::Jpg => image::encode_jpeg(&image, quality as u8),
                        _ => return Err(StorageError::BadKind(kind.name().to_string())),
                    };
                    self.storage
                        .write(kind, &name, &bytes, Some(kind.backup_name()))
                });
                self.finish("SAVEG", outcome);
                Ok(vec![])
            }
            _ => Err(illegal()),
        }
    }

    /// LOADV's value: a string for TXT files, an array for DAT and GRP.
    fn load_value(&self, spec: &str) -> storage::Result<Value> {
        let (kind, name) = typed_name(spec)?;
        let bytes = self.storage.read(kind, &name)?;
        match kind {
            FileKind::Txt => Ok(Value::str(&program_text(&bytes)?)),
            FileKind::Dat => {
                let file = sbfile::parse(&bytes).map_err(bad_file)?;
                Ok(array_value(data_array(file.data().map_err(bad_file)?)))
            }
            FileKind::Grp => Ok(array_value(image_array(
                image::decode(&bytes).map_err(bad_file)?,
            ))),
            FileKind::Jpg => Err(StorageError::BadKind(kind.name().to_string())),
        }
    }

    /// LOADG's image. A DAT file that is not a 2D integer array is an
    /// error, not a failure.
    fn load_image(&self, spec: &str) -> Result<storage::Result<Image>> {
        let (kind, name) = match parse_file_name(spec, FileKind::Grp) {
            Ok(file) => file,
            Err(e) => return Ok(Err(e)),
        };
        let bytes = match self.storage.read(kind, &name) {
            Ok(bytes) => bytes,
            Err(e) => return Ok(Err(e)),
        };
        if kind == FileKind::Txt {
            return Ok(Err(StorageError::BadKind(kind.name().to_string())));
        }
        if kind == FileKind::Dat {
            let data = sbfile::parse(&bytes).and_then(|f| f.data());
            if let Ok(data) = &data
                && !(data.dims.len() == 2 && matches!(data.values, DataValues::Int(_)))
            {
                return Err(Error::with_detail(
                    ErrorKind::TypeMismatch,
                    "not a 2D array",
                ));
            }
        }
        Ok(image::decode(&bytes).map_err(bad_file))
    }

    /// Asks before reading a file that exists, as LOAD does; whether the
    /// command must stop here for the answer.
    // UNVERIFIED: that LOADV and LOADG in a program do not ask, like LOAD.
    fn ask_load(&mut self, spec: &str, default: FileKind) -> bool {
        let Ok((kind, name)) = parse_file_name(spec, default) else {
            return false;
        };
        if !self.storage.exists(kind, &name) {
            return false;
        }
        let file = format!("{}:{name}", kind.name());
        let action = FileAction {
            title: "Load file".to_string(),
            summary: "Load this file.".to_string(),
            details: self.details(&file, None),
            accept: "Load",
            notice: format!("Loading {file}"),
            in_programs: false,
        };
        let ask = self.confirm_file(action, &[]);
        self.held(ask)
    }

    /// Asks before writing a file, as SAVE does.
    fn ask_save(&mut self, spec: &str, default: FileKind) -> bool {
        let Ok((kind, name)) = parse_file_name(spec, default) else {
            return false;
        };
        let file = format!("{}:{name}", kind.name());
        let mut details = self.details(&file, None);
        if self.storage.exists(kind, &name) {
            details.push(String::new());
            details.push("The file there now will be replaced.".to_string());
        }
        let action = FileAction {
            title: "Save file".to_string(),
            summary: "Save this file.".to_string(),
            details,
            accept: "Save",
            notice: format!("Saving {file}"),
            in_programs: true,
        };
        let ask = self.confirm_file(action, &[]);
        self.held(ask)
    }
}
