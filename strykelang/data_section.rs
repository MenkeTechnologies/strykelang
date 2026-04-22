//! Split `__DATA__` from program source (line must equal `__DATA__` after trim).

/// Truncate at the first line equal to `__END__` or `__DATA__` (after [`str::trim`]). Perl stops
/// compiling there; required `.pm` files often put pod after `__END__`.
pub fn strip_perl_end_marker(content: &str) -> &str {
    let mut start = 0usize;
    for chunk in content.split_inclusive('\n') {
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        if line.trim() == "__END__" || line.trim() == "__DATA__" {
            return &content[..start];
        }
        start += chunk.len();
    }
    content
}

/// Returns `(program_text_before_marker, Some(data bytes after marker))` or `(full, None)`.
pub fn split_data_section(content: &str) -> (String, Option<Vec<u8>>) {
    let mut prog = String::new();
    let mut in_data = false;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        if !in_data && line.trim_end() == "__DATA__" {
            in_data = true;
            continue;
        }
        if in_data {
            data_lines.push(line);
        } else {
            if !prog.is_empty() {
                prog.push('\n');
            }
            prog.push_str(line);
        }
    }

    if in_data {
        let mut data = data_lines.join("\n");
        if !data.is_empty() {
            data.push('\n');
        }
        (prog, Some(data.into_bytes()))
    } else {
        (content.to_string(), None)
    }
}

#[cfg(test)]
mod tests {
    use super::{split_data_section, strip_perl_end_marker};

    #[test]
    fn strip_end_before_pod() {
        let s = "1;\n__END__\n=pod\n";
        assert_eq!(strip_perl_end_marker(s), "1;\n");
    }

    #[test]
    fn strip_data_truncates_like_end() {
        let s = "use strict;\n__DATA__\ntrailing\n";
        assert_eq!(strip_perl_end_marker(s), "use strict;\n");
    }

    #[test]
    fn no_marker_returns_full() {
        let (p, d) = split_data_section("print 1;\n");
        assert_eq!(p, "print 1;\n");
        assert!(d.is_none());
    }

    #[test]
    fn splits_at_data() {
        let (p, d) = split_data_section("p 1;\n__DATA__\na\nb\n");
        assert_eq!(p, "p 1;");
        assert_eq!(d, Some(b"a\nb\n".to_vec()));
    }

    #[test]
    fn data_marker_only_yields_empty_program() {
        let (p, d) = split_data_section("__DATA__\n");
        assert_eq!(p, "");
        assert_eq!(d, Some(Vec::new()));
    }

    #[test]
    fn data_marker_with_trailing_spaces_on_line() {
        let (p, d) = split_data_section("1;\n__DATA__   \nbody\n");
        assert_eq!(p, "1;");
        assert_eq!(d, Some(b"body\n".to_vec()));
    }

    #[test]
    fn no_newline_after_last_program_line_before_marker() {
        let (p, d) = split_data_section("print\n__DATA__\nx");
        assert_eq!(p, "print");
        assert_eq!(d, Some(b"x\n".to_vec()));
    }
}
