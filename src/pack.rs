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
        Ok(bytes) => Ok(PerlValue::bytes(Arc::new(bytes))),
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
                Ok(PerlValue::array(vals))
            }
        }
        Err(msg) => Err(PerlError::runtime(format!("unpack: {}", msg), line)),
    }
}

fn value_to_bytes(v: &PerlValue) -> Result<Vec<u8>, String> {
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
                out.push(PerlValue::string(s));
            }
            'Z' => {
                let rest = data.get(pos..).unwrap_or(&[]);
                match t.repeat {
                    Repeat::Count(max) => {
                        let take = max.min(rest.len());
                        let chunk = &rest[..take];
                        pos += take;
                        let endz = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                        out.push(PerlValue::string(
                            String::from_utf8_lossy(&chunk[..endz]).to_string(),
                        ));
                    }
                    Repeat::One | Repeat::Star => {
                        let endz = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        let s = String::from_utf8_lossy(&rest[..endz]).to_string();
                        pos += endz + 1;
                        out.push(PerlValue::string(s));
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
                out.push(PerlValue::string(hex));
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
                    out.push(PerlValue::integer(v as i64));
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
                    out.push(PerlValue::integer(v as i64));
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
                    out.push(PerlValue::integer(v as i64));
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
                    out.push(PerlValue::integer(v as i64));
                }
            }
            'C' => match t.repeat {
                Repeat::Star => {
                    while pos < data.len() {
                        out.push(PerlValue::integer(data[pos] as i64));
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
                        out.push(PerlValue::integer(v as i64));
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
                    out.push(PerlValue::integer(v as i64));
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
                    out.push(PerlValue::integer(v));
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

    fn pack_bytes(template: &str, args: &[PerlValue]) -> Vec<u8> {
        let mut v = vec![PerlValue::string(template.into())];
        v.extend_from_slice(args);
        let p = perl_pack(&v, 0).expect("pack");
        p.as_bytes_arc().expect("bytes").as_ref().clone()
    }

    fn unpack_vals(template: &str, data: &[u8]) -> Vec<PerlValue> {
        let u = perl_unpack(
            &[
                PerlValue::string(template.into()),
                PerlValue::bytes(Arc::new(data.to_vec())),
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
        let e = perl_pack(&[PerlValue::string("C X".into()), PerlValue::integer(1)], 0)
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
            pack_bytes("a3", &[PerlValue::string("a".into())]),
            vec![b'a', 0, 0]
        );
        assert_eq!(
            pack_bytes("A3", &[PerlValue::string("a".into())]),
            vec![b'a', b' ', b' ']
        );
    }

    #[test]
    fn pack_z_one_appends_nul() {
        assert_eq!(
            pack_bytes("Z", &[PerlValue::string("ab".into())]),
            vec![b'a', b'b', 0]
        );
    }

    #[test]
    fn pack_z_count_truncates_or_pads() {
        let b = pack_bytes("Z4", &[PerlValue::string("abcdef".into())]);
        assert_eq!(b, vec![b'a', b'b', b'c', 0]);
        let b2 = pack_bytes("Z6", &[PerlValue::string("ab".into())]);
        assert_eq!(b2, vec![b'a', b'b', 0, 0, 0, 0]);
    }

    #[test]
    fn pack_h_one_nibble_pair() {
        assert_eq!(
            pack_bytes("H", &[PerlValue::string("ff".into())]),
            vec![255]
        );
    }

    #[test]
    fn pack_h_two_nibbles_from_template_count() {
        assert_eq!(
            pack_bytes("H4", &[PerlValue::string("dead".into())]),
            vec![0xde, 0xad]
        );
    }

    #[test]
    fn pack_h_rejects_odd_hex_length() {
        let e = perl_pack(
            &[PerlValue::string("H".into()), PerlValue::string("f".into())],
            0,
        )
        .expect_err("short hex");
        assert!(
            e.message.contains("nibble") || e.message.contains("even"),
            "{}",
            e.message
        );
    }

    #[test]
    fn pack_h_star_ignores_non_hex_separators() {
        assert_eq!(
            pack_bytes("H*", &[PerlValue::string("DE-AD".into())]),
            vec![0xde, 0xad]
        );
    }

    #[test]
    fn pack_x_inserts_zeros() {
        assert_eq!(pack_bytes("x3", &[]), vec![0, 0, 0]);
        assert_eq!(
            pack_bytes("C x2 C", &[PerlValue::integer(1), PerlValue::integer(2)]),
            vec![1, 0, 0, 2]
        );
    }

    #[test]
    fn pack_star_rejects_a() {
        let e = perl_pack(
            &[
                PerlValue::string("a*".into()),
                PerlValue::string("x".into()),
            ],
            0,
        )
        .expect_err("a*");
        assert!(e.message.contains("do not support"), "{}", e.message);
    }

    #[test]
    fn pack_not_enough_arguments() {
        let e = perl_pack(&[PerlValue::string("C C".into()), PerlValue::integer(1)], 0)
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
                PerlValue::string("N".into()),
                PerlValue::integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let b = be.as_bytes_arc().expect("expected Bytes");
        assert_eq!(b.as_ref(), &[1, 2, 3, 4]);

        let le = perl_pack(
            &[
                PerlValue::string("V".into()),
                PerlValue::integer(0x01020304),
            ],
            0,
        )
        .unwrap();
        let b2 = le.as_bytes_arc().expect("expected Bytes");
        assert_eq!(b2.as_ref(), &[4, 3, 2, 1]);

        let u = perl_unpack(
            &[
                PerlValue::string("N".into()),
                PerlValue::bytes(Arc::new(vec![0, 0, 0, 42])),
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
                PerlValue::string("C*".into()),
                PerlValue::integer(65),
                PerlValue::integer(66),
            ],
            0,
        )
        .unwrap();
        let b = p.as_bytes_arc().expect("expected Bytes");
        let u = perl_unpack(
            &[
                PerlValue::string("C*".into()),
                PerlValue::bytes(Arc::clone(&b)),
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
                PerlValue::string("N".into()),
                PerlValue::bytes(Arc::new(vec![1, 2])),
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
        let b = pack_bytes("N*", &[PerlValue::integer(1), PerlValue::integer(2)]);
        assert_eq!(b.len(), 8);
        let v = unpack_vals("N*", &b);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].to_int(), 1);
        assert_eq!(v[1].to_int(), 2);
    }

    #[test]
    fn pack_n2_two_big_endian_words() {
        let b = pack_bytes("N2", &[PerlValue::integer(1), PerlValue::integer(2)]);
        assert_eq!(b, vec![0, 0, 0, 1, 0, 0, 0, 2]);
    }

    #[test]
    fn pack_v_and_n_endian_differ() {
        let le = pack_bytes("v", &[PerlValue::integer(0x0102)]);
        let be = pack_bytes("n", &[PerlValue::integer(0x0102)]);
        assert_eq!(le, vec![0x02, 0x01]);
        assert_eq!(be, vec![0x01, 0x02]);
    }

    #[test]
    fn pack_q_signed_roundtrip() {
        let n = -42i64;
        let b = pack_bytes("q", &[PerlValue::integer(n)]);
        assert_eq!(b.len(), 8);
        let v = unpack_vals("q", &b);
        assert_eq!(v[0].to_int(), n);
    }

    #[test]
    fn unpack_from_string_scalar_accepted() {
        let u = perl_unpack(
            &[
                PerlValue::string("C".into()),
                PerlValue::string("\x07".into()),
            ],
            0,
        )
        .expect("unpack from str");
        assert_eq!(u.to_int(), 7);
    }

    #[test]
    fn unpack_rejects_non_string_data() {
        let e = perl_unpack(&[PerlValue::string("C".into()), PerlValue::integer(99)], 0)
            .expect_err("type");
        assert!(e.message.contains("string or packed"), "{}", e.message);
    }

    #[test]
    fn whitespace_in_template_is_skipped() {
        assert_eq!(
            pack_bytes("C  C", &[PerlValue::integer(1), PerlValue::integer(2)]),
            vec![1, 2]
        );
    }
}
