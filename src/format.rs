//! Perl `format` / `write` — picture lines and field padding (subset of Perl 5 `perlform`).

use crate::ast::Expr;
use crate::error::{PerlError, PerlResult};
use crate::parser::parse_format_value_line;

/// Parsed `format NAME = ... .` body (after registration).
#[derive(Debug, Clone)]
pub struct FormatTemplate {
    pub records: Vec<FormatRecord>,
}

#[derive(Debug, Clone)]
pub enum FormatRecord {
    /// Line with no `@` fields — printed as-is (newline added by `write`).
    Literal(String),
    /// Picture line with `@…` fields plus the following value line (comma-separated exprs).
    Picture {
        segments: Vec<PictureSegment>,
        exprs: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldAlign {
    Left,
    Right,
    Center,
    Numeric,
    Multiline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Numeric,
    Multiline,
}

#[derive(Debug, Clone)]
pub enum PictureSegment {
    Literal(String),
    Field {
        width: usize,
        align: FieldAlign,
        kind: FieldKind,
    },
}

/// Build a template from raw lines between `format N =` and `.`.
pub fn parse_format_template(lines: &[String]) -> PerlResult<FormatTemplate> {
    let mut records = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let pic_line = &lines[i];
        if !pic_line.contains('@') {
            records.push(FormatRecord::Literal(pic_line.clone()));
            i += 1;
            continue;
        }
        let segments = parse_picture_segments(pic_line)?;
        let n_fields = segments
            .iter()
            .filter(|s| matches!(s, PictureSegment::Field { .. }))
            .count();
        i += 1;
        if i >= lines.len() {
            return Err(PerlError::syntax(
                "picture line with @ fields must be followed by a value line",
                0,
            ));
        }
        let exprs = parse_format_value_line(&lines[i])?;
        if exprs.len() != n_fields {
            return Err(PerlError::syntax(
                format!(
                    "format: {} picture field(s) but {} value expression(s)",
                    n_fields,
                    exprs.len()
                ),
                0,
            ));
        }
        records.push(FormatRecord::Picture { segments, exprs });
        i += 1;
    }
    Ok(FormatTemplate { records })
}

fn parse_picture_segments(pic: &str) -> PerlResult<Vec<PictureSegment>> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut chars = pic.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            if chars.peek() == Some(&'@') {
                chars.next();
                lit.push('@');
                continue;
            }
            if !lit.is_empty() {
                out.push(PictureSegment::Literal(std::mem::take(&mut lit)));
            }
            let mut width = 0usize;
            let align = match chars.peek() {
                Some('<') => {
                    while chars.peek() == Some(&'<') {
                        chars.next();
                        width += 1;
                    }
                    FieldAlign::Left
                }
                Some('>') => {
                    while chars.peek() == Some(&'>') {
                        chars.next();
                        width += 1;
                    }
                    FieldAlign::Right
                }
                Some('|') => {
                    while chars.peek() == Some(&'|') {
                        chars.next();
                        width += 1;
                    }
                    FieldAlign::Center
                }
                Some('#') => {
                    while chars.peek() == Some(&'#') {
                        chars.next();
                        width += 1;
                    }
                    FieldAlign::Numeric
                }
                Some('*') => {
                    while chars.peek() == Some(&'*') {
                        chars.next();
                        width += 1;
                    }
                    FieldAlign::Multiline
                }
                _ => {
                    width = 1;
                    FieldAlign::Left
                }
            };
            if width == 0 {
                width = 1;
            }
            let kind = match align {
                FieldAlign::Numeric => FieldKind::Numeric,
                FieldAlign::Multiline => FieldKind::Multiline,
                _ => FieldKind::Text,
            };
            out.push(PictureSegment::Field {
                width,
                align,
                kind,
            });
        } else {
            lit.push(c);
        }
    }
    if !lit.is_empty() {
        out.push(PictureSegment::Literal(lit));
    }
    Ok(out)
}

/// Pad/truncate a value to `width` display columns (character count).
pub fn pad_field(s: &str, width: usize, align: FieldAlign) -> String {
    let s = if s.chars().count() > width {
        s.chars().take(width).collect::<String>()
    } else {
        s.to_string()
    };
    let len = s.chars().count();
    match align {
        FieldAlign::Left => {
            let pad = width.saturating_sub(len);
            format!("{}{}", s, " ".repeat(pad))
        }
        FieldAlign::Multiline => {
            let first = s.lines().next().unwrap_or("");
            let fl = first.chars().count();
            let t = if fl > width {
                first.chars().take(width).collect::<String>()
            } else {
                first.to_string()
            };
            let pad = width.saturating_sub(t.chars().count());
            format!("{}{}", t, " ".repeat(pad))
        }
        FieldAlign::Right => {
            let pad = width.saturating_sub(len);
            format!("{}{}", " ".repeat(pad), s)
        }
        FieldAlign::Center => {
            let pad = width.saturating_sub(len);
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
        }
        FieldAlign::Numeric => {
            if let Ok(n) = s.parse::<i64>() {
                format!("{n:>width$}", n = n, width = width)
            } else if let Ok(f) = s.parse::<f64>() {
                format!("{f:>width$}", f = f, width = width)
            } else {
                let pad = width.saturating_sub(len);
                format!("{}{}", " ".repeat(pad), s)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_format_template_empty() {
        let t = parse_format_template(&[]).expect("parse");
        assert!(t.records.is_empty());
    }

    #[test]
    fn parse_format_template_literal_only() {
        let t = parse_format_template(&["no fields here".to_string()]).expect("parse");
        assert_eq!(t.records.len(), 1);
        assert!(matches!(
            &t.records[0],
            FormatRecord::Literal(s) if s == "no fields here"
        ));
    }

    #[test]
    fn parse_format_template_picture_and_value_line() {
        let t = parse_format_template(&[
            "@<<<<".to_string(),
            r#"qq(ab)"#.to_string(),
        ])
        .expect("parse");
        assert_eq!(t.records.len(), 1);
        let FormatRecord::Picture { segments, exprs } = &t.records[0] else {
            panic!("expected picture");
        };
        assert_eq!(exprs.len(), 1);
        assert_eq!(segments.len(), 1);
        assert!(matches!(
            &segments[0],
            PictureSegment::Field {
                width: 4,
                align: FieldAlign::Left,
                kind: FieldKind::Text,
            }
        ));
    }

    #[test]
    fn parse_format_template_doubled_at_is_literal_at() {
        // Any line containing `@` is a picture line; `@@` escapes to one `@` in the picture.
        // With zero fields, the value line must be empty.
        let t = parse_format_template(&["@@email".to_string(), "".to_string()]).expect("parse");
        let FormatRecord::Picture { segments, exprs } = &t.records[0] else {
            panic!("expected picture");
        };
        assert!(exprs.is_empty());
        assert!(matches!(
            segments.as_slice(),
            [PictureSegment::Literal(s)] if s == "@email"
        ));
    }

    #[test]
    fn parse_format_template_picture_requires_value_line() {
        let err = parse_format_template(&["@<<<<".to_string()]).expect_err("missing value");
        assert!(err.to_string().contains("value line"));
    }

    #[test]
    fn parse_format_template_field_count_mismatch() {
        let err = parse_format_template(&["@<<, @<<".to_string(), "1".to_string()]).expect_err("mismatch");
        assert!(err.to_string().contains("picture field"));
    }

    #[test]
    fn parse_format_template_two_fields_two_exprs() {
        let t = parse_format_template(&["@<< @>>".to_string(), "1, 2".to_string()]).expect("parse");
        assert_eq!(t.records.len(), 1);
        let FormatRecord::Picture { exprs, .. } = &t.records[0] else {
            panic!("expected picture");
        };
        assert_eq!(exprs.len(), 2);
    }

    #[test]
    fn parse_format_value_line_qq_comma_qq_is_two_exprs() {
        let v = parse_format_value_line("qq(x), qq(y)").expect("parse");
        assert_eq!(
            v.len(),
            2,
            "comma-separated qq() should be two value expressions"
        );
    }

    #[test]
    fn pad_field_left_aligns_and_pads() {
        assert_eq!(pad_field("hi", 5, FieldAlign::Left), "hi   ");
    }

    #[test]
    fn pad_field_right_aligns() {
        assert_eq!(pad_field("hi", 5, FieldAlign::Right), "   hi");
    }

    #[test]
    fn pad_field_center_aligns() {
        assert_eq!(pad_field("hi", 5, FieldAlign::Center), " hi  ");
    }

    #[test]
    fn pad_field_numeric_right_aligns_integer() {
        assert_eq!(pad_field("42", 5, FieldAlign::Numeric), "   42");
    }

    #[test]
    fn pad_field_truncates_to_width() {
        assert_eq!(pad_field("abcdef", 3, FieldAlign::Left), "abc");
    }

    #[test]
    fn pad_field_multiline_uses_first_line() {
        assert_eq!(
            pad_field("first\nsecond", 6, FieldAlign::Multiline),
            "first "
        );
    }
}
