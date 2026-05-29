//! Bit manipulation, music theory, hash functions,
//! text utilities, statistical tests. Pure deterministic functions.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_u64(args: &[StrykeValue], idx: usize) -> Option<u64> {
    args.get(idx).map(|v| v.to_int() as u64)
}

fn arg_str(args: &[StrykeValue], idx: usize) -> Option<String> {
    args.get(idx).map(|v| v.as_str_or_empty())
}

fn as_vec_f64(v: &StrykeValue) -> Vec<f64> {
    if let Some(a) = v.as_array_ref() {
        return a.read().iter().map(|x| x.to_number()).collect();
    }
    if let Some(a) = v.as_array_vec() {
        return a.iter().map(|x| x.to_number()).collect();
    }
    Vec::new()
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

// ══════════════════════════════════════════════════════════════════════
// Bit manipulation
// ══════════════════════════════════════════════════════════════════════
/// `bit_extract` — see implementation.
pub fn bit_extract(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as u32;
    let len = arg_i64(args, 2).unwrap_or(1).clamp(1, 64) as u32;
    let mask = if len >= 64 {
        u64::MAX
    } else {
        (1u64 << len) - 1
    };
    StrykeValue::integer(((x >> start) & mask) as i64)
}
/// `bit_insert` — see implementation.
pub fn bit_insert(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    let v = arg_u64(args, 1).unwrap_or(0);
    let start = arg_i64(args, 2).unwrap_or(0).max(0) as u32;
    let len = arg_i64(args, 3).unwrap_or(1).clamp(1, 64) as u32;
    let mask = if len >= 64 {
        u64::MAX
    } else {
        (1u64 << len) - 1
    };
    let cleared = x & !(mask << start);
    StrykeValue::integer((cleared | ((v & mask) << start)) as i64)
}
/// `bit_reverse_u8` — see implementation.
pub fn bit_reverse_u8(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0) as u8;
    StrykeValue::integer(x.reverse_bits() as i64)
}
/// `bit_reverse_u16` — see implementation.
pub fn bit_reverse_u16(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0) as u16;
    StrykeValue::integer(x.reverse_bits() as i64)
}
/// `bit_reverse_u32` — see implementation.
pub fn bit_reverse_u32(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0) as u32;
    StrykeValue::integer(x.reverse_bits() as i64)
}
/// `bit_reverse_u64` — see implementation.
pub fn bit_reverse_u64(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.reverse_bits() as i64)
}
/// `bit_rotate_left` — see implementation.
pub fn bit_rotate_left(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    let n = arg_i64(args, 1).unwrap_or(0) as u32;
    StrykeValue::integer(x.rotate_left(n & 63) as i64)
}
/// `bit_rotate_right` — see implementation.
pub fn bit_rotate_right(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    let n = arg_i64(args, 1).unwrap_or(0) as u32;
    StrykeValue::integer(x.rotate_right(n & 63) as i64)
}
/// `bit_count_ones` — see implementation.
pub fn bit_count_ones(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.count_ones() as i64)
}
/// `bit_count_zeros` — see implementation.
pub fn bit_count_zeros(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.count_zeros() as i64)
}
/// `bit_first_set` — see implementation.
pub fn bit_first_set(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    if x == 0 {
        return StrykeValue::integer(-1);
    }
    StrykeValue::integer(x.trailing_zeros() as i64)
}
/// `bit_last_set` — see implementation.
pub fn bit_last_set(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    if x == 0 {
        return StrykeValue::integer(-1);
    }
    StrykeValue::integer(63 - x.leading_zeros() as i64)
}
/// `bit_first_clear` — see implementation.
pub fn bit_first_clear(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer((!x).trailing_zeros() as i64)
}
/// `bit_last_clear` — see implementation.
pub fn bit_last_clear(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(63 - (!x).leading_zeros() as i64)
}
/// `bit_clz` — see implementation.
pub fn bit_clz(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.leading_zeros() as i64)
}
/// `bit_ctz` — see implementation.
pub fn bit_ctz(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.trailing_zeros() as i64)
}
/// `bit_parity` — see implementation.
pub fn bit_parity(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer((x.count_ones() & 1) as i64)
}
/// `bit_log2_int` — see implementation.
pub fn bit_log2_int(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    if x == 0 {
        return StrykeValue::integer(-1);
    }
    StrykeValue::integer((63 - x.leading_zeros()) as i64)
}
/// `bit_swap_bytes` — see implementation.
pub fn bit_swap_bytes(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(x.swap_bytes() as i64)
}
/// `gray_code_encode` — see implementation.
pub fn gray_code_encode(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer((x ^ (x >> 1)) as i64)
}
/// `gray_code_decode` — see implementation.
pub fn gray_code_decode(args: &[StrykeValue]) -> StrykeValue {
    let mut x = arg_u64(args, 0).unwrap_or(0);
    let mut mask = x >> 1;
    while mask != 0 {
        x ^= mask;
        mask >>= 1;
    }
    StrykeValue::integer(x as i64)
}
/// `popcount_u32` — see implementation.
pub fn popcount_u32(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0) as u32;
    StrykeValue::integer(x.count_ones() as i64)
}
/// `popcount_u64` — see implementation.
pub fn popcount_u64(args: &[StrykeValue]) -> StrykeValue {
    bit_count_ones(args)
}

// ══════════════════════════════════════════════════════════════════════
// Music theory
// ══════════════════════════════════════════════════════════════════════

const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

fn parse_note_name(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let chars: Vec<char> = s.chars().collect();
    let letter = chars[0].to_ascii_uppercase();
    let base = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let mut idx = 1;
    let mut accidental = 0i64;
    if idx < chars.len() {
        match chars[idx] {
            '#' => {
                accidental = 1;
                idx += 1;
            }
            'b' => {
                accidental = -1;
                idx += 1;
            }
            _ => {}
        }
    }
    let octave: i64 = chars[idx..].iter().collect::<String>().parse().unwrap_or(4);
    Some((octave + 1) * 12 + base + accidental)
}
/// `midi_to_note_name` — see implementation.
pub fn midi_to_note_name(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_i64(args, 0).unwrap_or(0);
    let name = NOTE_NAMES[m.rem_euclid(12) as usize];
    let octave = m.div_euclid(12) - 1;
    StrykeValue::string(format!("{name}{octave}"))
}
/// `interval_name` — see implementation.
pub fn interval_name(args: &[StrykeValue]) -> StrykeValue {
    let semitones = arg_i64(args, 0).unwrap_or(0).abs();
    let name = match semitones % 12 {
        0 => "P1/P8",
        1 => "m2",
        2 => "M2",
        3 => "m3",
        4 => "M3",
        5 => "P4",
        6 => "TT",
        7 => "P5",
        8 => "m6",
        9 => "M6",
        10 => "m7",
        11 => "M7",
        _ => "",
    };
    StrykeValue::string(name.to_string())
}

fn chord_notes(root: i64, intervals: &[i64]) -> StrykeValue {
    arr_sv(
        intervals
            .iter()
            .map(|i| StrykeValue::integer(root + i))
            .collect(),
    )
}

fn chord_root_arg(args: &[StrykeValue]) -> i64 {
    if let Some(v) = args.first() {
        if let Some(s) = v.as_str() {
            if let Some(m) = parse_note_name(&s) {
                return m;
            }
        }
        return v.to_int();
    }
    60
}
/// `chord_major` — see implementation.
pub fn chord_major(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 4, 7])
}
/// `chord_minor` — see implementation.
pub fn chord_minor(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 3, 7])
}
/// `chord_diminished` — see implementation.
pub fn chord_diminished(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 3, 6])
}
/// `chord_augmented` — see implementation.
pub fn chord_augmented(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 4, 8])
}
/// `chord_dominant7` — see implementation.
pub fn chord_dominant7(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 4, 7, 10])
}
/// `chord_major7` — see implementation.
pub fn chord_major7(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 4, 7, 11])
}
/// `chord_minor7` — see implementation.
pub fn chord_minor7(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 3, 7, 10])
}
/// `chord_diminished7` — see implementation.
pub fn chord_diminished7(args: &[StrykeValue]) -> StrykeValue {
    chord_notes(chord_root_arg(args), &[0, 3, 6, 9])
}

fn scale_notes(root: i64, intervals: &[i64]) -> StrykeValue {
    let mut sum = 0i64;
    let mut out = vec![StrykeValue::integer(root)];
    for &iv in intervals {
        sum += iv;
        out.push(StrykeValue::integer(root + sum));
    }
    arr_sv(out)
}
/// `scale_major` — see implementation.
pub fn scale_major(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 2, 1, 2, 2, 2, 1])
}
/// `scale_minor` — see implementation.
pub fn scale_minor(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 1, 2, 2, 1, 2, 2])
}
/// `scale_pentatonic` — see implementation.
pub fn scale_pentatonic(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 2, 3, 2, 3])
}
/// `scale_blues` — see implementation.
pub fn scale_blues(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[3, 2, 1, 1, 3, 2])
}
/// `scale_chromatic` — see implementation.
pub fn scale_chromatic(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[1; 11])
}
/// `scale_dorian` — see implementation.
pub fn scale_dorian(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 1, 2, 2, 2, 1, 2])
}
/// `scale_phrygian` — see implementation.
pub fn scale_phrygian(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[1, 2, 2, 2, 1, 2, 2])
}
/// `scale_lydian` — see implementation.
pub fn scale_lydian(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 2, 2, 1, 2, 2, 1])
}
/// `scale_mixolydian` — see implementation.
pub fn scale_mixolydian(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 2, 1, 2, 2, 1, 2])
}
/// `scale_locrian` — see implementation.
pub fn scale_locrian(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[1, 2, 2, 1, 2, 2, 2])
}
/// `scale_harmonic_minor` — see implementation.
pub fn scale_harmonic_minor(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 1, 2, 2, 1, 3, 1])
}
/// `scale_melodic_minor` — see implementation.
pub fn scale_melodic_minor(args: &[StrykeValue]) -> StrykeValue {
    scale_notes(chord_root_arg(args), &[2, 1, 2, 2, 2, 2, 1])
}
/// `seconds_per_beat` — see implementation.
pub fn seconds_per_beat(args: &[StrykeValue]) -> StrykeValue {
    let bpm = arg_f64(args, 0).unwrap_or(120.0).max(1.0);
    StrykeValue::float(60.0 / bpm)
}
/// `tempo_to_ms_per_beat` — see implementation.
pub fn tempo_to_ms_per_beat(args: &[StrykeValue]) -> StrykeValue {
    let bpm = arg_f64(args, 0).unwrap_or(120.0).max(1.0);
    StrykeValue::float(60_000.0 / bpm)
}

// ══════════════════════════════════════════════════════════════════════
// Hash functions
// ══════════════════════════════════════════════════════════════════════
/// `jenkins_hash` — see implementation.
pub fn jenkins_hash(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut h = 0u32;
    for b in s.bytes() {
        h = h.wrapping_add(b as u32);
        h = h.wrapping_add(h.wrapping_shl(10));
        h ^= h.wrapping_shr(6);
    }
    h = h.wrapping_add(h.wrapping_shl(3));
    h ^= h.wrapping_shr(11);
    h = h.wrapping_add(h.wrapping_shl(15));
    StrykeValue::integer(h as i64)
}
/// `loose_hash` — see implementation.
pub fn loose_hash(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut h = 0u32;
    for b in s.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    StrykeValue::integer(h as i64)
}
/// `crc8` — see implementation.
pub fn crc8(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut crc = 0u8;
    for b in s.bytes() {
        crc ^= b;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x07;
            } else {
                crc <<= 1;
            }
        }
    }
    StrykeValue::integer(crc as i64)
}
/// `crc16` — see implementation.
pub fn crc16(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut crc = 0xFFFFu16;
    for b in s.bytes() {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    StrykeValue::integer(crc as i64)
}
/// `crc16_xmodem` — see implementation.
pub fn crc16_xmodem(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut crc = 0u16;
    for b in s.bytes() {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    StrykeValue::integer(crc as i64)
}
/// `crc32_zlib` — see implementation.
pub fn crc32_zlib(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut crc = 0xFFFFFFFFu32;
    for b in s.bytes() {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    StrykeValue::integer((!crc) as i64)
}
/// `crc32c` — see implementation.
pub fn crc32c(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut crc = 0xFFFFFFFFu32;
    for b in s.bytes() {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
        }
    }
    StrykeValue::integer((!crc) as i64)
}

fn hex_of(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}
/// `hmac_md5_hex` — see implementation.
pub fn hmac_md5_hex(args: &[StrykeValue]) -> StrykeValue {
    use hmac::{Hmac, Mac};
    use md5::Md5;
    let key = arg_str(args, 0).unwrap_or_default();
    let msg = arg_str(args, 1).unwrap_or_default();
    type HmacMd5 = Hmac<Md5>;
    let mut mac = HmacMd5::new_from_slice(key.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    StrykeValue::string(hex_of(&result))
}
/// `hmac_sha1_hex` — see implementation.
pub fn hmac_sha1_hex(args: &[StrykeValue]) -> StrykeValue {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    let key = arg_str(args, 0).unwrap_or_default();
    let msg = arg_str(args, 1).unwrap_or_default();
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(key.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    StrykeValue::string(hex_of(&result))
}
/// `hmac_sha256_hex` — see implementation.
pub fn hmac_sha256_hex(args: &[StrykeValue]) -> StrykeValue {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let key = arg_str(args, 0).unwrap_or_default();
    let msg = arg_str(args, 1).unwrap_or_default();
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    StrykeValue::string(hex_of(&result))
}
/// `hmac_sha384_hex` — see implementation.
pub fn hmac_sha384_hex(args: &[StrykeValue]) -> StrykeValue {
    use hmac::{Hmac, Mac};
    use sha2::Sha384;
    let key = arg_str(args, 0).unwrap_or_default();
    let msg = arg_str(args, 1).unwrap_or_default();
    type HmacSha384 = Hmac<Sha384>;
    let mut mac = HmacSha384::new_from_slice(key.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    StrykeValue::string(hex_of(&result))
}
/// `hmac_sha512_hex` — see implementation.
pub fn hmac_sha512_hex(args: &[StrykeValue]) -> StrykeValue {
    use hmac::{Hmac, Mac};
    use sha2::Sha512;
    let key = arg_str(args, 0).unwrap_or_default();
    let msg = arg_str(args, 1).unwrap_or_default();
    type HmacSha512 = Hmac<Sha512>;
    let mut mac = HmacSha512::new_from_slice(key.as_bytes()).unwrap();
    mac.update(msg.as_bytes());
    let result = mac.finalize().into_bytes();
    StrykeValue::string(hex_of(&result))
}

// ══════════════════════════════════════════════════════════════════════
// Text utilities
// ══════════════════════════════════════════════════════════════════════
/// `detab` — see implementation.
pub fn detab(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(8).max(1) as usize;
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = width - (col % width);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            if ch == '\n' {
                col = 0;
            } else {
                col += 1;
            }
        }
    }
    StrykeValue::string(out)
}
/// `entab` — see implementation.
pub fn entab(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(8).max(1) as usize;
    let mut out = String::with_capacity(s.len());
    for line in s.split('\n') {
        let chars: Vec<char> = line.chars().collect();
        let mut col = 0usize;
        let mut buf = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == ' ' {
                let mut count = 0;
                let next_tab = ((col / width) + 1) * width;
                while i < chars.len() && chars[i] == ' ' && col + count < next_tab {
                    count += 1;
                    i += 1;
                }
                if col + count == next_tab {
                    buf.push('\t');
                    col += count;
                } else {
                    for _ in 0..count {
                        buf.push(' ');
                    }
                    col += count;
                }
            } else {
                buf.push(chars[i]);
                col += 1;
                i += 1;
            }
        }
        let _ = chars;
        out.push_str(&buf);
        out.push('\n');
    }
    if !s.ends_with('\n') {
        out.pop();
    }
    StrykeValue::string(out)
}
/// `word_wrap` — see implementation.
pub fn word_wrap(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(80).max(1) as usize;
    let mut out = String::new();
    let mut line_len = 0usize;
    let mut first_on_line = true;
    for word in s.split_whitespace() {
        let w = word.chars().count();
        if first_on_line {
            out.push_str(word);
            line_len = w;
            first_on_line = false;
        } else if line_len + 1 + w > width {
            out.push('\n');
            out.push_str(word);
            line_len = w;
        } else {
            out.push(' ');
            out.push_str(word);
            line_len += 1 + w;
        }
    }
    StrykeValue::string(out)
}
/// `strip_indent` — see implementation.
pub fn strip_indent(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let lines: Vec<&str> = s.split('\n').collect();
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let out: String = lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                *l
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    StrykeValue::string(out)
}
/// `indent_block` — see implementation.
pub fn indent_block(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let prefix = arg_str(args, 1).unwrap_or_else(|| "    ".to_string());
    let out: String = s
        .split('\n')
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n");
    StrykeValue::string(out)
}
/// `justify_left` — see implementation.
pub fn justify_left(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(s.chars().count() as i64).max(0) as usize;
    let len = s.chars().count();
    if len >= width {
        return StrykeValue::string(s);
    }
    StrykeValue::string(format!("{}{}", s, " ".repeat(width - len)))
}
/// `justify_right` — see implementation.
pub fn justify_right(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(s.chars().count() as i64).max(0) as usize;
    let len = s.chars().count();
    if len >= width {
        return StrykeValue::string(s);
    }
    StrykeValue::string(format!("{}{}", " ".repeat(width - len), s))
}
/// `justify_center` — see implementation.
pub fn justify_center(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(s.chars().count() as i64).max(0) as usize;
    let len = s.chars().count();
    if len >= width {
        return StrykeValue::string(s);
    }
    let extra = width - len;
    let left = extra / 2;
    let right = extra - left;
    StrykeValue::string(format!("{}{}{}", " ".repeat(left), s, " ".repeat(right)))
}
/// `truncate_middle` — see implementation.
pub fn truncate_middle(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let width = arg_i64(args, 1).unwrap_or(20).max(3) as usize;
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= width {
        return StrykeValue::string(s);
    }
    let keep = (width - 1) / 2;
    let left: String = chars[..keep].iter().collect();
    let right: String = chars[chars.len() - (width - keep - 1)..].iter().collect();
    StrykeValue::string(format!("{left}…{right}"))
}
/// `unicode_codepoints` — see implementation.
pub fn unicode_codepoints(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    arr_sv(s.chars().map(|c| StrykeValue::integer(c as i64)).collect())
}

// ══════════════════════════════════════════════════════════════════════
// Statistical tests
// ══════════════════════════════════════════════════════════════════════

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn variance(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64
}

fn make_result(t: f64, df: f64, p: f64) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("statistic".to_string(), StrykeValue::float(t));
    h.insert("df".to_string(), StrykeValue::float(df));
    h.insert("p_value".to_string(), StrykeValue::float(p));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

fn t_dist_p_two_sided(t: f64, df: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, StudentsT};
    if df <= 0.0 || !df.is_finite() {
        return f64::NAN;
    }
    let dist = StudentsT::new(0.0, 1.0, df).ok();
    match dist {
        Some(d) => 2.0 * (1.0 - d.cdf(t.abs())),
        None => f64::NAN,
    }
}

fn chi2_p(stat: f64, df: f64) -> f64 {
    use statrs::distribution::{ChiSquared, ContinuousCDF};
    if df <= 0.0 {
        return f64::NAN;
    }
    ChiSquared::new(df)
        .ok()
        .map(|d| 1.0 - d.cdf(stat))
        .unwrap_or(f64::NAN)
}

fn f_dist_p(stat: f64, df1: f64, df2: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, FisherSnedecor};
    FisherSnedecor::new(df1, df2)
        .ok()
        .map(|d| 1.0 - d.cdf(stat))
        .unwrap_or(f64::NAN)
}

fn normal_p_two_sided(z: f64) -> f64 {
    use statrs::distribution::{ContinuousCDF, Normal};
    let n = Normal::new(0.0, 1.0).unwrap();
    2.0 * (1.0 - n.cdf(z.abs()))
}
/// `t_test_paired` — see implementation.
pub fn t_test_paired(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    if n < 2 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let diffs: Vec<f64> = (0..n).map(|i| a[i] - b[i]).collect();
    let m = mean(&diffs);
    let s = variance(&diffs).sqrt();
    if s == 0.0 {
        return make_result(f64::NAN, (n - 1) as f64, f64::NAN);
    }
    let t = m / (s / (n as f64).sqrt());
    let df = (n - 1) as f64;
    let p = t_dist_p_two_sided(t, df);
    make_result(t, df, p)
}
/// `chi_square_goodness_fit` — see implementation.
pub fn chi_square_goodness_fit(args: &[StrykeValue]) -> StrykeValue {
    let obs = args.first().map(as_vec_f64).unwrap_or_default();
    let exp = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = obs.len().min(exp.len());
    if n < 2 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let stat: f64 = (0..n)
        .filter(|&i| exp[i] > 0.0)
        .map(|i| (obs[i] - exp[i]).powi(2) / exp[i])
        .sum();
    let df = (n - 1) as f64;
    let p = chi2_p(stat, df);
    make_result(stat, df, p)
}
/// `chi_square_independence` — see implementation.
pub fn chi_square_independence(args: &[StrykeValue]) -> StrykeValue {
    let m = args
        .first()
        .map(|v| {
            if let Some(rows) = v.as_array_ref() {
                rows.read().iter().map(as_vec_f64).collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .unwrap_or_default();
    if m.is_empty() || m[0].is_empty() {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let rows = m.len();
    let cols = m[0].len();
    let row_sums: Vec<f64> = m.iter().map(|r| r.iter().sum()).collect();
    let col_sums: Vec<f64> = (0..cols).map(|j| m.iter().map(|r| r[j]).sum()).collect();
    let total: f64 = row_sums.iter().sum();
    if total == 0.0 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let mut stat = 0.0;
    for i in 0..rows {
        for j in 0..cols {
            let exp = row_sums[i] * col_sums[j] / total;
            if exp > 0.0 {
                stat += (m[i][j] - exp).powi(2) / exp;
            }
        }
    }
    let df = ((rows - 1) * (cols - 1)) as f64;
    let p = chi2_p(stat, df);
    make_result(stat, df, p)
}
/// `anova_one_way` — see implementation.
pub fn anova_one_way(args: &[StrykeValue]) -> StrykeValue {
    let groups: Vec<Vec<f64>> = args
        .iter()
        .map(as_vec_f64)
        .filter(|g| !g.is_empty())
        .collect();
    let k = groups.len();
    if k < 2 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let n: usize = groups.iter().map(|g| g.len()).sum();
    let grand: f64 = groups.iter().flat_map(|g| g.iter()).sum::<f64>() / n as f64;
    let ss_between: f64 = groups
        .iter()
        .map(|g| {
            let m = mean(g);
            g.len() as f64 * (m - grand).powi(2)
        })
        .sum();
    let ss_within: f64 = groups
        .iter()
        .map(|g| {
            let m = mean(g);
            g.iter().map(|x| (x - m).powi(2)).sum::<f64>()
        })
        .sum();
    let df1 = (k - 1) as f64;
    let df2 = (n - k) as f64;
    if df2 <= 0.0 || ss_within == 0.0 {
        return make_result(f64::NAN, df1, f64::NAN);
    }
    let f_stat = (ss_between / df1) / (ss_within / df2);
    let p = f_dist_p(f_stat, df1, df2);
    make_result(f_stat, df1, p)
}
/// `rank_data` — see implementation.
pub fn rank_data(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let n = xs.len();
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        xs[a]
            .partial_cmp(&xs[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut ranks = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && xs[indices[j + 1]] == xs[indices[i]] {
            j += 1;
        }
        let avg_rank = (i + j) as f64 / 2.0 + 1.0;
        for k in i..=j {
            ranks[indices[k]] = avg_rank;
        }
        i = j + 1;
    }
    arr_f64(ranks)
}
/// `mann_whitney_u` — see implementation.
pub fn mann_whitney_u(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let combined: Vec<f64> = a.iter().chain(b.iter()).cloned().collect();
    let ranks_sv = rank_data(&[arr_f64(combined)]);
    let ranks = as_vec_f64(&ranks_sv);
    let r1: f64 = ranks[..n1].iter().sum();
    let u1 = r1 - (n1 * (n1 + 1)) as f64 / 2.0;
    let u2 = (n1 * n2) as f64 - u1;
    let u = u1.min(u2);
    let mean_u = (n1 * n2) as f64 / 2.0;
    let sigma = (((n1 * n2 * (n1 + n2 + 1)) as f64) / 12.0).sqrt();
    let z = if sigma > 0.0 {
        (u - mean_u) / sigma
    } else {
        0.0
    };
    let p = normal_p_two_sided(z);
    make_result(u, (n1 + n2) as f64, p)
}
/// `kruskal_wallis` — see implementation.
pub fn kruskal_wallis(args: &[StrykeValue]) -> StrykeValue {
    let groups: Vec<Vec<f64>> = args
        .iter()
        .map(as_vec_f64)
        .filter(|g| !g.is_empty())
        .collect();
    let k = groups.len();
    if k < 2 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let n: usize = groups.iter().map(|g| g.len()).sum();
    let combined: Vec<f64> = groups.iter().flat_map(|g| g.iter().cloned()).collect();
    let ranks = as_vec_f64(&rank_data(&[arr_f64(combined)]));
    let mut h = 0.0;
    let mut idx = 0;
    for g in &groups {
        let group_ranks: Vec<f64> = ranks[idx..idx + g.len()].to_vec();
        let rsum: f64 = group_ranks.iter().sum();
        h += rsum.powi(2) / g.len() as f64;
        idx += g.len();
    }
    let h_stat = 12.0 / (n * (n + 1)) as f64 * h - 3.0 * (n + 1) as f64;
    let df = (k - 1) as f64;
    let p = chi2_p(h_stat, df);
    make_result(h_stat, df, p)
}
/// `ks_test_one_sample` — see implementation.
pub fn ks_test_one_sample(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let mu = arg_f64(args, 1).unwrap_or(0.0);
    let sigma = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 0 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    use statrs::distribution::{ContinuousCDF, Normal};
    let dist = Normal::new(mu, sigma).unwrap();
    let mut d: f64 = 0.0;
    for (i, x) in sorted.iter().enumerate() {
        let cdf = dist.cdf(*x);
        let lo = (i as f64) / n as f64;
        let hi = (i as f64 + 1.0) / n as f64;
        d = d.max((cdf - lo).abs()).max((hi - cdf).abs());
    }
    let p = (-2.0 * (n as f64) * d.powi(2)).exp();
    make_result(d, n as f64, p)
}
/// `ks_test_two_sample` — see implementation.
pub fn ks_test_two_sample(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let mut sa = a;
    let mut sb = b;
    sa.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sb.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut d: f64 = 0.0;
    let mut i = 0;
    let mut j = 0;
    let mut cdf_a = 0.0;
    let mut cdf_b = 0.0;
    while i < n1 && j < n2 {
        if sa[i] <= sb[j] {
            cdf_a = (i + 1) as f64 / n1 as f64;
            i += 1;
        } else {
            cdf_b = (j + 1) as f64 / n2 as f64;
            j += 1;
        }
        d = d.max((cdf_a - cdf_b).abs());
    }
    let en = ((n1 * n2) as f64 / (n1 + n2) as f64).sqrt();
    let lambda = (en + 0.12 + 0.11 / en) * d;
    let mut p = 0.0;
    let mut sign = 2.0;
    for k in 1..=100 {
        let term = sign * (-2.0 * (k as f64).powi(2) * lambda.powi(2)).exp();
        p += term;
        sign = -sign;
        if term.abs() < 1e-10 {
            break;
        }
    }
    make_result(d, (n1 + n2) as f64, p.clamp(0.0, 1.0))
}
/// `binomial_test` — see implementation.
pub fn binomial_test(args: &[StrykeValue]) -> StrykeValue {
    let k = arg_i64(args, 0).unwrap_or(0).max(0) as u64;
    let n = arg_i64(args, 1).unwrap_or(0).max(0) as u64;
    let p = arg_f64(args, 2).unwrap_or(0.5).clamp(0.0, 1.0);
    if n == 0 {
        return make_result(0.0, 0.0, f64::NAN);
    }
    use statrs::distribution::{Binomial, DiscreteCDF};
    let dist = match Binomial::new(p, n) {
        Ok(d) => d,
        Err(_) => return make_result(0.0, 0.0, f64::NAN),
    };
    let cdf_low = dist.cdf(k);
    let cdf_high = 1.0 - dist.cdf(k.saturating_sub(1));
    let p_val = (2.0 * cdf_low.min(cdf_high)).min(1.0);
    make_result(k as f64, n as f64, p_val)
}
/// `proportion_test` — see implementation.
pub fn proportion_test(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let n = arg_f64(args, 1).unwrap_or(1.0).max(1.0);
    let p0 = arg_f64(args, 2).unwrap_or(0.5);
    let p_hat = x / n;
    let se = (p0 * (1.0 - p0) / n).sqrt();
    let z = if se > 0.0 { (p_hat - p0) / se } else { 0.0 };
    let p = normal_p_two_sided(z);
    make_result(z, n - 1.0, p)
}
/// `fisher_exact_2x2` — see implementation.
pub fn fisher_exact_2x2(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_i64(args, 0).unwrap_or(0).max(0) as u64;
    let b = arg_i64(args, 1).unwrap_or(0).max(0) as u64;
    let c = arg_i64(args, 2).unwrap_or(0).max(0) as u64;
    let d = arg_i64(args, 3).unwrap_or(0).max(0) as u64;
    fn lfact(n: u64) -> f64 {
        if n == 0 {
            return 0.0;
        }
        libm::lgamma((n + 1) as f64)
    }
    let row1 = a + b;
    let row2 = c + d;
    let col1 = a + c;
    let col2 = b + d;
    let total = row1 + row2;
    let log_p_observed = lfact(row1) + lfact(row2) + lfact(col1) + lfact(col2)
        - lfact(total)
        - lfact(a)
        - lfact(b)
        - lfact(c)
        - lfact(d);
    // Two-sided: sum p(k) for all valid tables with p(k) <= p(observed)
    let lo = col1.saturating_sub(row2);
    let hi = col1.min(row1);
    let mut p_sum = 0.0;
    for k in lo..=hi {
        let bb = row1.saturating_sub(k);
        let cc = col1.saturating_sub(k);
        let dd = (row2).saturating_sub(cc);
        let log_p = lfact(row1) + lfact(row2) + lfact(col1) + lfact(col2)
            - lfact(total)
            - lfact(k)
            - lfact(bb)
            - lfact(cc)
            - lfact(dd);
        if log_p <= log_p_observed + 1e-12 {
            p_sum += log_p.exp();
        }
    }
    make_result(log_p_observed.exp(), (total - 1) as f64, p_sum.min(1.0))
}
/// `wilcoxon_signed_rank` — see implementation.
pub fn wilcoxon_signed_rank(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    if n < 2 {
        return make_result(f64::NAN, 0.0, f64::NAN);
    }
    let mut diffs: Vec<(f64, i32)> = Vec::new();
    for i in 0..n {
        let d = a[i] - b[i];
        if d != 0.0 {
            diffs.push((d.abs(), if d > 0.0 { 1 } else { -1 }));
        }
    }
    let m = diffs.len();
    if m == 0 {
        return make_result(0.0, 0.0, 1.0);
    }
    let abs_diffs: Vec<f64> = diffs.iter().map(|x| x.0).collect();
    let ranks = as_vec_f64(&rank_data(&[arr_f64(abs_diffs)]));
    let w_plus: f64 = diffs
        .iter()
        .zip(ranks.iter())
        .filter(|(d, _)| d.1 > 0)
        .map(|(_, r)| r)
        .sum();
    let w_minus: f64 = diffs
        .iter()
        .zip(ranks.iter())
        .filter(|(d, _)| d.1 < 0)
        .map(|(_, r)| r)
        .sum();
    let w = w_plus.min(w_minus);
    let mean_w = (m * (m + 1)) as f64 / 4.0;
    let var_w = (m * (m + 1) * (2 * m + 1)) as f64 / 24.0;
    let z = if var_w > 0.0 {
        (w - mean_w) / var_w.sqrt()
    } else {
        0.0
    };
    let p = normal_p_two_sided(z);
    make_result(w, m as f64, p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv_i(x: i64) -> StrykeValue {
        StrykeValue::integer(x)
    }
    fn sv_s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }

    #[test]
    fn bit_reverse_u32_simple() {
        let r = bit_reverse_u32(&[sv_i(1)]).to_int();
        assert_eq!(r as u32, 0x80000000);
    }

    #[test]
    fn bit_count_ones_basic() {
        assert_eq!(bit_count_ones(&[sv_i(0b1011)]).to_int(), 3);
        assert_eq!(bit_count_ones(&[sv_i(0)]).to_int(), 0);
    }

    #[test]
    fn gray_code_roundtrip() {
        for i in 0..256u64 {
            let g = gray_code_encode(&[sv_i(i as i64)]).to_int();
            let back = gray_code_decode(&[sv_i(g)]).to_int();
            assert_eq!(back, i as i64);
        }
    }

    #[test]
    fn chord_major_c4() {
        let notes = chord_major(&[sv_i(60)]);
        let v: Vec<i64> = if let Some(a) = notes.as_array_ref() {
            a.read().iter().map(|x| x.to_int()).collect()
        } else {
            vec![]
        };
        assert_eq!(v, vec![60, 64, 67]);
    }

    #[test]
    fn scale_major_c() {
        let notes = scale_major(&[sv_i(60)]);
        let v: Vec<i64> = if let Some(a) = notes.as_array_ref() {
            a.read().iter().map(|x| x.to_int()).collect()
        } else {
            vec![]
        };
        assert_eq!(v, vec![60, 62, 64, 65, 67, 69, 71, 72]);
    }

    #[test]
    fn chord_minor_from_name() {
        let notes = chord_minor(&[sv_s("A4")]);
        let v: Vec<i64> = if let Some(a) = notes.as_array_ref() {
            a.read().iter().map(|x| x.to_int()).collect()
        } else {
            vec![]
        };
        assert_eq!(v, vec![69, 72, 76]);
    }

    #[test]
    fn midi_to_note_60() {
        let r = midi_to_note_name(&[sv_i(60)]);
        assert_eq!(r.as_str_or_empty(), "C4");
    }

    #[test]
    fn crc16_known() {
        // CRC16-Modbus of "123456789" = 0x4B37
        let r = crc16(&[sv_s("123456789")]).to_int();
        assert_eq!(r as u16, 0x4B37);
    }

    #[test]
    fn crc32_zlib_known() {
        // CRC32 of "123456789" = 0xCBF43926
        let r = crc32_zlib(&[sv_s("123456789")]).to_int();
        assert_eq!(r as u32, 0xCBF43926);
    }

    #[test]
    fn hmac_sha256_rfc4231() {
        // RFC 4231 Test Case 1: key=0x0b*20 (20 bytes), data="Hi There"
        let key =
            "\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b";
        let r = hmac_sha256_hex(&[sv_s(key), sv_s("Hi There")]).as_str_or_empty();
        assert_eq!(
            r,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn detab_8col() {
        let r = detab(&[sv_s("\tab"), sv_i(8)]).as_str_or_empty();
        assert_eq!(r, "        ab");
    }

    #[test]
    fn justify_basic() {
        assert_eq!(
            justify_left(&[sv_s("hi"), sv_i(5)]).as_str_or_empty(),
            "hi   "
        );
        assert_eq!(
            justify_right(&[sv_s("hi"), sv_i(5)]).as_str_or_empty(),
            "   hi"
        );
        assert_eq!(
            justify_center(&[sv_s("hi"), sv_i(6)]).as_str_or_empty(),
            "  hi  "
        );
    }

    #[test]
    fn t_test_paired_significant() {
        let a = arr_f64(vec![10.0, 11.0, 12.0, 13.0, 14.0]);
        let b = arr_f64(vec![9.0, 9.5, 10.5, 11.0, 12.0]);
        let r = t_test_paired(&[a, b]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert!(h.get("statistic").unwrap().to_number() > 0.0);
            let p = h.get("p_value").unwrap().to_number();
            assert!((0.0..=1.0).contains(&p));
        }
    }

    #[test]
    fn rank_data_with_ties() {
        let r = rank_data(&[arr_f64(vec![10.0, 20.0, 20.0, 30.0])]);
        let v = as_vec_f64(&r);
        assert_eq!(v, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn chi_square_goodness_basic() {
        let o = arr_f64(vec![10.0, 20.0, 30.0]);
        let e = arr_f64(vec![10.0, 20.0, 30.0]);
        let r = chi_square_goodness_fit(&[o, e]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert!(h.get("statistic").unwrap().to_number() < 1e-9);
        }
    }

    #[test]
    fn anova_distinct_groups() {
        let g1 = arr_f64(vec![1.0, 2.0, 3.0]);
        let g2 = arr_f64(vec![10.0, 11.0, 12.0]);
        let g3 = arr_f64(vec![100.0, 101.0, 102.0]);
        let r = anova_one_way(&[g1, g2, g3]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert!(h.get("statistic").unwrap().to_number() > 100.0);
            assert!(h.get("p_value").unwrap().to_number() < 0.01);
        }
    }
}
