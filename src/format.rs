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
