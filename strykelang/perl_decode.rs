//! UTF-8 vs Latin-1 octet decoding for Perl-compatible text (no dependency on [`crate::value`]).
//! Used by I/O, `par_lines`, and byte stringification.

/// Decode bytes as UTF-8 when the buffer is entirely valid UTF-8. Otherwise treat the buffer as
/// newline-separated records: each line is UTF-8 if valid, else each byte maps to U+0000..U+00FF
/// (Latin-1 / octet semantics, like Perl’s default handling of non-UTF-8 text rather than U+FFFD).
pub fn decode_utf8_or_latin1(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let mut out = String::new();
            let mut first = true;
            for mut segment in bytes.split(|b| *b == b'\n') {
                if !first {
                    out.push('\n');
                }
                first = false;
                if segment.ends_with(b"\r") {
                    segment = &segment[..segment.len() - 1];
                }
                out.push_str(&decode_utf8_or_latin1_line(segment));
            }
            out
        }
    }
}

/// One line or fragment without embedded `\n` (e.g. `par_lines` slice): UTF-8 if valid, else
/// Latin-1 octets (U+0000..U+00FF).
pub fn decode_utf8_or_latin1_line(line: &[u8]) -> String {
    match std::str::from_utf8(line) {
        Ok(s) => s.to_string(),
        Err(_) => line
            .iter()
            .map(|&b| char::from_u32(u32::from(b)).unwrap())
            .collect(),
    }
}

/// One physical line from [`std::io::BufRead::read_until`] (at most one `\n`), UTF-8 or Latin-1 per byte.
pub fn decode_utf8_or_latin1_read_until(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw
            .iter()
            .map(|&b| char::from_u32(u32::from(b)).unwrap())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_utf8_or_latin1_accepts_valid_utf8_whole_file() {
        let s = decode_utf8_or_latin1("café".as_bytes());
        assert_eq!(s, "café");
    }

    #[test]
    fn decode_utf8_or_latin1_line_maps_octets() {
        assert_eq!(
            decode_utf8_or_latin1_line(&[0xff, 0xfe]),
            "\u{00ff}\u{00fe}"
        );
    }

    #[test]
    fn decode_utf8_or_latin1_read_until_falls_back_per_byte_when_not_utf8() {
        assert_eq!(
            decode_utf8_or_latin1_read_until(&[b'a', 0xff, b'\n']),
            "a\u{00ff}\n"
        );
    }

    #[test]
    fn decode_utf8_or_latin1_multiline_latin1_only_on_later_lines() {
        let mut v = b"ascii\n".to_vec();
        v.push(0xfe);
        v.push(b'\n');
        assert_eq!(decode_utf8_or_latin1(&v), "ascii\n\u{00fe}\n");
    }
}
