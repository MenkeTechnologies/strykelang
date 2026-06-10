//! Subset of Perl `pack` / `unpack` for binary I/O.
//! Supported: `A` `a` `N` `n` `V` `v` `C` `Q` `q` `Z` `H` `x` `w` `i` `I` `l` `L` `s` `S` `f` `d` (optional repeat count after each; `*` for some).

use std::sync::Arc;

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

#[derive(Clone, Copy, Debug)]
enum Repeat {
    One,
    Count(usize),
    Star,
}

struct Token {
    op: char,
    repeat: Repeat,
}

fn tokenize(template: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let mut it = template.chars().peekable();
    while let Some(&c) = it.peek() {
        if c.is_ascii_whitespace() {
            it.next();
            continue;
        }
        let op = it.next().unwrap();
        if !matches!(
            op,
            'A' | 'a'
                | 'N'
                | 'n'
                | 'V'
                | 'v'
                | 'C'
                | 'Q'
                | 'q'
                | 'Z'
                | 'H'
                | 'x'
                | 'w'
                | 'i'
                | 'I'
                | 'l'
                | 'L'
                | 's'
                | 'S'
                | 'f'
                | 'd'
        ) {
            return Err(format!("unsupported pack type '{}'", op));
        }
        let repeat = match it.peek() {
            Some('*') => {
                it.next();
                Repeat::Star
            }
            Some(d) if d.is_ascii_digit() => {
                let mut n = 0usize;
                while let Some(&d) = it.peek() {
                    if d.is_ascii_digit() {
                        n = n
                            .saturating_mul(10)
                            .saturating_add((d as u8 - b'0') as usize);
                        it.next();
                    } else {
                        break;
                    }
                }
                // Perl-pack parity: an explicit count of 0 means "zero items".
                // Pre-fix `.max(1)` silently coerced this to 1 — `pack "C0", 1, 2`
                // emitted one byte, and `pack "x0"` emitted a NUL. Perl emits 0
                // bytes in both cases.
                Repeat::Count(n)
            }
            _ => Repeat::One,
        };
        out.push(Token { op, repeat });
    }
    Ok(out)
}

fn repeat_fixed(r: Repeat, one: usize) -> Result<usize, String> {
    match r {
        Repeat::One => Ok(one),
        Repeat::Count(n) => Ok(n),
        Repeat::Star => Err("unexpected '*'".into()),
    }
}

fn take_arg<'a>(args: &mut &'a [StrykeValue]) -> Result<&'a StrykeValue, String> {
    if args.is_empty() {
        return Err("not enough arguments".into());
    }
    let v = &args[0];
    *args = &args[1..];
    Ok(v)
}

/// `pack TEMPLATE, LIST`
pub fn perl_pack(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime("pack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let mut rest = &args[1..];
    match pack_impl(&template, &mut rest) {
        Ok(bytes) => Ok(StrykeValue::bytes(Arc::new(bytes))),
        Err(msg) => Err(StrykeError::runtime(format!("pack: {}", msg), line)),
    }
}

fn pack_impl(template: &str, args: &mut &[StrykeValue]) -> Result<Vec<u8>, String> {
    let tokens = tokenize(template)?;
    let mut buf = Vec::new();
    for t in tokens {
        match t.op {
            'A' | 'a' => {
                let n = match t.repeat {
                    Repeat::One => 1,
                    Repeat::Count(n) => n,
                    Repeat::Star => return Err("'A' and 'a' do not support '*'".into()),
                };
                let s = take_arg(args)?.to_string();
                let bytes = s.as_bytes();
                let mut chunk = vec![0u8; n];
                let copy = n.min(bytes.len());
                chunk[..copy].copy_from_slice(&bytes[..copy]);
                if t.op == 'A' {
                    for b in chunk.iter_mut().skip(copy) {
                        *b = b' ';
                    }
                }
                buf.extend(chunk);
            }
            'Z' => match t.repeat {
                Repeat::One | Repeat::Star => {
                    let s = take_arg(args)?.to_string();
                    let mut b = s.into_bytes();
                    b.push(0);
                    buf.extend(b);
                }
                Repeat::Count(max) => {
                    let s = take_arg(args)?.to_string();
                    let mut b = s.into_bytes();
                    b.push(0);
                    if b.len() > max {
                        b.truncate(max);
                        if max > 0 {
                            b[max - 1] = 0;
                        }
                    } else {
                        while b.len() < max {
                            b.push(0);
                        }
                    }
                    buf.extend(b);
                }
            },
            'H' => {
                let s = take_arg(args)?.to_string();
                let hex: String = s.chars().filter(char::is_ascii_hexdigit).collect();
                let mut hex = match t.repeat {
                    Repeat::Star => hex,
                    Repeat::Count(n) => {
                        if hex.len() < n {
                            return Err("hex string too short".into());
                        }
                        hex[..n].to_string()
                    }
                    Repeat::One => {
                        if hex.is_empty() {
                            return Err("hex string too short".into());
                        }
                        hex[..1].to_string()
                    }
                };
                // Perl-pack parity: odd-length hex pads with a trailing '0' nibble
                // (so "abc" becomes 2 bytes 0xAB 0xC0). Pre-fix this errored out,
                // breaking otherwise-valid odd-length round-trips.
                if hex.len() % 2 != 0 {
                    hex.push('0');
                }
                let mut i = 0;
                while i < hex.len() {
                    let byte = u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string())?;
                    buf.push(byte);
                    i += 2;
                }
            }
            'N' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u32;
                    buf.extend(v.to_be_bytes());
                }
            }
            'n' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = (take_arg(args)?.to_int() as u16).to_be_bytes();
                    buf.extend(v);
                }
            }
            'V' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u32;
                    buf.extend(v.to_le_bytes());
                }
            }
            'v' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = (take_arg(args)?.to_int() as u16).to_le_bytes();
                    buf.extend(v);
                }
            }
            'C' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = (take_arg(args)?.to_int() & 0xff) as u8;
                    buf.push(v);
                }
            }
            'Q' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u64;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'q' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int();
                    buf.extend(v.to_ne_bytes());
                }
            }
            // BER compressed integer (variable-length encoding, big-endian, high bit = continuation)
            'w' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    // Perl-pack parity: pack 'w' on a negative integer errors with
                    // "Cannot compress negative numbers in pack". Pre-fix the
                    // `as u64` cast sign-laundered -1 to 0xFFFFFFFFFFFFFFFF and
                    // silently encoded ~10 BER bytes.
                    let signed = take_arg(args)?.to_int();
                    if signed < 0 {
                        return Err("Cannot compress negative numbers in pack".into());
                    }
                    let mut v = signed as u64;
                    let mut ber = Vec::new();
                    ber.push((v & 0x7f) as u8);
                    v >>= 7;
                    while v > 0 {
                        ber.push((v & 0x7f) as u8 | 0x80);
                        v >>= 7;
                    }
                    ber.reverse();
                    buf.extend(ber);
                }
            }
            'i' => {
                // Native signed int (typically 4 bytes on modern platforms)
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as i32;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'I' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u32;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'l' => {
                // Signed 32-bit, native byte order
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as i32;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'L' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u32;
                    buf.extend(v.to_ne_bytes());
                }
            }
            's' => {
                // Signed 16-bit, native byte order
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as i16;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'S' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_int() as u16;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'f' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_number() as f32;
                    buf.extend(v.to_ne_bytes());
                }
            }
            'd' => {
                let count = match t.repeat {
                    Repeat::Star => args.len(),
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    let v = take_arg(args)?.to_number();
                    buf.extend(v.to_ne_bytes());
                }
            }
            'x' => {
                let n = match t.repeat {
                    Repeat::One => 1,
                    Repeat::Count(n) => n,
                    Repeat::Star => return Err("'x' does not support '*'".into()),
                };
                buf.extend(std::iter::repeat_n(0u8, n));
            }
            _ => return Err(format!("internal: {}", t.op)),
        }
    }
    Ok(buf)
}

/// `unpack TEMPLATE, SCALAR`
pub fn perl_unpack(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("unpack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let data = value_to_bytes(&args[1]).map_err(|m| StrykeError::runtime(m, line))?;
    match unpack_impl(&template, &data) {
        Ok(vals) => {
            if vals.len() == 1 {
                Ok(vals.into_iter().next().unwrap())
            } else {
                Ok(StrykeValue::array(vals))
            }
        }
        Err(msg) => Err(StrykeError::runtime(format!("unpack: {}", msg), line)),
    }
}

fn value_to_bytes(v: &StrykeValue) -> Result<Vec<u8>, String> {
    if let Some(b) = v.as_bytes_arc() {
        return Ok((*b).clone());
    }
    if let Some(s) = v.as_str() {
        return Ok(s.as_bytes().to_vec());
    }
    Err("unpack: data must be string or packed bytes".into())
}

fn unpack_width(op: char, repeat: Repeat) -> Result<Option<usize>, String> {
    // None = variable / rest
    match op {
        'A' | 'a' => match repeat {
            Repeat::One => Ok(Some(1)),
            Repeat::Count(n) => Ok(Some(n)),
            Repeat::Star => Ok(None),
        },
        'Z' => match repeat {
            Repeat::One | Repeat::Star => Ok(None),
            Repeat::Count(n) => Ok(Some(n)),
        },
        'H' => match repeat {
            Repeat::One => Ok(Some(1)),
            Repeat::Count(n) => Ok(Some(n.div_ceil(2))),
            Repeat::Star => Ok(None),
        },
        'N' | 'V' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 4))
            }
        },
        'n' | 'v' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 2))
            }
        },
        'C' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c))
            }
        },
        'Q' | 'q' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 8))
            }
        },
        'w' => Ok(None), // variable-length
        'i' | 'I' | 'l' | 'L' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 4))
            }
        },
        's' | 'S' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 2))
            }
        },
        'f' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 4))
            }
        },
        'd' => match repeat {
            Repeat::Star => Ok(None),
            _ => {
                let c = repeat_fixed(repeat, 1)?;
                Ok(Some(c * 8))
            }
        },
        'x' => {
            let n = match repeat {
                Repeat::One => 1,
                Repeat::Count(n) => n,
                Repeat::Star => return Err("'x' cannot use '*' in unpack".into()),
            };
            Ok(Some(n))
        }
        _ => Err("internal width".into()),
    }
}

fn unpack_impl(template: &str, data: &[u8]) -> Result<Vec<StrykeValue>, String> {
    let tokens = tokenize(template)?;
    let mut pos = 0usize;
    let mut out = Vec::new();
    for t in tokens {
        let need = unpack_width(t.op, t.repeat)?;
        match t.op {
            'A' | 'a' => {
                let n = match need {
                    Some(n) => n,
                    None => data.len().saturating_sub(pos),
                };
                let end = pos + n;
                if end > data.len() {
                    return Err("unpack: data too short".into());
                }
                let slice = &data[pos..end];
                pos = end;
                let s = if t.op == 'A' {
                    // A: strip trailing spaces and NULs
                    String::from_utf8_lossy(slice)
                        .trim_end_matches([' ', '\0'])
                        .to_string()
                } else {
                    // a: return raw bytes including NULs
                    String::from_utf8_lossy(slice).to_string()
                };
                out.push(StrykeValue::string(s));
            }
            'Z' => {
                let rest = data.get(pos..).unwrap_or(&[]);
                match t.repeat {
                    Repeat::Count(max) => {
                        let take = max.min(rest.len());
                        let chunk = &rest[..take];
                        pos += take;
                        let endz = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                        out.push(StrykeValue::string(
                            String::from_utf8_lossy(&chunk[..endz]).to_string(),
                        ));
                    }
                    Repeat::One | Repeat::Star => {
                        let endz = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        let s = String::from_utf8_lossy(&rest[..endz]).to_string();
                        pos += endz + 1;
                        out.push(StrykeValue::string(s));
                    }
                }
            }
            'H' => {
                // Perl-pack parity: 'H' emits LOWERCASE hex (`%02x`, not `%02X`).
                // Repeat::One reads 1 byte but emits only 1 hex char (the high
                // nibble); Repeat::Count(n) emits exactly n chars. Pre-fix used
                // uppercase and emitted 2 chars for Repeat::One — breaking
                // byte-identical round-trips and case-sensitive consumers.
                let nbytes = match t.repeat {
                    Repeat::Star => data.len().saturating_sub(pos),
                    Repeat::Count(n) => n.div_ceil(2),
                    Repeat::One => 1,
                };
                let end = pos + nbytes;
                if end > data.len() {
                    return Err("unpack: data too short".into());
                }
                let chunk = &data[pos..end];
                pos = end;
                let mut hex = String::with_capacity(chunk.len() * 2);
                for &b in chunk {
                    hex.push_str(&format!("{:02x}", b));
                }
                match t.repeat {
                    Repeat::Count(n) if hex.len() > n => hex.truncate(n),
                    Repeat::One if hex.len() > 1 => hex.truncate(1),
                    _ => {}
                }
                out.push(StrykeValue::string(hex));
            }
            'N' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for N".into());
                    }
                    let v = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'n' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 2,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 2 > data.len() {
                        return Err("unpack: data too short for n".into());
                    }
                    let v = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap());
                    pos += 2;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'V' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for V".into());
                    }
                    let v = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'v' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 2,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 2 > data.len() {
                        return Err("unpack: data too short for v".into());
                    }
                    let v = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap());
                    pos += 2;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'C' => match t.repeat {
                Repeat::Star => {
                    while pos < data.len() {
                        out.push(StrykeValue::integer(data[pos] as i64));
                        pos += 1;
                    }
                }
                _ => {
                    let count = repeat_fixed(t.repeat, 1)?;
                    for _ in 0..count {
                        if pos >= data.len() {
                            return Err("unpack: data too short for C".into());
                        }
                        let v = data[pos];
                        pos += 1;
                        out.push(StrykeValue::integer(v as i64));
                    }
                }
            },
            'Q' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 8,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 8 > data.len() {
                        return Err("unpack: data too short for Q".into());
                    }
                    let v = u64::from_ne_bytes(data[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'q' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 8,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 8 > data.len() {
                        return Err("unpack: data too short for q".into());
                    }
                    let v = i64::from_ne_bytes(data[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    out.push(StrykeValue::integer(v));
                }
            }
            'w' => {
                // BER compressed integer
                let count = match t.repeat {
                    Repeat::Star => usize::MAX, // decode as many as possible
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                let mut decoded = 0usize;
                while decoded < count && pos < data.len() {
                    let mut val: u64 = 0;
                    loop {
                        if pos >= data.len() {
                            return Err("unpack: data too short for w".into());
                        }
                        let byte = data[pos];
                        pos += 1;
                        val = (val << 7) | (byte & 0x7f) as u64;
                        if byte & 0x80 == 0 {
                            break;
                        }
                    }
                    out.push(StrykeValue::integer(val as i64));
                    decoded += 1;
                }
            }
            'i' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for i".into());
                    }
                    let v = i32::from_ne_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'I' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for I".into());
                    }
                    let v = u32::from_ne_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'l' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for l".into());
                    }
                    let v = i32::from_ne_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'L' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for L".into());
                    }
                    let v = u32::from_ne_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            's' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 2,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 2 > data.len() {
                        return Err("unpack: data too short for s".into());
                    }
                    let v = i16::from_ne_bytes(data[pos..pos + 2].try_into().unwrap());
                    pos += 2;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'S' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 2,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 2 > data.len() {
                        return Err("unpack: data too short for S".into());
                    }
                    let v = u16::from_ne_bytes(data[pos..pos + 2].try_into().unwrap());
                    pos += 2;
                    out.push(StrykeValue::integer(v as i64));
                }
            }
            'f' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 4,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 4 > data.len() {
                        return Err("unpack: data too short for f".into());
                    }
                    let v = f32::from_ne_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    out.push(StrykeValue::float(v as f64));
                }
            }
            'd' => {
                let count = match t.repeat {
                    Repeat::Star => (data.len().saturating_sub(pos)) / 8,
                    _ => repeat_fixed(t.repeat, 1)?,
                };
                for _ in 0..count {
                    if pos + 8 > data.len() {
                        return Err("unpack: data too short for d".into());
                    }
                    let v = f64::from_ne_bytes(data[pos..pos + 8].try_into().unwrap());
                    pos += 8;
                    out.push(StrykeValue::float(v));
                }
            }
            'x' => {
                let n = match t.repeat {
                    Repeat::One => 1,
                    Repeat::Count(n) => n,
                    Repeat::Star => {
                        return Err("unpack: internal 'x' with '*'".into());
                    }
                };
                pos = pos.saturating_add(n);
                if pos > data.len() {
                    return Err("unpack: x past end".into());
                }
            }
            _ => return Err(format!("internal unpack {}", t.op)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn pack_bytes(template: &str, args: &[StrykeValue]) -> Vec<u8> {
        let mut v = vec![StrykeValue::string(template.into())];
        v.extend_from_slice(args);
        let p = perl_pack(&v, 0).expect("pack");
        p.as_bytes_arc().expect("bytes").as_ref().clone()
    }

    fn unpack_vals(template: &str, data: &[u8]) -> Vec<StrykeValue> {
        let u = perl_unpack(
            &[
                StrykeValue::string(template.into()),
                StrykeValue::bytes(Arc::new(data.to_vec())),
            ],
            0,
        )
        .expect("unpack");
        if let Some(a) = u.as_array_vec() {
            a
        } else {
            vec![u]
        }
    }

    #[test]
    fn tokenize_rejects_unsupported_type() {
        let e = perl_pack(
            &[StrykeValue::string("C X".into()), StrykeValue::integer(1)],
            0,
        )
        .expect_err("unsupported");
        assert!(
            e.message.contains("unsupported pack type"),
            "msg={}",
            e.message
        );
    }

    #[test]
    fn pack_a_pads_with_nul_a_pads_with_space() {
        assert_eq!(
            pack_bytes("a3", &[StrykeValue::string("a".into())]),
            vec![b'a', 0, 0]
        );
        assert_eq!(
            pack_bytes("A3", &[StrykeValue::string("a".into())]),
            vec![b'a', b' ', b' ']
        );
    }

    #[test]
    fn pack_z_one_appends_nul() {
        assert_eq!(
            pack_bytes("Z", &[StrykeValue::string("ab".into())]),
            vec![b'a', b'b', 0]
        );
    }

    #[test]
    fn pack_z_count_truncates_or_pads() {
        let b = pack_bytes("Z4", &[StrykeValue::string("abcdef".into())]);
        assert_eq!(b, vec![b'a', b'b', b'c', 0]);
        let b2 = pack_bytes("Z6", &[StrykeValue::string("ab".into())]);
        assert_eq!(b2, vec![b'a', b'b', 0, 0, 0, 0]);
    }

    #[test]
    fn pack_h_one_nibble_pair() {
        // Perl-pack parity: `H` (no count = Repeat::One) reads ONE nibble, not
        // two. Pre-fix this took 2 chars and emitted 0xff; Perl emits 0xf0 (the
        // high nibble of the lone nibble).
        //   perl -e 'printf "%v02X\n", pack("H", "ff")'  → F0
        assert_eq!(
            pack_bytes("H", &[StrykeValue::string("ff".into())]),
            vec![0xf0]
        );
    }

    #[test]
    fn pack_h_two_nibbles_from_template_count() {
        assert_eq!(
            pack_bytes("H4", &[StrykeValue::string("dead".into())]),
            vec![0xde, 0xad]
        );
    }

    #[test]
    fn pack_h_pads_odd_hex_length_with_trailing_zero_nibble() {
        // FIXED for Perl-pack parity: odd-length hex pads with a trailing '0'
        // nibble (so "abc" produces 2 bytes 0xAB 0xC0). Pre-fix this errored
        // with "hex length must be even".
        //   perl -e 'printf "%v02X\n", pack("H*", "abc")'  → AB.C0
        let bytes = pack_bytes("H*", &[StrykeValue::string("abc".into())]);
        assert_eq!(bytes, vec![0xab, 0xc0]);
    }

    #[test]
    fn pack_h_star_ignores_non_hex_separators() {
        assert_eq!(
            pack_bytes("H*", &[StrykeValue::string("DE-AD".into())]),
            vec![0xde, 0xad]
        );
    }

    #[test]
    fn pack_x_inserts_zeros() {
        assert_eq!(pack_bytes("x3", &[]), vec![0, 0, 0]);
        assert_eq!(
            pack_bytes(
                "C x2 C",
                &[StrykeValue::integer(1), StrykeValue::integer(2)]
            ),
            vec![1, 0, 0, 2]
        );
    }

    #[test]
    fn pack_star_rejects_a() {
        let e = perl_pack(
            &[
                StrykeValue::string("a*".into()),
                StrykeValue::string("x".into()),
            ],
            0,
        )
        .expect_err("a*");
        assert!(e.message.contains("do not support"), "{}", e.message);
    }

    #[test]
    fn pack_not_enough_arguments() {
        let e = perl_pack(
            &[StrykeValue::string("C C".into()), StrykeValue::integer(1)],
            0,
        )
        .expect_err("short");
        assert!(e.message.contains("not enough"), "{}", e.message);
    }

    #[test]
    fn pack_empty_args_list() {
        let e = perl_pack(&[], 0).expect_err("no args");
        assert!(e.message.contains("not enough"), "{}", e.message);
    }

    #[test]
    fn unpack_n_v() {
        let be = perl_pack(
            &[
                StrykeValue::string("N".into()),
                StrykeValue::integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let b = be.as_bytes_arc().expect("expected Bytes");
        assert_eq!(b.as_ref(), &[1, 2, 3, 4]);

        let le = perl_pack(
            &[
                StrykeValue::string("V".into()),
                StrykeValue::integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let b2 = le.as_bytes_arc().expect("expected Bytes");
        assert_eq!(b2.as_ref(), &[4, 3, 2, 1]);

        let u = perl_unpack(
            &[
                StrykeValue::string("N".into()),
                StrykeValue::bytes(Arc::new(vec![0, 0, 0, 42])),
            ],
            0,
        )
        .unwrap();
        assert_eq!(u.to_int(), 42);
    }

    #[test]
    fn pack_c_star_roundtrip() {
        let p = perl_pack(
            &[
                StrykeValue::string("C*".into()),
                StrykeValue::integer(65),
                StrykeValue::integer(66),
            ],
            0,
        )
        .unwrap();
        let b = p.as_bytes_arc().expect("expected Bytes");
        let u = perl_unpack(
            &[
                StrykeValue::string("C*".into()),
                StrykeValue::bytes(Arc::clone(&b)),
            ],
            0,
        )
        .unwrap();
        let vals = u.as_array_vec().expect("expected array");
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0].to_int(), 65);
        assert_eq!(vals[1].to_int(), 66);
    }

    #[test]
    fn unpack_a_trims_space_padding_unpack_z_reads_c_string() {
        let v = unpack_vals("A4", b"hi  ");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].to_string(), "hi");

        let v2 = unpack_vals("Z6", &[b'a', b'b', 0, 0, 0, 0]);
        assert_eq!(v2[0].to_string(), "ab");
    }

    #[test]
    fn unpack_n_reports_short_buffer() {
        let e = perl_unpack(
            &[
                StrykeValue::string("N".into()),
                StrykeValue::bytes(Arc::new(vec![1, 2])),
            ],
            0,
        )
        .expect_err("short");
        assert!(e.message.contains("too short"), "{}", e.message);
    }

    #[test]
    fn unpack_x_skips_bytes() {
        let v = unpack_vals("x2 C", &[0u8, 0, 7]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].to_int(), 7);
    }

    #[test]
    fn pack_n_star_consumes_all_remaining_args() {
        let b = pack_bytes("N*", &[StrykeValue::integer(1), StrykeValue::integer(2)]);
        assert_eq!(b.len(), 8);
        let v = unpack_vals("N*", &b);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_int(), 1);
        assert_eq!(v[1].to_int(), 2);
    }

    #[test]
    fn pack_n2_two_big_endian_words() {
        let b = pack_bytes("N2", &[StrykeValue::integer(1), StrykeValue::integer(2)]);
        assert_eq!(b, vec![0, 0, 0, 1, 0, 0, 0, 2]);
    }

    #[test]
    fn pack_v_and_n_endian_differ() {
        let le = pack_bytes("v", &[StrykeValue::integer(0x0102)]);
        let be = pack_bytes("n", &[StrykeValue::integer(0x0102)]);
        assert_eq!(le, vec![0x02, 0x01]);
        assert_eq!(be, vec![0x01, 0x02]);
    }

    #[test]
    fn pack_q_signed_roundtrip() {
        let n = -42i64;
        let b = pack_bytes("q", &[StrykeValue::integer(n)]);
        assert_eq!(b.len(), 8);
        let v = unpack_vals("q", &b);
        assert_eq!(v[0].to_int(), n);
    }

    #[test]
    fn unpack_from_string_scalar_accepted() {
        let u = perl_unpack(
            &[
                StrykeValue::string("C".into()),
                StrykeValue::string("\x07".into()),
            ],
            0,
        )
        .expect("unpack from str");
        assert_eq!(u.to_int(), 7);
    }

    #[test]
    fn unpack_rejects_non_string_data() {
        let e = perl_unpack(
            &[StrykeValue::string("C".into()), StrykeValue::integer(99)],
            0,
        )
        .expect_err("type");
        assert!(e.message.contains("string or packed"), "{}", e.message);
    }

    #[test]
    fn whitespace_in_template_is_skipped() {
        assert_eq!(
            pack_bytes("C  C", &[StrykeValue::integer(1), StrykeValue::integer(2)]),
            vec![1, 2]
        );
    }

    #[test]
    fn test_pack_unpack_ber_w() {
        // BER (w) test: 128 -> 0x81 0x00
        let cases = vec![
            (0, vec![0]),
            (127, vec![127]),
            (128, vec![0x81, 0x00]),
            (16383, vec![0xff, 0x7f]),
            (16384, vec![0x81, 0x80, 0x00]),
        ];
        for (val, expected) in cases {
            let b = pack_bytes("w", &[StrykeValue::integer(val)]);
            assert_eq!(b, expected, "pack failed for {}", val);
            let u = unpack_vals("w", &b);
            assert_eq!(u[0].to_int(), val, "unpack failed for {}", val);
        }
    }

    #[test]
    fn test_pack_unpack_floats() {
        let f_val = 1.25f32;
        let d_val = std::f64::consts::PI;

        let b_f = pack_bytes("f", &[StrykeValue::float(f_val as f64)]);
        assert_eq!(b_f.len(), 4);
        let u_f = unpack_vals("f", &b_f);
        assert!((u_f[0].to_number() - f_val as f64).abs() < 1e-7);

        let b_d = pack_bytes("d", &[StrykeValue::float(d_val)]);
        assert_eq!(b_d.len(), 8);
        let u_d = unpack_vals("d", &b_d);
        assert_eq!(u_d[0].to_number(), d_val);
    }

    #[test]
    fn test_pack_unpack_native_types() {
        // s (signed 16), S (unsigned 16)
        let b_s = pack_bytes("s", &[StrykeValue::integer(-32768)]);
        assert_eq!(b_s.len(), 2);
        assert_eq!(unpack_vals("s", &b_s)[0].to_int(), -32768);

        let b_s_u16 = pack_bytes("S", &[StrykeValue::integer(65535)]);
        assert_eq!(b_s_u16.len(), 2);
        assert_eq!(unpack_vals("S", &b_s_u16)[0].to_int(), 65535);

        // i (native signed int), I (native unsigned int) - usually 4 bytes
        let b_i = pack_bytes("i", &[StrykeValue::integer(-123456)]);
        assert_eq!(b_i.len(), 4);
        assert_eq!(unpack_vals("i", &b_i)[0].to_int(), -123456);

        let b_i_u32 = pack_bytes("I", &[StrykeValue::integer(123456)]);
        assert_eq!(b_i_u32.len(), 4);
        assert_eq!(unpack_vals("I", &b_i_u32)[0].to_int(), 123456);

        // l (32-bit signed), L (32-bit unsigned)
        let b_l = pack_bytes("l", &[StrykeValue::integer(-2147483648)]);
        assert_eq!(b_l.len(), 4);
        assert_eq!(unpack_vals("l", &b_l)[0].to_int(), -2147483648);

        let b_l_u32 = pack_bytes("L", &[StrykeValue::integer(4294967295)]);
        assert_eq!(b_l_u32.len(), 4);
        assert_eq!(unpack_vals("L", &b_l_u32)[0].to_int(), 4294967295);
    }

    #[test]
    fn test_unpack_h_star_odd_nibbles() {
        // Perl-pack parity: `H` emits LOWERCASE hex. Pre-fix the implementation
        // used `{:02X}` (uppercase) — broke byte-identical round-trips through
        // pack/unpack. perl -e 'print unpack("H*", pack("C3", 0xde, 0xad, 0xbe))'
        // → deadbe (not DEADBE).
        let data = vec![0xDE, 0xAD, 0xBE];
        let v = unpack_vals("H5", &data);
        assert_eq!(v[0].to_string(), "deadb");

        let v2 = unpack_vals("H*", &data);
        assert_eq!(v2[0].to_string(), "deadbe");
    }

    #[test]
    fn test_unpack_multiple_tokens() {
        let data = vec![0x41, 0x00, 0x00, 0x2A]; // 'A', 0, 0, 42
        let v = unpack_vals("A1 x2 C", &data);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_string(), "A");
        assert_eq!(v[1].to_int(), 42);
    }

    #[test]
    fn test_pack_unpack_z_star() {
        let b = pack_bytes("Z*", &[StrykeValue::string("hello".into())]);
        assert_eq!(b, b"hello\0");
        let v = unpack_vals("Z*", &b);
        assert_eq!(v[0].to_string(), "hello");
    }
}
