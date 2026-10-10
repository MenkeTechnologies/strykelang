//! Perl `pack` / `unpack` for binary I/O, after `pp_pack.c` and `perlfunc`.
//!
//! Types: `a A Z` (strings), `b B` (bit strings), `h H` (hex strings), `c C W U`
//! (characters), `s S l L q Q j J i I n N v V` (integers; `!` selects the native
//! `short`/`long`/signed-network forms, `<` / `>` force the byte order), `f d F`
//! (floats), `w` (BER integer), `u` (uuencode), `x X @ .` (positioning), `( ... )`
//! groups, `LEN/ITEM` length-prefixed items, and `%N` checksums in `unpack`. A
//! count is a number, `*`, `[N]` or `[template]` (the byte size of a template).
//!
//! `U` is a Unicode code point, stored UTF-8 encoded. As in Perl, a template that
//! starts with `U` packs a character string (`pack "U", 0x263A` is one character);
//! otherwise the result is the byte string holding the UTF-8 encoding
//! (`pack "C0U", 0x263A` is three bytes). `unpack` decodes one UTF-8 sequence per
//! `U`.

use std::sync::Arc;

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Count {
    /// No count written: one item (`a` is one byte, `H` one nibble).
    One,
    N(usize),
    Star,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Endian {
    Native,
    Little,
    Big,
}

impl Endian {
    fn is_little(self) -> bool {
        match self {
            Endian::Little => true,
            Endian::Big => false,
            Endian::Native => cfg!(target_endian = "little"),
        }
    }
}

#[derive(Clone, Debug)]
enum Node {
    Item {
        op: char,
        bang: bool,
        endian: Endian,
        count: Count,
    },
    Group {
        body: Vec<Node>,
        endian: Endian,
        count: Count,
    },
    /// `LEN/SEQ`: the length item counts the sequence item.
    Slash { len: Box<Node>, seq: Box<Node> },
    /// `%N` — `unpack` replaces the next item's values with their sum modulo `2**N`.
    Checksum(u32),
}

const OPS: &str = "aAZbBhHcCWUsSlLqQjJiInNvVfdFwuxX@.";

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    fn skip_blank(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_whitespace() {
                self.chars.next();
            } else if c == '#' {
                while let Some(c) = self.chars.next() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn number(&mut self) -> Option<usize> {
        let mut n: Option<usize> = None;
        while let Some(&d) = self.chars.peek() {
            let Some(v) = d.to_digit(10) else { break };
            n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(v as usize));
            self.chars.next();
        }
        n
    }

    fn modifiers(&mut self) -> Result<(bool, Endian), String> {
        let (mut bang, mut endian) = (false, Endian::Native);
        while let Some(&c) = self.chars.peek() {
            match c {
                '!' => bang = true,
                '<' | '>' => {
                    let want = if c == '<' {
                        Endian::Little
                    } else {
                        Endian::Big
                    };
                    if endian != Endian::Native && endian != want {
                        return Err("Can't use both '<' and '>' after type".into());
                    }
                    endian = want;
                }
                _ => break,
            }
            self.chars.next();
        }
        Ok((bang, endian))
    }

    fn count(&mut self) -> Result<Count, String> {
        match self.chars.peek() {
            Some('*') => {
                self.chars.next();
                Ok(Count::Star)
            }
            Some('[') => {
                self.chars.next();
                if self.chars.peek().is_some_and(char::is_ascii_digit) {
                    let n = self.number().unwrap_or(0);
                    if self.chars.next() != Some(']') {
                        return Err("No group ending character ']' found in template".into());
                    }
                    return Ok(Count::N(n));
                }
                let mut depth = 1;
                let mut inner = String::new();
                for c in self.chars.by_ref() {
                    match c {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    inner.push(c);
                }
                if depth != 0 {
                    return Err("No group ending character ']' found in template".into());
                }
                Ok(Count::N(template_size(&parse(&inner)?)?))
            }
            Some(d) if d.is_ascii_digit() => Ok(Count::N(self.number().unwrap_or(0))),
            _ => Ok(Count::One),
        }
    }

    /// One item or group, without a trailing `/`.
    fn node(&mut self) -> Result<Option<Node>, String> {
        self.skip_blank();
        let Some(c) = self.chars.next() else {
            return Ok(None);
        };
        match c {
            '(' => {
                let body = self.sequence(true)?;
                let (_, endian) = self.modifiers()?;
                let count = self.count()?;
                Ok(Some(Node::Group {
                    body,
                    endian,
                    count,
                }))
            }
            '%' => Ok(Some(Node::Checksum(self.number().unwrap_or(16) as u32))),
            c if OPS.contains(c) => {
                let (bang, endian) = self.modifiers()?;
                let count = self.count()?;
                Ok(Some(Node::Item {
                    op: c,
                    bang,
                    endian,
                    count,
                }))
            }
            ')' => Err("Mismatched brackets in template".into()),
            c => Err(format!("unsupported pack type '{}'", c)),
        }
    }

    fn sequence(&mut self, in_group: bool) -> Result<Vec<Node>, String> {
        let mut out = Vec::new();
        loop {
            self.skip_blank();
            match self.chars.peek() {
                None if in_group => {
                    return Err("No group ending character ')' found in template".into())
                }
                None => return Ok(out),
                Some(')') if in_group => {
                    self.chars.next();
                    return Ok(out);
                }
                _ => {}
            }
            let Some(node) = self.node()? else {
                return Ok(out);
            };
            self.skip_blank();
            if self.chars.peek() == Some(&'/') {
                self.chars.next();
                let Some(seq) = self.node()? else {
                    return Err("Code missing after '/'".into());
                };
                out.push(Node::Slash {
                    len: Box::new(node),
                    seq: Box::new(seq),
                });
            } else {
                out.push(node);
            }
        }
    }
}

fn parse(template: &str) -> Result<Vec<Node>, String> {
    Parser {
        chars: template.chars().peekable(),
    }
    .sequence(false)
}

/// Byte size of a fixed-width template (`x[L]`, `@[N2]`).
fn template_size(nodes: &[Node]) -> Result<usize, String> {
    let mut total = 0usize;
    for node in nodes {
        let (size, count) = match node {
            Node::Item {
                op, bang, count, ..
            } => (
                fixed_width(*op, *bang).ok_or("Within []-length, '*' not allowed")?,
                *count,
            ),
            Node::Group { body, count, .. } => (template_size(body)?, *count),
            Node::Slash { .. } => return Err("'/' not allowed within []-length".into()),
            Node::Checksum(_) => continue,
        };
        let n = match count {
            Count::One => 1,
            Count::N(n) => n,
            Count::Star => return Err("Within []-length, '*' not allowed".into()),
        };
        total += size * n;
    }
    Ok(total)
}

/// Width in bytes of one item of a numeric / positioning type.
fn fixed_width(op: char, bang: bool) -> Option<usize> {
    Some(match op {
        'c' | 'C' | 'W' | 'a' | 'A' | 'Z' | 'x' => 1,
        's' | 'S' | 'n' | 'v' => 2,
        'l' | 'L' if bang => 8,
        'l' | 'L' | 'i' | 'I' | 'N' | 'V' | 'f' => 4,
        'q' | 'Q' | 'j' | 'J' | 'd' | 'F' => 8,
        _ => return None,
    })
}

/// `(bytes, signed, forced_endian)` of an integer type, `None` for other types.
fn int_layout(op: char, bang: bool) -> Option<(usize, bool, Option<Endian>)> {
    Some(match op {
        'c' => (1, true, None),
        'C' | 'W' => (1, false, None),
        's' => (2, true, None),
        'S' => (2, false, None),
        'l' => (if bang { 8 } else { 4 }, true, None),
        'L' => (if bang { 8 } else { 4 }, false, None),
        'i' => (4, true, None),
        'I' => (4, false, None),
        'q' | 'j' => (8, true, None),
        'Q' | 'J' => (8, false, None),
        'n' => (2, bang, Some(Endian::Big)),
        'N' => (4, bang, Some(Endian::Big)),
        'v' => (2, bang, Some(Endian::Little)),
        'V' => (4, bang, Some(Endian::Little)),
        _ => return None,
    })
}

fn take_arg<'a>(args: &mut &'a [StrykeValue]) -> Result<&'a StrykeValue, String> {
    if args.is_empty() {
        return Err("not enough arguments".into());
    }
    let v = &args[0];
    *args = &args[1..];
    Ok(v)
}

/// The integer a `pack` numeric type stores: negative values wrap, UVs keep all 64 bits.
fn pack_int_arg(v: &StrykeValue) -> u64 {
    crate::value::perl_uv_arg(v)
}

fn push_int(out: &mut Vec<u8>, v: u64, size: usize, little: bool) {
    let le = v.to_le_bytes();
    if little {
        out.extend_from_slice(&le[..size]);
    } else {
        out.extend(le[..size].iter().rev());
    }
}

fn read_int(bytes: &[u8], little: bool) -> u64 {
    let mut v = 0u64;
    if little {
        for (i, b) in bytes.iter().enumerate() {
            v |= u64::from(*b) << (8 * i);
        }
    } else {
        for b in bytes {
            v = (v << 8) | u64::from(*b);
        }
    }
    v
}

fn int_value(raw: u64, size: usize, signed: bool) -> StrykeValue {
    if signed {
        let shift = 64 - 8 * size as u32;
        StrykeValue::integer(((raw << shift) as i64) >> shift)
    } else {
        StrykeValue::unsigned(raw)
    }
}

const UU_ALPHABET_OFFSET: u8 = 32;

fn uu_char(six_bits: u8) -> u8 {
    if six_bits == 0 {
        b'`'
    } else {
        six_bits + UU_ALPHABET_OFFSET
    }
}

/// Mutable pack state: the output bytes plus whether non-`U` bytes are characters.
struct Packer {
    out: Vec<u8>,
    /// Template began with `U`: the result is a character string, so a byte
    /// packed by any other type is the character with that code.
    chars: bool,
}

impl Packer {
    fn bytes(&mut self, b: &[u8]) {
        if self.chars {
            let mut enc = [0u8; 4];
            for &byte in b {
                self.out
                    .extend_from_slice(char::from(byte).encode_utf8(&mut enc).as_bytes());
            }
        } else {
            self.out.extend_from_slice(b);
        }
    }

    fn pack_nodes(
        &mut self,
        nodes: &[Node],
        args: &mut &[StrykeValue],
        group_start: usize,
    ) -> Result<(), String> {
        for node in nodes {
            match node {
                Node::Item {
                    op,
                    bang,
                    endian,
                    count,
                } => self.pack_item(*op, *bang, *endian, *count, args, group_start)?,
                Node::Group { body, count, .. } => {
                    let reps = match count {
                        Count::One => 1,
                        Count::N(n) => *n,
                        Count::Star => usize::MAX,
                    };
                    let mut done = 0;
                    while done < reps && (*count != Count::Star || !args.is_empty()) {
                        let start = self.out.len();
                        self.pack_nodes(body, args, start)?;
                        done += 1;
                    }
                }
                Node::Slash { len, seq } => self.pack_slash(len, seq, args, group_start)?,
                Node::Checksum(_) => return Err("'%' may not be used in pack".into()),
            }
        }
        Ok(())
    }

    fn pack_slash(
        &mut self,
        len: &Node,
        seq: &Node,
        args: &mut &[StrykeValue],
        group_start: usize,
    ) -> Result<(), String> {
        let Node::Item { op: len_op, .. } = len else {
            return Err("'/' must follow a numeric type in pack".into());
        };
        let (n, seq_count, data): (usize, Count, Option<String>) = match seq {
            Node::Item {
                op: 'a' | 'A' | 'Z',
                count,
                ..
            } => {
                let s = take_arg(args)?.to_string();
                let mut n = string_bytes(&s).len();
                if let Count::N(c) = count {
                    n = n.min(*c);
                }
                if matches!(seq, Node::Item { op: 'Z', .. }) && !matches!(count, Count::N(_)) {
                    n += 1;
                }
                (n, Count::N(n), Some(s))
            }
            Node::Item { count, .. } | Node::Group { count, .. } => {
                let avail = args.len();
                let n = match count {
                    Count::N(c) => avail.min(*c),
                    _ => avail,
                };
                (n, Count::N(n), None)
            }
            _ => return Err("'/' must be followed by a string type or group".into()),
        };
        // Length item: numeric types take the count, string types its decimal text.
        let len_arg = if matches!(len_op, 'a' | 'A' | 'Z') {
            StrykeValue::string(n.to_string())
        } else {
            StrykeValue::integer(n as i64)
        };
        let mut len_args: &[StrykeValue] = std::slice::from_ref(&len_arg);
        self.pack_nodes(std::slice::from_ref(len), &mut len_args, group_start)?;
        match (seq, data) {
            (
                Node::Item {
                    op, bang, endian, ..
                },
                Some(s),
            ) => {
                let one = [StrykeValue::string(s)];
                let mut rest: &[StrykeValue] = &one;
                self.pack_item(*op, *bang, *endian, seq_count, &mut rest, group_start)
            }
            (
                Node::Item {
                    op, bang, endian, ..
                },
                None,
            ) => self.pack_item(*op, *bang, *endian, seq_count, args, group_start),
            (Node::Group { body, .. }, _) => {
                for _ in 0..n {
                    let start = self.out.len();
                    self.pack_nodes(body, args, start)?;
                }
                Ok(())
            }
            _ => Err("'/' must be followed by a string type or group".into()),
        }
    }

    fn pack_item(
        &mut self,
        op: char,
        bang: bool,
        endian: Endian,
        count: Count,
        args: &mut &[StrykeValue],
        group_start: usize,
    ) -> Result<(), String> {
        match op {
            'a' | 'A' | 'Z' => {
                let owned = string_bytes(&take_arg(args)?.to_string());
                let bytes = owned.as_slice();
                // A bare `Z` takes the whole string plus its terminating NUL (stryke's pinned
                // behavior; perl treats it as `Z1`).
                let count = if op == 'Z' && count == Count::One {
                    Count::Star
                } else {
                    count
                };
                let mut field: Vec<u8> = match count {
                    Count::Star => {
                        let mut b = bytes.to_vec();
                        if op == 'Z' {
                            b.push(0);
                        }
                        b
                    }
                    Count::One | Count::N(_) => {
                        let n = if let Count::N(n) = count { n } else { 1 };
                        let mut b = vec![if op == 'A' { b' ' } else { 0 }; n];
                        let copy = n.min(bytes.len());
                        b[..copy].copy_from_slice(&bytes[..copy]);
                        if op == 'Z' && n > 0 && bytes.len() >= n {
                            b[n - 1] = 0;
                        }
                        b
                    }
                };
                self.bytes(&std::mem::take(&mut field));
            }
            'b' | 'B' => {
                let s = take_arg(args)?.to_string();
                let bits: Vec<u8> = s.bytes().map(|c| c & 1).collect();
                let n = match count {
                    Count::Star => bits.len(),
                    Count::One => 1,
                    Count::N(n) => n,
                };
                let mut field = vec![0u8; n.div_ceil(8)];
                for i in 0..n {
                    if bits.get(i).copied().unwrap_or(0) == 1 {
                        let shift = if op == 'b' { i % 8 } else { 7 - i % 8 };
                        field[i / 8] |= 1 << shift;
                    }
                }
                self.bytes(&field);
            }
            'h' | 'H' => {
                let s = take_arg(args)?.to_string();
                let digits: Vec<u8> = s
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .map(|c| c.to_digit(16).unwrap_or(0) as u8)
                    .collect();
                let n = match count {
                    Count::Star => digits.len(),
                    Count::One => {
                        if digits.is_empty() {
                            return Err("hex string too short".into());
                        }
                        1
                    }
                    Count::N(n) => {
                        if digits.len() < n {
                            return Err("hex string too short".into());
                        }
                        n
                    }
                };
                let mut field = vec![0u8; n.div_ceil(2)];
                for (i, &d) in digits.iter().take(n).enumerate() {
                    let high = (op == 'H') == (i % 2 == 0);
                    field[i / 2] |= if high { d << 4 } else { d };
                }
                self.bytes(&field);
            }
            'U' | 'W' => {
                let n = match count {
                    Count::Star => args.len(),
                    Count::One => 1,
                    Count::N(n) => n,
                };
                for _ in 0..n {
                    let cp = take_arg(args)?.to_int();
                    let ch = u32::try_from(cp)
                        .ok()
                        .and_then(char::from_u32)
                        .unwrap_or(char::REPLACEMENT_CHARACTER);
                    if op == 'W' && !self.chars && u32::from(ch) < 256 {
                        self.out.push(u32::from(ch) as u8);
                    } else {
                        let mut enc = [0u8; 4];
                        self.out
                            .extend_from_slice(ch.encode_utf8(&mut enc).as_bytes());
                    }
                }
            }
            'w' => {
                let n = match count {
                    Count::Star => args.len(),
                    Count::One => 1,
                    Count::N(n) => n,
                };
                for _ in 0..n {
                    // `pack 'w'` on a negative integer is an error, never a wrapped value.
                    let signed = take_arg(args)?.to_int();
                    if signed < 0 {
                        return Err("Cannot compress negative numbers in pack".into());
                    }
                    let mut v = signed as u64;
                    let mut ber = vec![(v & 0x7f) as u8];
                    v >>= 7;
                    while v > 0 {
                        ber.push((v & 0x7f) as u8 | 0x80);
                        v >>= 7;
                    }
                    ber.reverse();
                    self.bytes(&ber);
                }
            }
            'f' | 'd' | 'F' => {
                let n = match count {
                    Count::Star => args.len(),
                    Count::One => 1,
                    Count::N(n) => n,
                };
                for _ in 0..n {
                    let v = take_arg(args)?.to_number();
                    let little = endian.is_little();
                    let mut bytes = if op == 'f' {
                        (v as f32).to_le_bytes().to_vec()
                    } else {
                        v.to_le_bytes().to_vec()
                    };
                    if !little {
                        bytes.reverse();
                    }
                    self.bytes(&bytes);
                }
            }
            'u' => {
                let s = take_arg(args)?.to_string();
                let line_len = match count {
                    Count::N(n) if n >= 3 => n / 3 * 3,
                    _ => 45,
                };
                for line in s.as_bytes().chunks(line_len) {
                    let mut enc = vec![uu_char(line.len() as u8)];
                    for tri in line.chunks(3) {
                        let b = [
                            tri[0],
                            tri.get(1).copied().unwrap_or(0),
                            tri.get(2).copied().unwrap_or(0),
                        ];
                        enc.push(uu_char(b[0] >> 2));
                        enc.push(uu_char(((b[0] & 3) << 4) | (b[1] >> 4)));
                        enc.push(uu_char(((b[1] & 15) << 2) | (b[2] >> 6)));
                        enc.push(uu_char(b[2] & 63));
                    }
                    enc.push(b'\n');
                    self.bytes(&enc);
                }
            }
            'x' => {
                if bang {
                    let align = match count {
                        Count::N(n) if n > 0 => n,
                        _ => 1,
                    };
                    let pad = (align - self.out.len() % align) % align;
                    self.bytes(&vec![0u8; pad]);
                } else {
                    let n = match count {
                        Count::One => 1,
                        Count::N(n) => n,
                        Count::Star => return Err("'x' does not support '*'".into()),
                    };
                    self.bytes(&vec![0u8; n]);
                }
            }
            'X' => {
                let n = if bang {
                    let align = match count {
                        Count::N(n) if n > 0 => n,
                        _ => 1,
                    };
                    self.out.len() % align
                } else {
                    match count {
                        Count::One => 1,
                        Count::N(n) => n,
                        Count::Star => 0,
                    }
                };
                if n > self.out.len() {
                    return Err("'X' outside of string in pack".into());
                }
                let keep = self.out.len() - n;
                self.out.truncate(keep);
            }
            '@' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => 0,
                };
                let target = group_start + n;
                self.out.resize(target, 0);
            }
            '.' => {
                let n = take_arg(args)?.to_int();
                let target = usize::try_from(n).map_err(|_| "'.' outside of string in pack")?;
                self.out.resize(target, 0);
            }
            _ => {
                let Some((size, _signed, forced)) = int_layout(op, bang) else {
                    return Err(format!("unsupported pack type '{}'", op));
                };
                let little = forced.unwrap_or(endian).is_little();
                let n = match count {
                    Count::Star => args.len(),
                    Count::One => 1,
                    Count::N(n) => n,
                };
                if op == 'C' && count == Count::N(0) {
                    return Ok(());
                }
                for _ in 0..n {
                    let v = pack_int_arg(take_arg(args)?);
                    let mut field = Vec::with_capacity(size);
                    push_int(&mut field, v, size, little);
                    self.bytes(&field);
                }
            }
        }
        Ok(())
    }
}

/// `pack TEMPLATE, LIST`
pub fn perl_pack(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    if args.is_empty() {
        return Err(StrykeError::runtime("pack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let mut rest = &args[1..];
    let result = parse(&template).and_then(|nodes| {
        let mut packer = Packer {
            out: Vec::new(),
            // A leading `U` selects character mode: the UTF-8 buffer is a text string.
            chars: template.trim_start().starts_with('U'),
        };
        packer.pack_nodes(&nodes, &mut rest, 0)?;
        Ok(packer)
    });
    match result {
        Ok(p) if p.chars => Ok(StrykeValue::string(
            String::from_utf8_lossy(&p.out).into_owned(),
        )),
        Ok(p) => Ok(StrykeValue::bytes(Arc::new(p.out))),
        Err(msg) => Err(StrykeError::runtime(format!("pack: {}", msg), line)),
    }
}

/// `unpack TEMPLATE, SCALAR`
pub fn perl_unpack(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    if args.len() < 2 {
        return Err(StrykeError::runtime("unpack: not enough arguments", line));
    }
    let template = args[0].to_string();
    let data = value_to_bytes(&args[1]).map_err(|m| StrykeError::runtime(m, line))?;
    let result = parse(&template).and_then(|nodes| {
        let mut u = Unpacker {
            data: &data,
            pos: 0,
        };
        let mut out = Vec::new();
        u.unpack_nodes(&nodes, &mut out, 0)?;
        Ok(out)
    });
    match result {
        Ok(vals) if vals.len() == 1 => Ok(vals.into_iter().next().unwrap_or(StrykeValue::UNDEF)),
        Ok(vals) => Ok(StrykeValue::array(vals)),
        Err(msg) => Err(StrykeError::runtime(format!("unpack: {}", msg), line)),
    }
}

fn value_to_bytes(v: &StrykeValue) -> Result<Vec<u8>, String> {
    if let Some(b) = v.as_bytes_arc() {
        return Ok((*b).clone());
    }
    if let Some(s) = v.as_str() {
        return Ok(string_bytes(&s));
    }
    Err("unpack: data must be string or packed bytes".into())
}

/// The bytes of a string operand: one byte per character when every character fits in a byte
/// (`"\xff"` is the byte `0xFF`, as perl sees a non-wide string), the UTF-8 encoding otherwise.
fn string_bytes(s: &str) -> Vec<u8> {
    if s.chars().all(|c| u32::from(c) < 256) {
        s.chars().map(|c| u32::from(c) as u8).collect()
    } else {
        s.as_bytes().to_vec()
    }
}

/// Text when the bytes are valid UTF-8, otherwise the raw byte string.
fn bytes_value(b: &[u8]) -> StrykeValue {
    match std::str::from_utf8(b) {
        Ok(s) => StrykeValue::string(s.to_string()),
        Err(_) => StrykeValue::bytes(Arc::new(b.to_vec())),
    }
}

struct Unpacker<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Unpacker<'_> {
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&[u8], String> {
        if self.pos + n > self.data.len() {
            return Err(format!("data too short for {}", what));
        }
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    fn unpack_nodes(
        &mut self,
        nodes: &[Node],
        out: &mut Vec<StrykeValue>,
        group_start: usize,
    ) -> Result<(), String> {
        let mut i = 0;
        while i < nodes.len() {
            if let Node::Checksum(bits) = &nodes[i] {
                i += 1;
                let Some(target) = nodes.get(i) else {
                    return Err("'%' must be followed by a type".into());
                };
                let mut inner = Vec::new();
                self.unpack_nodes(std::slice::from_ref(target), &mut inner, group_start)?;
                out.push(checksum(&inner, *bits, target));
                i += 1;
                continue;
            }
            self.unpack_node(&nodes[i], out, group_start)?;
            i += 1;
        }
        Ok(())
    }

    fn unpack_node(
        &mut self,
        node: &Node,
        out: &mut Vec<StrykeValue>,
        group_start: usize,
    ) -> Result<(), String> {
        match node {
            Node::Item {
                op,
                bang,
                endian,
                count,
            } => self.unpack_item(*op, *bang, *endian, *count, out, group_start),
            Node::Group { body, count, .. } => {
                let reps = match count {
                    Count::One => 1,
                    Count::N(n) => *n,
                    Count::Star => usize::MAX,
                };
                let mut done = 0;
                while done < reps && (*count != Count::Star || self.pos < self.data.len()) {
                    let before = self.pos;
                    let start = self.pos;
                    self.unpack_nodes(body, out, start)?;
                    done += 1;
                    if *count == Count::Star && self.pos == before {
                        break;
                    }
                }
                Ok(())
            }
            Node::Slash { len, seq } => {
                let mut lens = Vec::new();
                self.unpack_node(len, &mut lens, group_start)?;
                let n = lens.first().map_or(0, |v| v.to_int().max(0) as usize);
                match &**seq {
                    Node::Item {
                        op, bang, endian, ..
                    } => self.unpack_item(*op, *bang, *endian, Count::N(n), out, group_start),
                    Node::Group { body, endian, .. } => {
                        let g = Node::Group {
                            body: body.clone(),
                            endian: *endian,
                            count: Count::N(n),
                        };
                        self.unpack_node(&g, out, group_start)
                    }
                    _ => Err("'/' must be followed by a string type or group".into()),
                }
            }
            Node::Checksum(_) => Ok(()),
        }
    }

    fn unpack_item(
        &mut self,
        op: char,
        bang: bool,
        endian: Endian,
        count: Count,
        out: &mut Vec<StrykeValue>,
        group_start: usize,
    ) -> Result<(), String> {
        match op {
            'a' | 'A' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => self.remaining(),
                };
                let n = n.min(self.remaining());
                let mut slice = self.take(n, "string")?;
                if op == 'A' {
                    while let Some((&last, head)) = slice.split_last() {
                        if last == 0 || last.is_ascii_whitespace() {
                            slice = head;
                        } else {
                            break;
                        }
                    }
                }
                out.push(bytes_value(slice));
            }
            'Z' => {
                let rest = &self.data[self.pos.min(self.data.len())..];
                match count {
                    Count::N(max) => {
                        let take = max.min(rest.len());
                        let chunk = &rest[..take];
                        self.pos += take;
                        let end = chunk.iter().position(|&b| b == 0).unwrap_or(chunk.len());
                        out.push(bytes_value(&chunk[..end]));
                    }
                    Count::One | Count::Star => {
                        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                        out.push(bytes_value(&rest[..end]));
                        self.pos += end + 1;
                    }
                }
            }
            'b' | 'B' => {
                let bits = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => self.remaining() * 8,
                };
                let bytes = self.take(bits.div_ceil(8), "bit string")?;
                let s: String = (0..bits)
                    .map(|i| {
                        let shift = if op == 'b' { i % 8 } else { 7 - i % 8 };
                        if bytes[i / 8] >> shift & 1 == 1 {
                            '1'
                        } else {
                            '0'
                        }
                    })
                    .collect();
                out.push(StrykeValue::string(s));
            }
            'h' | 'H' => {
                // `H` is lowercase hex, high nibble first; `h` is low nibble first.
                let nibbles = match count {
                    Count::Star => self.remaining() * 2,
                    Count::One => 1,
                    Count::N(n) => n,
                };
                let bytes = self.take(nibbles.div_ceil(2), "hex string")?;
                let s: String = (0..nibbles)
                    .map(|i| {
                        let b = bytes[i / 2];
                        let high = (op == 'H') == (i % 2 == 0);
                        let d = if high { b >> 4 } else { b & 15 };
                        char::from_digit(u32::from(d), 16).unwrap_or('0')
                    })
                    .collect();
                out.push(StrykeValue::string(s));
            }
            'U' => {
                let n = match count {
                    Count::Star => usize::MAX,
                    Count::One => 1,
                    Count::N(n) => n,
                };
                let mut decoded = 0usize;
                while decoded < n && self.pos < self.data.len() {
                    let rest = &self.data[self.pos..(self.pos + 4).min(self.data.len())];
                    let (cp, len) = match std::str::from_utf8(rest) {
                        Ok(s) => s.chars().next().map(|c| (c as i64, c.len_utf8())),
                        Err(e) if e.valid_up_to() > 0 => {
                            std::str::from_utf8(&rest[..e.valid_up_to()])
                                .ok()
                                .and_then(|s| s.chars().next())
                                .map(|c| (c as i64, c.len_utf8()))
                        }
                        Err(_) => None,
                    }
                    // Not UTF-8 at this position: the byte is its own code point.
                    .unwrap_or((i64::from(self.data[self.pos]), 1));
                    self.pos += len;
                    out.push(StrykeValue::integer(cp));
                    decoded += 1;
                }
            }
            'w' => {
                let n = match count {
                    Count::Star => usize::MAX,
                    Count::One => 1,
                    Count::N(n) => n,
                };
                let mut decoded = 0usize;
                while decoded < n && self.pos < self.data.len() {
                    let mut val: u64 = 0;
                    loop {
                        let byte = *self.take(1, "w")?.first().unwrap_or(&0);
                        val = (val << 7) | u64::from(byte & 0x7f);
                        if byte & 0x80 == 0 {
                            break;
                        }
                    }
                    out.push(StrykeValue::unsigned(val));
                    decoded += 1;
                }
            }
            'f' | 'd' | 'F' => {
                let size = if op == 'f' { 4 } else { 8 };
                let n = match count {
                    Count::Star => self.remaining() / size,
                    Count::One => 1,
                    Count::N(n) => n,
                };
                for _ in 0..n {
                    let mut b = self.take(size, &op.to_string())?.to_vec();
                    if !endian.is_little() {
                        b.reverse();
                    }
                    let v = if op == 'f' {
                        f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    } else {
                        f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
                    };
                    out.push(StrykeValue::float(v));
                }
            }
            'u' => {
                let mut decoded = Vec::new();
                while self.pos < self.data.len() && self.data[self.pos] != b'\n' {
                    let len =
                        usize::from(self.data[self.pos].wrapping_sub(UU_ALPHABET_OFFSET) & 63);
                    self.pos += 1;
                    let mut line = Vec::new();
                    while self.pos < self.data.len() && self.data[self.pos] != b'\n' {
                        line.push(self.data[self.pos].wrapping_sub(UU_ALPHABET_OFFSET) & 63);
                        self.pos += 1;
                    }
                    self.pos += 1;
                    let mut bytes = Vec::new();
                    for quad in line.chunks(4) {
                        let q = |i: usize| quad.get(i).copied().unwrap_or(0);
                        bytes.push(q(0) << 2 | q(1) >> 4);
                        bytes.push(q(1) << 4 | q(2) >> 2);
                        bytes.push(q(2) << 6 | q(3));
                    }
                    bytes.truncate(len);
                    decoded.extend(bytes);
                }
                out.push(bytes_value(&decoded));
            }
            'x' => {
                if bang {
                    let align = match count {
                        Count::N(n) if n > 0 => n,
                        _ => 1,
                    };
                    let pad = (align - self.pos % align) % align;
                    self.pos += pad;
                    if self.pos > self.data.len() {
                        return Err("'x' outside of string in unpack".into());
                    }
                } else {
                    let n = match count {
                        Count::One => 1,
                        Count::N(n) => n,
                        Count::Star => return Err("'x' cannot use '*' in unpack".into()),
                    };
                    self.pos = self.pos.saturating_add(n);
                    if self.pos > self.data.len() {
                        return Err("x past end".into());
                    }
                }
            }
            'X' => {
                let n = if bang {
                    let align = match count {
                        Count::N(n) if n > 0 => n,
                        _ => 1,
                    };
                    self.pos % align
                } else {
                    match count {
                        Count::One => 1,
                        Count::N(n) => n,
                        Count::Star => 0,
                    }
                };
                if n > self.pos {
                    return Err("'X' outside of string in unpack".into());
                }
                self.pos -= n;
            }
            '@' => {
                let n = match count {
                    Count::One => 1,
                    Count::N(n) => n,
                    Count::Star => 0,
                };
                let target = group_start + n;
                if target > self.data.len() {
                    return Err("'@' outside of string in unpack".into());
                }
                self.pos = target;
            }
            '.' => out.push(StrykeValue::integer(self.pos as i64)),
            'c' | 'C' | 'W' if matches!(count, Count::N(0)) => {}
            _ => {
                let Some((size, signed, forced)) = int_layout(op, bang) else {
                    return Err(format!("unsupported pack type '{}'", op));
                };
                let little = forced.unwrap_or(endian).is_little();
                let n = match count {
                    Count::Star => self.remaining() / size,
                    Count::One => 1,
                    Count::N(n) => n,
                };
                for _ in 0..n {
                    let raw = read_int(self.take(size, &op.to_string())?, little);
                    out.push(int_value(raw, size, signed));
                }
            }
        }
        Ok(())
    }
}

/// `%N` checksum of the values an item produced: bit strings count their set bits,
/// numbers add; the sum is reduced modulo `2**N`.
fn checksum(values: &[StrykeValue], bits: u32, node: &Node) -> StrykeValue {
    let is_bits = matches!(node, Node::Item { op: 'b' | 'B', .. });
    let mut sum: u128 = 0;
    let mut float_sum = 0.0f64;
    let mut use_float = false;
    for v in values {
        if is_bits {
            sum += v.to_string().bytes().filter(|&b| b == b'1').count() as u128;
        } else if let Some(f) = v.as_float() {
            use_float = true;
            float_sum += f;
        } else {
            sum = sum.wrapping_add(crate::value::perl_uv_arg(v) as u128);
        }
    }
    if use_float {
        return StrykeValue::float(float_sum + sum as f64);
    }
    if bits < 64 {
        StrykeValue::integer((sum & ((1u128 << bits) - 1)) as i64)
    } else {
        StrykeValue::unsigned(sum as u64)
    }
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
            &[StrykeValue::string("C P".into()), StrykeValue::integer(1)],
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
    fn pack_a_star_takes_the_whole_string() {
        assert_eq!(
            pack_bytes("a*", &[StrykeValue::string("xyz".into())]),
            b"xyz"
        );
        assert_eq!(
            pack_bytes("A*", &[StrykeValue::string("xyz".into())]),
            b"xyz"
        );
        assert_eq!(
            pack_bytes("Z*", &[StrykeValue::string("xyz".into())]),
            b"xyz\0"
        );
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

    #[test]
    fn pack_leading_u_is_a_character_string() {
        let p = perl_pack(
            &[
                StrykeValue::string("U*".into()),
                StrykeValue::integer(0x48),
                StrykeValue::integer(0x263A),
            ],
            0,
        )
        .expect("pack");
        assert!(
            p.as_bytes_arc().is_none(),
            "leading U selects character mode"
        );
        assert_eq!(p.to_string(), "H\u{263A}");
    }

    #[test]
    fn pack_c0u_is_the_utf8_byte_string() {
        let b = pack_bytes("C0U", &[StrykeValue::integer(0x263A)]);
        assert_eq!(b, [0xE2, 0x98, 0xBA]);
    }

    #[test]
    fn unpack_u_decodes_one_code_point_per_u() {
        let v = unpack_vals("U*", "A\u{E9}\u{20AC}".as_bytes());
        let cps: Vec<i64> = v.iter().map(StrykeValue::to_int).collect();
        assert_eq!(cps, [0x41, 0xE9, 0x20AC]);
        // A byte that starts no UTF-8 sequence is its own code point.
        let v = unpack_vals("U2", &[0xFF, b'a']);
        let cps: Vec<i64> = v.iter().map(StrykeValue::to_int).collect();
        assert_eq!(cps, [0xFF, 0x61]);
    }
}
