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
use sb_core::value::{Array, ArrayData, ArrayRef, Value, new_str, real_to_int};

use crate::confirm::FileAction;
use crate::files::{check_file_name, program_text, str_arg};
use crate::graphics::{GRP_SIZE, PAGES};
use crate::image::{self, Image};
use crate::runtime::{Command, Runtime, checking, no_form, opt_int};
use crate::sbfile::{self, Data, DataValues};
use crate::storage::{self, FileKind, FileRef, NamedKind, StorageError};

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

/// LOADV's and SAVEV's type of file, once the name is checked and before
/// anything else: `JPG:`, `PNG:` and a name without its type are File
/// type mismatch, with no argument's number. SAVEV checks it before the
/// project (see [`Runtime::refuse_save_project`]).
fn value_type(spec: &str) -> Result<()> {
    let detail = match spec.split_once(':') {
        None => "no type".to_string(),
        Some((kind, _)) if ["JPG", "PNG"].iter().any(|k| kind.eq_ignore_ascii_case(k)) => {
            format!("{} file", kind.to_ascii_uppercase())
        }
        Some(_) => return Ok(()),
    };
    Err(Error::with_detail(ErrorKind::FileTypeMismatch, detail))
}

/// SAVEG's type of file, checked as SAVEV's is: GRP, JPG or none, which
/// is GRP. `TXT:`, `DAT:` and `PNG:` are File type mismatch.
fn saveg_type(spec: &str) -> Result<()> {
    match storage::split_name(spec).and_then(|parts| parts.kind) {
        None | Some(NamedKind::File(FileKind::Grp | FileKind::Jpg)) => Ok(()),
        Some(kind) => {
            let detail = format!("{} file", kind.name());
            Err(Error::with_detail(ErrorKind::FileTypeMismatch, detail))
        }
    }
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

/// The most SmileBASIC 4 saves of a DAT or GRP file's PCBN body, in bytes.
const MAX_SAVED: usize = 0x100_0000;

/// SmileBASIC 4 stores a DAT or GRP file's PCBN body compressed, or as it
/// is when compressed is still too large; too large both ways is Array is
/// too large, after the questions. Ours writes the
/// body as it is, so this only checks, and compresses only what is too
/// large as it is.
fn check_saved_size(raw: usize, body: impl FnOnce() -> Vec<u8>) -> Result<()> {
    if raw > MAX_SAVED && miniz_oxide::deflate::compress_to_vec_zlib(&body(), 6).len() > MAX_SAVED {
        return Err(ErrorKind::ArrayTooLarge.into());
    }
    Ok(())
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

/// SAVEG's rectangle: `x, y, width, height` on a 2048-pixel page, each
/// checked in turn.
fn rect_arg(args: &[Value]) -> Result<(i32, i32, i32, i32)> {
    let int = |i: usize, what: &str, range: std::ops::Range<i32>| {
        checking(&args[i], || {
            let n = args[i].to_int()?;
            match range.contains(&n) {
                true => Ok(n),
                false => Err(out_of_range(what, n)),
            }
        })
    };
    let x = int(0, "X", 0..GRP_SIZE)?;
    let y = int(1, "Y", 0..GRP_SIZE)?;
    let w = int(2, "width", 1..GRP_SIZE + 1)?;
    let h = int(3, "height", 1..GRP_SIZE + 1)?;
    // SmileBASIC 4 names the argument before the size that is too large:
    // a rectangle too wide is (SAVEG:4), one too tall (SAVEG:5).
    let past = || Error::with_detail(ErrorKind::OutOfRange, "past the page");
    if x + w > GRP_SIZE {
        return Err(past().of_value(&args[1]));
    }
    if y + h > GRP_SIZE {
        return Err(past().of_value(&args[2]));
    }
    Ok((x, y, w, h))
}

/// SAVEG's quality, 1 to 100: taken for any type of file, and used only
/// for JPG files. A GRP file keeps every pixel.
fn quality_arg(v: Option<&Value>) -> Result<i32> {
    let Some(v) = v else {
        return Ok(JPEG_QUALITY);
    };
    checking(v, || match opt_int(v)? {
        None => Ok(JPEG_QUALITY),
        Some(q @ 1..=100) => Ok(q),
        Some(q) => Err(out_of_range("compression", q)),
    })
}

impl Runtime {
    pub(crate) fn loadsave_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        self.after_failure(|rt| rt.loadsave_command_now(command, args, want))
    }

    fn loadsave_command_now(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args) {
            (Command::Loadv, Want::Value, [spec]) => {
                let spec = checking(spec, || str_arg(spec))?;
                check_file_name(&spec, 0)?;
                value_type(&spec)?;
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
                let spec = checking(spec, || str_arg(spec))?;
                check_file_name(&spec, 0)?;
                value_type(&spec)?;
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
            // A name it cannot load is refused before the count of
            // arguments and OUT variables.
            // Then a TXT file wants one argument and one OUT variable, the
            // argument count first; DAT and GRP files the OUT count first,
            // 0 or 1.
            (Command::Loadv, _, [spec, ..]) => {
                let spec = checking(spec, || str_arg(spec))?;
                check_file_name(&spec, 0)?;
                value_type(&spec)?;
                let outs = want.count();
                let out_error = match read_kind(&spec) {
                    Some(FileKind::Txt) => args.len() == 1 && outs != 1,
                    _ => outs > 1,
                };
                match out_error {
                    true => Err(ErrorKind::WrongNumberOfOutArguments.into()),
                    // One OUT variable runs as the function.
                    false => Err(no_form()),
                }
            }
            (Command::Savev, Want::Outs(0), [spec, value]) => {
                let spec = checking(spec, || str_arg(spec))?;
                check_file_name(&spec, 0)?;
                value_type(&spec)?;
                self.refuse_save_project(&spec)?;
                let file = self.write_name(&spec, FileKind::Txt);
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
                if self.ask_save(&file) {
                    return Ok(vec![]);
                }
                match (file.as_ref().map(|f| f.kind), value) {
                    (Ok(FileKind::Dat), _) => check_saved_size(bytes.len(), || bytes.clone())?,
                    // SmileBASIC 4 saves the image as 32-bit integers.
                    (Ok(FileKind::Grp), Value::Array(a)) => {
                        let raw = sbfile::DATA_HEADER + a.borrow().len() * 4;
                        check_saved_size(raw, || {
                            let mut data = array_data(&a.borrow());
                            if let DataValues::Real(v) = &data.values {
                                let ints = v.iter().map(|&x| real_to_int(x).unwrap_or(0));
                                data.values = DataValues::Int(ints.collect());
                            }
                            sbfile::data_body(&data)
                        })?;
                    }
                    _ => {}
                }
                let outcome = file.and_then(|file| {
                    (self.storage).write_ref(&file, &bytes, Some(file.kind.backup_name()))
                });
                self.finish("SAVEV", outcome);
                Ok(vec![])
            }
            (Command::Loadg, Want::Outs(0 | 2), [spec, page, rest @ ..])
                if matches!(rest.len(), 0 | 2) =>
            {
                let spec = checking(spec, || str_arg(spec))?;
                // The page first, then the name, then its type.
                let page = page_arg(page)?;
                check_file_name(&spec, 0)?;
                self.loadg_type(&spec)?;
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
                            meter.charge_read(&spec, stored, false);
                        }
                        self.grp.put_image(page, at, &image);
                        Ok(sizes(image.width, image.height))
                    }
                    None => Ok(sizes(0, 0)),
                }
            }
            (Command::Saveg, Want::Outs(0), [spec, page, rest @ ..])
                if matches!(rest.len(), 0 | 1 | 4 | 5) =>
            {
                // The page, the rectangle and the quality come before the
                // name, as SmileBASIC 4 checks them.
                let spec = checking(spec, || str_arg(spec))?;
                let page = page_arg(page)?;
                let (rect, quality) = match rest {
                    [] | [_] => ((0, 0, GRP_SIZE, GRP_SIZE), rest.first()),
                    _ => (rect_arg(&rest[..4])?, rest.get(4)),
                };
                let quality = quality_arg(quality)?;
                check_file_name(&spec, 0)?;
                saveg_type(&spec)?;
                self.refuse_save_project(&spec)?;
                let file = self.write_name(&spec, FileKind::Grp);
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

    /// LOADG's type of file, once the name is checked, in a program, a
    /// subprogram or at the prompt: `PNG:` is Can't access other project
    /// in this mode, unless SmileBASIC 4's own `#` projects are involved
    /// (and then fails, as we cannot load those), and `TXT:` File type
    /// mismatch. A type that is none (`XYZ:`) is Illegal format, as the
    /// name is. JPG and DAT files load, and so do files named with any
    /// project.
    fn loadg_type(&self, spec: &str) -> Result<()> {
        let Some(parts) = storage::split_name(spec) else {
            return Ok(());
        };
        match parts.kind {
            Some(NamedKind::Png) => {
                // `PNG:#NAME` is a file of the current project, not a
                // system project's.
                let named = parts.project.is_some_and(|p| p.starts_with('#'));
                match named || self.storage.project().starts_with('#') {
                    true => Ok(()),
                    false => Err(ErrorKind::CantAccessOtherProject.into()),
                }
            }
            Some(NamedKind::File(FileKind::Txt)) => {
                Err(Error::with_detail(ErrorKind::FileTypeMismatch, "TXT file"))
            }
            _ => Ok(()),
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
            originals: Vec::new(),
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
        let originals = self.originals_of(file, Some(file.kind.backup_name()));
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
            originals,
        };
        let ask = self.confirm_file(action, &[]);
        self.held(ask)
    }
}
