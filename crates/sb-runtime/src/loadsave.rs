//! Files of values and images: LOADV, SAVEV, LOADG and SAVEG.
//!
//! As with LOAD and SAVE, a file that cannot be read or written sets
//! RESULT() to 0 rather than stopping the program; a wrong argument is an
//! error.

use std::cell::RefCell;
use std::rc::Rc;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::memory;
use sb_core::value::{Array, ArrayData, ArrayRef, Value, new_str};

use crate::confirm::FileAction;
use crate::files::{program_text, str_arg};
use crate::graphics::{GRP_SIZE, PAGES};
use crate::image::{self, Image};
use crate::runtime::{Command, Runtime, checking, illegal, no_form, opt_int};
use crate::sbfile::{self, Data, DataValues};
use crate::storage::{self, FileKind, FileRef, StorageError};

/// JPEG quality when SAVEG is not given one.
const JPEG_QUALITY: i32 = 60;

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn page_arg(v: &Value) -> Result<usize> {
    checking(v, || {
        let n = v.to_int()?;
        usize::try_from(n)
            .ok()
            .filter(|&p| p < PAGES)
            .ok_or_else(|| out_of_range("graphic page", n))
    })
}

/// The kind of a file named for reading, whose type cannot be left out.
fn read_kind(spec: &str) -> Option<FileKind> {
    spec.contains(':')
        .then(|| storage::parse_read_name(spec, FileKind::Txt).ok())
        .flatten()
        .map(|f| f.kind)
}

fn bad_file(e: impl ToString) -> StorageError {
    StorageError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        e.to_string(),
    ))
}

fn array_value(array: Array) -> Value {
    Value::Array(Rc::new(RefCell::new(array)))
}

/// LOADV's type of file, before the file is looked for, in both forms:
/// `JPG:`, `PNG:` and a name without its type are File type mismatch.
fn loadv_type(spec: &str) -> Result<()> {
    let detail = match spec.split_once(':') {
        None => "no type".to_string(),
        Some((kind, _)) if ["JPG", "PNG"].iter().any(|k| kind.eq_ignore_ascii_case(k)) => {
            format!("{} file", kind.to_ascii_uppercase())
        }
        Some(_) => return Ok(()),
    };
    Err(Error::with_detail(ErrorKind::FileTypeMismatch, detail))
}

/// What LOADV gives when a file cannot be read: an empty string for TXT,
/// otherwise the omitted value (TYPEOF 0), although the manual says an
/// empty array (checked on hardware).
fn failed_value(spec: &str) -> Value {
    match read_kind(spec) {
        Some(FileKind::Txt) => Value::str(""),
        _ => Value::Default,
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

/// What an array's strings take.
fn strings_size(array: &Array) -> usize {
    match &array.data {
        ArrayData::Str(v) => (v.iter())
            .map(|s| memory::string_bytes(s.borrow().len()))
            .sum(),
        _ => 0,
    }
}

/// What an array holds, its strings too.
fn array_size(array: &Array) -> usize {
    memory::array_bytes(array.elem_type(), array.len()).saturating_add(strings_size(array))
}

fn loadv_memory(e: Error) -> Error {
    Error::with_detail(e.kind, "LOADV")
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
        // Truncated as INT() does, so 2147483647.5 fits, with a NaN as 0.
        (ArrayData::Int(_), ArrayData::Real(v)) => ArrayData::Int(
            v.into_iter()
                .map(|x| match x.is_nan() {
                    true => Ok(0),
                    false => sb_core::value::real_to_int(x),
                })
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
                loadv_type(&spec)?;
                if self.ask_load(&spec, FileKind::Txt) {
                    return Ok(vec![Value::Int(0)]);
                }
                let outcome = self.load_value(&spec);
                // The new value has to fit beside what it will replace.
                if let Ok(value) = &outcome {
                    let bytes = match value {
                        Value::Str(s) => memory::string_bytes(s.borrow().len()),
                        Value::Array(a) => array_size(&a.borrow()),
                        _ => 0,
                    };
                    memory::request(bytes).map_err(loadv_memory)?;
                }
                let value = self.finish("LOADV", outcome);
                Ok(vec![value.unwrap_or_else(|| failed_value(&spec))])
            }
            (Command::Loadv, Want::Outs(0), [spec, target]) => {
                let spec = str_arg(spec)?;
                loadv_type(&spec)?;
                // The command form is for DAT and GRP files only, whatever
                // it is given to load into: a TXT file into a string is the
                // same error as into an array.
                if let Some(kind @ FileKind::Txt) = read_kind(&spec) {
                    let detail = format!("{} in the command form", kind.name());
                    return Err(Error::with_detail(
                        ErrorKind::WrongNumberOfArguments,
                        detail,
                    ));
                }
                // Before the file is looked for, for GRP files as for DAT.
                let Value::Array(target) = target else {
                    return Err(Error::new(ErrorKind::TypeMismatch).of_arg(1));
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
                // The array stays where it is unless it has to grow, which
                // needs a new block beside the old, as RESIZE does.
                if let Ok(array) = &outcome {
                    let target = target.borrow();
                    let elem = target.elem_type();
                    memory::grow(
                        memory::array_bytes(elem, target.len()),
                        memory::array_bytes(elem, array.len()),
                    )
                    .map_err(loadv_memory)?;
                    memory::charge(strings_size(array));
                }
                if let Some(array) = self.finish("LOADV", outcome) {
                    fill(target, array)?;
                }
                Ok(vec![])
            }
            (Command::Savev, Want::Outs(0), [spec, value]) => {
                let spec = str_arg(spec)?;
                self.refuse_save_project(&spec)?;
                // The type cannot be left out.
                let file = match spec.contains(':') {
                    true => self.write_name(&spec, FileKind::Txt),
                    false => Err(StorageError::BadKind(String::new())),
                };
                let bytes = match (file.as_ref().map(|f| f.kind), value) {
                    (Ok(FileKind::Txt), Value::Str(s)) => {
                        String::from_utf16_lossy(&s.borrow()).into_bytes()
                    }
                    // The bare PCBN array, without SmileBASIC 4's container.
                    (Ok(FileKind::Dat), Value::Array(a)) => {
                        sbfile::data_body(&array_data(&a.borrow()))
                    }
                    (Ok(FileKind::Grp), Value::Array(a)) => {
                        image::encode_png(&array_image(&a.borrow())?)
                            .map_err(|e| Error::with_detail(ErrorKind::IllegalFileFormat, e))?
                    }
                    (Ok(FileKind::Txt | FileKind::Dat | FileKind::Grp), _) => {
                        return Err(ErrorKind::TypeMismatch.into());
                    }
                    _ => Vec::new(),
                };
                // A name without its type asks too, and then fails.
                if self.ask_save(&self.write_name(&spec, FileKind::Txt)) {
                    return Ok(vec![]);
                }
                let outcome = file.and_then(|file| match file.kind {
                    FileKind::Jpg => Err(StorageError::BadKind(file.kind.name().to_string())),
                    kind => (self.storage).write_ref(&file, &bytes, Some(kind.backup_name())),
                });
                self.finish("SAVEV", outcome);
                Ok(vec![])
            }
            (Command::Loadg, Want::Outs(0 | 2), [spec, page, rest @ ..])
                if matches!(rest.len(), 0 | 2) =>
            {
                let spec = str_arg(spec)?;
                self.loadg_type(&spec)?;
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
                    Some((image, stored)) => {
                        if let Some(meter) = &mut self.meter {
                            meter.charge_loadg((image.width * image.height) as u64, stored);
                        }
                        self.grp.put_image(page, at, &image);
                        Ok(sizes(image.width, image.height))
                    }
                    None => Ok(sizes(0, 0)),
                }
            }
            (Command::Saveg, Want::Outs(0), [spec, page, rest @ ..]) => {
                let spec = str_arg(spec)?;
                let page = page_arg(page)?;
                self.refuse_save_project(&spec)?;
                let file = self.write_name(&spec, FileKind::Grp);
                let jpeg = matches!(&file, Ok(f) if f.kind == FileKind::Jpg);
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
                if self.ask_save(&file) {
                    return Ok(vec![]);
                }
                let image = Image {
                    width: rect.2 as usize,
                    height: rect.3 as usize,
                    pixels: self.grp.save_rect(page, rect),
                };
                let outcome = file.and_then(|file| {
                    let bytes = match file.kind {
                        FileKind::Grp => image::encode_png(&image).map_err(bad_file)?,
                        FileKind::Jpg => {
                            image::encode_jpeg(&image, quality as u8).map_err(bad_file)?
                        }
                        kind => return Err(StorageError::BadKind(kind.name().to_string())),
                    };
                    (self.storage).write_ref(&file, &bytes, Some(file.kind.backup_name()))
                });
                self.finish("SAVEG", outcome);
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }

    /// LOADV's value: a string for TXT files, an array for DAT and GRP.
    fn load_value(&self, spec: &str) -> storage::Result<Value> {
        if !spec.contains(':') {
            return Err(StorageError::BadKind(String::new()));
        }
        let file = self.read_name(spec, FileKind::Txt)?;
        let bytes = self.storage.read_ref(&file)?;
        match file.kind {
            FileKind::Txt => Ok(Value::str(&program_text(&bytes)?)),
            FileKind::Dat => {
                let (data, _) = sbfile::read_data(&bytes).map_err(bad_file)?;
                Ok(array_value(data_array(data)))
            }
            FileKind::Grp => Ok(array_value(image_array(
                image::decode(&bytes).map_err(bad_file)?,
            ))),
            FileKind::Jpg => Err(StorageError::BadKind(file.kind.name().to_string())),
        }
    }

    /// LOADG's type of file, before anything else, in a program, a
    /// subprogram or at the prompt: `PNG:` is Can't access other project
    /// in this mode, unless SmileBASIC 4's own `#` projects are involved
    /// (and then fails, as we cannot load those), `TXT:` File type
    /// mismatch, and a type that is none (`XYZ:`) Illegal format. JPG and
    /// DAT files load, and so do files named with any project.
    fn loadg_type(&self, spec: &str) -> Result<()> {
        let Some((kind, rest)) = spec.split_once(':') else {
            return Ok(());
        };
        if kind.eq_ignore_ascii_case("PNG") {
            let system = rest.starts_with('#') || self.storage.project().starts_with('#');
            return match system {
                true => Ok(()),
                false => Err(ErrorKind::CantAccessOtherProject.into()),
            };
        }
        match FileKind::from_name(kind) {
            Some(FileKind::Txt) => Err(Error::with_detail(ErrorKind::FileTypeMismatch, "TXT file")),
            Some(_) => Ok(()),
            None => Err(Error::new(ErrorKind::IllegalFormat).of_arg(0)),
        }
    }

    /// LOADG's image, and the size of its file. A DAT file that is not a
    /// 2D integer array is an Illegal file format error, not a failure
    /// (checked on hardware for a string array).
    fn load_image(&self, spec: &str) -> Result<storage::Result<(Image, u64)>> {
        let file = match self.read_name(spec, FileKind::Grp) {
            Ok(file) => file,
            Err(e) => return Ok(Err(e)),
        };
        let kind = file.kind;
        let bytes = match self.storage.read_ref(&file) {
            Ok(bytes) => bytes,
            Err(e) => return Ok(Err(e)),
        };
        if kind == FileKind::Dat
            && let Ok((data, _)) = &sbfile::read_data(&bytes)
            && !(data.dims.len() == 2 && matches!(data.values, DataValues::Int(_)))
        {
            let detail = "not a 2D integer array";
            return Err(Error::with_detail(ErrorKind::IllegalFileFormat, detail));
        }
        let stored = bytes.len() as u64;
        Ok(image::decode(&bytes).map(|i| (i, stored)).map_err(bad_file))
    }

    /// Asks before reading a file that exists, as LOAD does; whether the
    /// command must stop here for the answer.
    // LOADV and LOADG in a program do not ask, like LOAD.
    fn ask_load(&mut self, spec: &str, default: FileKind) -> bool {
        let Ok(file) = self.read_name(spec, default) else {
            return false;
        };
        if !self.storage.exists_ref(&file) {
            return false;
        }
        let file = file.label();
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
    fn ask_save(&mut self, file: &storage::Result<FileRef>) -> bool {
        let Ok(file) = file else {
            return false;
        };
        let exists = self.storage.exists_ref(file);
        let file = file.label();
        let mut details = self.details(&file, None);
        if exists {
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
