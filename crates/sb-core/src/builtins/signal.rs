//! Signal processing: FFT, IFFT, FFTWFN, BIQUAD and BQPARAM, and CLASSIFY.

use std::f64::consts::{LN_2, PI};

use crate::error::{Error, ErrorKind, Result};
use crate::value::{Array, ArrayRef, ElemType, Value};

use super::opt;

/// BIQUAD's and BQPARAM's coefficient array: five coefficients, then the
/// last two inputs and outputs of each channel.
const COEFFICIENTS: usize = 13;

fn number_array(v: &Value) -> Result<ArrayRef> {
    let a = v.clone().into_array()?;
    if a.borrow().elem_type() == ElemType::Str {
        return Err(ErrorKind::TypeMismatch.into());
    }
    Ok(a)
}

fn reals(a: &Array) -> Result<Vec<f64>> {
    (0..a.len()).map(|i| a.get(i).to_real()).collect()
}

fn store(a: &ArrayRef, values: &[f64]) -> Result<()> {
    let mut a = a.borrow_mut();
    for (i, &x) in values.iter().enumerate() {
        a.set(i, Value::Real(x))?;
    }
    Ok(())
}

/// CLASSIFY: 0 for a finite number, 1 for infinity, 2 for NaN.
pub(super) fn classify(args: &[Value]) -> Result<Value> {
    let x = args[0].to_real()?;
    Ok(Value::Int(match x {
        x if x.is_nan() => 2,
        x if x.is_infinite() => 1,
        _ => 0,
    }))
}

/// The sine and cosine of `k / len` of a turn, exact at quarter turns, so
/// that small transforms give exact zeros: SmileBASIC 4's IFFT of
/// 1,2,3,4 prints 0 where `sin(PI)` would leave -0.
fn turn(k: usize, len: usize) -> (f64, f64) {
    if (4 * k).is_multiple_of(len) {
        return [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)][4 * k / len % 4];
    }
    (2.0 * PI * k as f64 / len as f64).sin_cos()
}

/// In-place radix-2 transform; `inverse` turns the sign of the angle.
fn transform(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = turn(k, len);
                let s = sign * s;
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// FFT and IFFT: `out_re, out_im, in_re, in_im [, window]`, all arrays of
/// one power-of-two length; the outputs may be the inputs.
// The forward transform is not scaled and the inverse divides by the
// length; a bad length is an Illegal function call.
fn fourier(args: &[Value], inverse: bool) -> Result<Vec<Value>> {
    let arrays = args.iter().map(number_array).collect::<Result<Vec<_>>>()?;
    let n = arrays[0].borrow().len();
    if !n.is_power_of_two() || arrays.iter().any(|a| a.borrow().len() != n) {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "array length",
        ));
    }
    let mut re = reals(&arrays[2].borrow())?;
    let mut im = reals(&arrays[3].borrow())?;
    if let Some(window) = arrays.get(4) {
        let w = reals(&window.borrow())?;
        for i in 0..n {
            re[i] *= w[i];
            im[i] *= w[i];
        }
    }
    transform(&mut re, &mut im, inverse);
    if inverse {
        let scale = 1.0 / n as f64;
        re.iter_mut().chain(im.iter_mut()).for_each(|x| *x *= scale);
    }
    store(&arrays[0], &re)?;
    store(&arrays[1], &im)?;
    Ok(vec![])
}

pub(super) fn fft(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    fourier(args, false)
}

pub(super) fn ifft(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    fourier(args, true)
}

/// FFTWFN array, type: fills the array with `#WFRECT`, `#WFHAMM`,
/// `#WFHANN` or `#WFBLKM`, in the periodic forms that divide by the
/// length, which must be a power of two (both checked on hardware).
pub(super) fn fftwfn(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let array = number_array(&args[0])?;
    let kind = args[1].to_int()?;
    if !(0..=3).contains(&kind) {
        return Err(Error::with_detail(ErrorKind::OutOfRange, "window type"));
    }
    let n = array.borrow().len();
    if !n.is_power_of_two() {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "array length",
        ));
    }
    let span = n as f64;
    let values: Vec<f64> = (0..n)
        .map(|i| {
            let x = 2.0 * PI * i as f64 / span;
            match kind {
                0 => 1.0,
                1 => 0.54 - 0.46 * x.cos(),
                2 => 0.5 - 0.5 * x.cos(),
                _ => 0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos(),
            }
        })
        .collect();
    store(&array, &values)?;
    Ok(vec![])
}

fn coefficient_array(v: &Value) -> Result<ArrayRef> {
    let a = number_array(v)?;
    if a.borrow().len() < COEFFICIENTS {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "fewer than 13 coefficients",
        ));
    }
    Ok(a)
}

/// The channels of a sound array: one for a 1D array, two for a 2D one
/// whose first size is 2.
fn channels(a: &Array) -> Result<(usize, usize)> {
    match a.dims[..] {
        [len] => Ok((1, len)),
        [2, len] => Ok((2, len)),
        _ => Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "not mono or stereo",
        )),
    }
}

/// BIQUAD out, in, coefficients: filters each channel, carrying the last
/// two inputs and outputs over from the previous call in the
/// coefficients' elements 5 to 8 (mono or left) and 9 to 12 (right).
// Each four is in[t-1], in[t-2], out[t-1], out[t-2]. Output and input
// must have as many channels. An output shorter than the input is an
// error naming the input; in a longer one only the input's length is
// filtered, and the rest is left alone.
pub(super) fn biquad(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let output = number_array(&args[0])?;
    let input = number_array(&args[1])?;
    let params = coefficient_array(&args[2])?;
    let mut fp = reals(&params.borrow())?;
    let (samples, ch, len, out_len) = {
        let input = input.borrow();
        let (ch, len) = channels(&input)?;
        let (out_ch, out_len) = channels(&output.borrow())?;
        if out_ch != ch {
            return Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                "output and input differ in channels",
            ));
        }
        if out_len < len {
            return Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                "output shorter than input",
            )
            .of_value(&args[1]));
        }
        (reals(&input)?, ch, len, out_len)
    };
    let mut result = vec![0.0; samples.len()];
    for c in 0..ch {
        let h = 5 + 4 * c;
        let [mut x1, mut x2, mut y1, mut y2] = [fp[h], fp[h + 1], fp[h + 2], fp[h + 3]];
        for t in 0..len {
            let x = samples[c * len + t];
            let y = fp[0] * x + fp[1] * x1 + fp[2] * x2 - fp[3] * y1 - fp[4] * y2;
            result[c * len + t] = y;
            (x2, x1, y2, y1) = (x1, x, y1, y);
        }
        fp[h..h + 4].copy_from_slice(&[x1, x2, y1, y2]);
    }
    if out_len == len {
        store(&output, &result)?;
    } else {
        let mut output = output.borrow_mut();
        for c in 0..ch {
            for t in 0..len {
                output.set(c * out_len + t, Value::Real(result[c * len + t]))?;
            }
        }
    }
    store(&params, &fp)?;
    Ok(vec![])
}

/// BQPARAM coefficients, type, rate, cutoff, Q or bandwidth [, gain]: the
/// coefficients of Robert Bristow-Johnson's Audio EQ Cookbook, divided by
/// a0, with the band-pass at a 0 dB peak and shelves taking Q (all eight
/// types match hardware to five places). Elements 5 to 12 are left alone.
// Checked on hardware: the gain is given for types 5 to 7 only, else
// Wrong number of arguments, checked first; a zero rate or Q is an Illegal
// function call.
pub(super) fn bqparam(args: &[Value], _outs: usize) -> Result<Vec<Value>> {
    let params = coefficient_array(&args[0])?;
    let kind = args[1].to_int()?;
    if !(0..=7).contains(&kind) {
        return Err(Error::with_detail(ErrorKind::OutOfRange, "filter type"));
    }
    let rate = args[2].to_real()?;
    let cutoff = args[3].to_real()?;
    let q = args[4].to_real()?;
    let gain = match (opt(args, 5), kind >= 5) {
        (Some(v), true) => v.to_real()?,
        (None, false) => 0.0,
        _ => return Err(ErrorKind::WrongNumberOfArguments.into()),
    };
    if rate == 0.0 {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "sampling rate",
        ));
    }
    if q == 0.0 {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "Q or bandwidth",
        ));
    }
    let w0 = 2.0 * PI * cutoff / rate;
    let (sin, cos) = w0.sin_cos();
    let alpha = match kind {
        3 | 4 | 7 => sin * (LN_2 / 2.0 * q * w0 / sin).sinh(),
        _ => sin / (2.0 * q),
    };
    let a = 10f64.powf(gain / 40.0);
    let shelf = 2.0 * a.sqrt() * alpha;
    let [b0, b1, b2, a0, a1, a2] = match kind {
        0 => [
            1.0 - alpha,
            -2.0 * cos,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ],
        1 => [
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ],
        2 => [
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        ],
        3 => [alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        4 => [1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        5 => [
            a * ((a + 1.0) - (a - 1.0) * cos + shelf),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - shelf),
            (a + 1.0) + (a - 1.0) * cos + shelf,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - shelf,
        ],
        6 => [
            a * ((a + 1.0) + (a - 1.0) * cos + shelf),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - shelf),
            (a + 1.0) - (a - 1.0) * cos + shelf,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - shelf,
        ],
        _ => [
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        ],
    };
    store(&params, &[b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0])?;
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ArrayData;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn real_array(values: &[f64]) -> Value {
        let mut a = Array::new(ElemType::Real, vec![values.len()]);
        a.data = ArrayData::Real(values.to_vec());
        Value::Array(Rc::new(RefCell::new(a)))
    }

    fn values(v: &Value) -> Vec<f64> {
        let Value::Array(a) = v else { panic!() };
        reals(&a.borrow()).unwrap()
    }

    fn close(a: &[f64], b: &[f64]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn classifies_numbers() {
        let c = |x: Value| classify(&[x]).unwrap();
        assert!(matches!(c(Value::Int(3)), Value::Int(0)));
        assert!(matches!(c(Value::Real(0.5)), Value::Int(0)));
        assert!(matches!(c(Value::Real(f64::INFINITY)), Value::Int(1)));
        assert!(matches!(c(Value::Real(f64::NEG_INFINITY)), Value::Int(1)));
        assert!(matches!(c(Value::Real(f64::NAN)), Value::Int(2)));
    }

    #[test]
    fn fft_and_back() {
        let re = real_array(&[1.0, 2.0, 3.0, 4.0]);
        let im = real_array(&[0.0; 4]);
        fft(&[re.clone(), im.clone(), re.clone(), im.clone()], 0).unwrap();
        assert!(close(&values(&re), &[10.0, -2.0, -2.0, -2.0]));
        assert!(close(&values(&im), &[0.0, 2.0, 0.0, -2.0]));
        ifft(&[re.clone(), im.clone(), re.clone(), im.clone()], 0).unwrap();
        assert!(close(&values(&re), &[1.0, 2.0, 3.0, 4.0]));
        assert!(close(&values(&im), &[0.0; 4]));

        let three = real_array(&[0.0; 3]);
        let err = fft(&[three.clone(), three.clone(), three.clone(), three], 0);
        assert_eq!(err.unwrap_err().kind, ErrorKind::IllegalFunctionCall);
    }

    #[test]
    fn windows() {
        let w = real_array(&[0.0; 4]);
        fftwfn(&[w.clone(), Value::Int(2)], 0).unwrap();
        assert!(close(&values(&w), &[0.0, 0.5, 1.0, 0.5]));
        fftwfn(&[w.clone(), Value::Int(0)], 0).unwrap();
        assert!(close(&values(&w), &[1.0; 4]));
        assert!(fftwfn(&[w, Value::Int(4)], 0).is_err());
        let five = real_array(&[0.0; 5]);
        let err = fftwfn(&[five, Value::Int(0)], 0).unwrap_err();
        assert_eq!(err.kind, ErrorKind::IllegalFunctionCall);
    }

    #[test]
    fn biquad_keeps_its_history() {
        // y[t] = x[t] + 0.5 * y[t-1]
        let mut fp = vec![0.0; COEFFICIENTS];
        fp[0] = 1.0;
        fp[3] = -0.5;
        let fp = real_array(&fp);
        let input = real_array(&[1.0, 0.0, 0.0]);
        let output = real_array(&[0.0; 3]);
        biquad(&[output.clone(), input.clone(), fp.clone()], 0).unwrap();
        assert!(close(&values(&output), &[1.0, 0.5, 0.25]));
        let zero = real_array(&[0.0; 3]);
        biquad(&[output.clone(), zero, fp.clone()], 0).unwrap();
        assert!(close(&values(&output), &[0.125, 0.0625, 0.03125]));
        assert!(close(&values(&fp)[5..9], &[0.0, 0.0, 0.03125, 0.0625]));
    }

    #[test]
    fn low_pass_passes_dc() {
        let fp = real_array(&[9.0; COEFFICIENTS]);
        let args = [
            fp.clone(),
            Value::Int(1),
            Value::Int(48000),
            Value::Int(8000),
        ];
        let q = Value::Real(1.0 / 2f64.sqrt());
        bqparam(
            &[
                args[0].clone(),
                args[1].clone(),
                args[2].clone(),
                args[3].clone(),
                q,
            ],
            0,
        )
        .unwrap();
        let c = values(&fp);
        // The gain at 0 Hz is (b0+b1+b2)/(1+a1+a2).
        assert!(((c[0] + c[1] + c[2]) / (1.0 + c[3] + c[4]) - 1.0).abs() < 1e-9);
        assert!(close(&c[5..], &[9.0; 8]));
        let shelf = [
            fp,
            Value::Int(5),
            Value::Int(48000),
            Value::Int(8000),
            Value::Int(1),
        ];
        assert_eq!(
            bqparam(&shelf, 0).unwrap_err().kind,
            ErrorKind::WrongNumberOfArguments
        );
    }
}
