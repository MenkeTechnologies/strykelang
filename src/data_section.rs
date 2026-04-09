//! Split `__DATA__` from program source (line must equal `__DATA__` after trim).

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
    use super::split_data_section;

    #[test]
    fn no_marker_returns_full() {
        let (p, d) = split_data_section("print 1;\n");
        assert_eq!(p, "print 1;\n");
        assert!(d.is_none());
    }

    #[test]
    fn splits_at_data() {
        let (p, d) = split_data_section("say 1;\n__DATA__\na\nb\n");
        assert_eq!(p, "say 1;");
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
