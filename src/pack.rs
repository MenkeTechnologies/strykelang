//! Subset of Perl `pack` / `unpack` for binary I/O.
//! Supported: `A` `a` `N` `n` `V` `v` `C` `Q` `q` `Z` `H` `x` (optional repeat count after each; `*` for some).

use std::sync::Arc;

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

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
            'A' | 'a' | 'N' | 'n' | 'V' | 'v' | 'C' | 'Q' | 'q' | 'Z' | 'H' | 'x'
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
                Repeat::Count(n.max(1))
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

fn take_arg<'a>(args: &mut &'a [PerlValue]) -> Result<&'a PerlValue, String> {
    if args.is_empty() {
        return Err("not enough arguments".into());
    }
    let v = &args[0];
    *args = &args[1..];
    Ok(v)
}

/// `pack TEMPLATE, LIST`
pub fn perl_pack(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime("pack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let mut rest = &args[1..];
    match pack_impl(&template, &mut rest) {
        Ok(bytes) => Ok(PerlValue::Bytes(Arc::new(bytes))),
        Err(msg) => Err(PerlError::runtime(format!("pack: {}", msg), line)),
    }
}

fn pack_impl(template: &str, args: &mut &[PerlValue]) -> Result<Vec<u8>, String> {
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
                let hex = match t.repeat {
                    Repeat::Star => hex,
                    Repeat::Count(n) => {
                        if hex.len() < n {
                            return Err("hex string too short".into());
                        }
                        hex[..n].to_string()
                    }
                    Repeat::One => {
                        if hex.len() < 2 {
                            return Err("hex string too short (need 2 nibbles)".into());
                        }
                        hex[..2].to_string()
                    }
                };
                if hex.len() % 2 != 0 {
                    return Err("hex length must be even".into());
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
pub fn perl_unpack(args: &[PerlValue], line: usize) -> PerlResult<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime("unpack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let data = value_to_bytes(&args[1]).map_err(|m| PerlError::runtime(m, line))?;
    match unpack_impl(&template, &data) {
        Ok(vals) => {
            if vals.len() == 1 {
                Ok(vals.into_iter().next().unwrap())
            } else {
                Ok(PerlValue::Array(vals))
            }
        }
        Err(msg) => Err(PerlError::runtime(format!("unpack: {}", msg), line)),
    }
}

fn value_to_bytes(v: &PerlValue) -> Result<Vec<u8>, String> {
    match v {
        PerlValue::Bytes(b) => Ok((**b).clone()),
        PerlValue::String(s) => Ok(s.as_bytes().to_vec()),
        _ => Err("unpack: data must be string or packed bytes".into()),
    }
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

fn unpack_impl(template: &str, data: &[u8]) -> Result<Vec<PerlValue>, String> {
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
                    String::from_utf8_lossy(slice)
                        .trim_end_matches([' ', '\0'])
                        .to_string()
                } else {
                    let endz = slice.iter().position(|&b| b == 0).unwrap_or(slice.len());
                    String::from_utf8_lossy(&slice[..endz]).to_string()
                };
                out.push(PerlValue::String(s));
            }
            'Z' => {
                let rest = data.get(pos..).unwrap_or(&[]);
                match t.repeat {
                    Repeat::Count(max) => {
                        let take = max.min(rest.len());
                        let chunk = &rest[..take];
                        pos += take;
                        let endz = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                        out.push(PerlValue::String(
                            String::from_utf8_lossy(&chunk[..endz]).to_string(),
                        ));
                    }
                    Repeat::One | Repeat::Star => {
                        let endz = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        let s = String::from_utf8_lossy(&rest[..endz]).to_string();
                        pos += endz + 1;
                        out.push(PerlValue::String(s));
                    }
                }
            }
            'H' => {
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
                    hex.push_str(&format!("{:02X}", b));
                }
                if let Repeat::Count(n) = t.repeat {
                    if hex.len() > n {
                        hex.truncate(n);
                    }
                }
                out.push(PerlValue::String(hex));
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
                    out.push(PerlValue::Integer(v as i64));
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
                    out.push(PerlValue::Integer(v as i64));
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
                    out.push(PerlValue::Integer(v as i64));
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
                    out.push(PerlValue::Integer(v as i64));
                }
            }
            'C' => match t.repeat {
                Repeat::Star => {
                    while pos < data.len() {
                        out.push(PerlValue::Integer(data[pos] as i64));
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
                        out.push(PerlValue::Integer(v as i64));
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
                    out.push(PerlValue::Integer(v as i64));
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
                    out.push(PerlValue::Integer(v));
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

    #[test]
    fn pack_unpack_n_v() {
        let be = perl_pack(
            &[
                PerlValue::String("N".into()),
                PerlValue::Integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let PerlValue::Bytes(ref b) = be else {
            panic!("expected Bytes");
        };
        assert_eq!(b.as_ref(), &[1, 2, 3, 4]);

        let le = perl_pack(
            &[
                PerlValue::String("V".into()),
                PerlValue::Integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let PerlValue::Bytes(ref b2) = le else {
            panic!("expected Bytes");
        };
        assert_eq!(b2.as_ref(), &[4, 3, 2, 1]);

        let u = perl_unpack(
            &[
                PerlValue::String("N".into()),
                PerlValue::Bytes(Arc::new(vec![0, 0, 0, 42])),
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
                PerlValue::String("C*".into()),
                PerlValue::Integer(65),
                PerlValue::Integer(66),
            ],
            0,
        )
        .unwrap();
        let PerlValue::Bytes(b) = p else {
            panic!("expected Bytes");
        };
        let u = perl_unpack(&[PerlValue::String("C*".into()), PerlValue::Bytes(b)], 0).unwrap();
        let PerlValue::Array(vals) = u else {
            panic!("expected array");
        };
        assert_eq!(vals.len(), 2);
        assert_eq!(vals[0].to_int(), 65);
        assert_eq!(vals[1].to_int(), 66);
    }
}
