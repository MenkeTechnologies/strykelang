//! Triple-engine compiled regex: [`regex`] (fast subset), [`fancy_regex`] (backrefs, etc.),
//! then [`pcre2`] for patterns neither accepts (PCRE2-specific syntax).

use std::sync::Arc;

use crate::value::PerlValue;

/// Compiled pattern: Rust [`regex`], [`fancy_regex`], or PCRE2.
#[derive(Debug, Clone)]
pub enum PerlCompiledRegex {
    Rust(Arc<regex::Regex>),
    Fancy(Arc<fancy_regex::Regex>),
    Pcre2(Arc<pcre2::bytes::Regex>),
}

/// Unified captures for match-variable setup (`$1`, `@-`, `%+`, …).
#[derive(Debug)]
pub enum PerlCaptures<'a> {
    Rust(regex::Captures<'a>),
    Fancy(fancy_regex::Captures<'a>),
    Pcre2 {
        caps: pcre2::bytes::Captures<'a>,
        hay: &'a str,
    },
}

impl<'a> PerlCaptures<'a> {
    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Self::Rust(c) => c.len(),
            Self::Fancy(c) => c.len(),
            Self::Pcre2 { caps, .. } => caps.len(),
        }
    }

    #[inline]
    pub fn get(&self, i: usize) -> Option<RegexMatch<'a>> {
        match self {
            Self::Rust(c) => c.get(i).map(Into::into),
            Self::Fancy(c) => c.get(i).map(Into::into),
            Self::Pcre2 { caps, hay } => caps
                .get(i)
                .map(|m| regex_match_from_pcre(hay, m)),
        }
    }

    #[inline]
    pub fn name(&self, name: &str) -> Option<RegexMatch<'a>> {
        match self {
            Self::Rust(c) => c.name(name).map(Into::into),
            Self::Fancy(c) => c.name(name).map(Into::into),
            Self::Pcre2 { caps, hay } => caps
                .name(name)
                .map(|m| regex_match_from_pcre(hay, m)),
        }
    }
}

fn regex_match_from_pcre<'a>(subject: &'a str, m: pcre2::bytes::Match<'a>) -> RegexMatch<'a> {
    let start = m.start();
    let end = m.end();
    let text = subject.get(start..end).unwrap_or("");
    RegexMatch { start, end, text }
}

/// Minimal match view shared by engines.
#[derive(Clone, Copy, Debug)]
pub struct RegexMatch<'a> {
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
}

impl<'a> From<regex::Match<'a>> for RegexMatch<'a> {
    fn from(m: regex::Match<'a>) -> Self {
        Self {
            start: m.start(),
            end: m.end(),
            text: m.as_str(),
        }
    }
}

impl<'a> From<fancy_regex::Match<'a>> for RegexMatch<'a> {
    fn from(m: fancy_regex::Match<'a>) -> Self {
        Self {
            start: m.start(),
            end: m.end(),
            text: m.as_str(),
        }
    }
}

impl PerlCompiledRegex {
    /// Compile `re_str` (already Perl-expanded: flags as `(?i)` etc. are in the string).
    /// Tries [`regex::Regex`], then [`fancy_regex::Regex`], then [`pcre2::bytes::Regex`].
    pub fn compile(re_str: &str) -> Result<Arc<Self>, String> {
        if let Ok(r) = regex::Regex::new(re_str) {
            return Ok(Arc::new(Self::Rust(Arc::new(r))));
        }
        if let Ok(r) = fancy_regex::Regex::new(re_str) {
            return Ok(Arc::new(Self::Fancy(Arc::new(r))));
        }
        match pcre2::bytes::Regex::new(re_str) {
            Ok(r) => Ok(Arc::new(Self::Pcre2(Arc::new(r)))),
            Err(e) => Err(e.to_string()),
        }
    }

    #[inline]
    pub fn is_match(&self, s: &str) -> bool {
        match self {
            Self::Rust(r) => r.is_match(s),
            Self::Fancy(r) => r.is_match(s).unwrap_or(false),
            Self::Pcre2(r) => r.is_match(s.as_bytes()).unwrap_or(false),
        }
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<PerlCaptures<'t>> {
        match self {
            Self::Rust(r) => r.captures(text).map(PerlCaptures::Rust),
            Self::Fancy(r) => match r.captures(text) {
                Ok(Some(c)) => Some(PerlCaptures::Fancy(c)),
                _ => None,
            },
            Self::Pcre2(r) => match r.captures(text.as_bytes()) {
                Ok(Some(c)) => Some(PerlCaptures::Pcre2 { caps: c, hay: text }),
                _ => None,
            },
        }
    }

    /// Iterator over all non-overlapping capture sets (for `/g` in list context).
    pub fn captures_iter<'r, 't>(
        &'r self,
        text: &'t str,
    ) -> CaptureIter<'r, 't> {
        match self {
            Self::Rust(r) => CaptureIter::Rust(r.captures_iter(text)),
            Self::Fancy(r) => CaptureIter::Fancy(r.captures_iter(text)),
            Self::Pcre2(r) => CaptureIter::Pcre2 {
                hay: text,
                it: r.captures_iter(text.as_bytes()),
            },
        }
    }

    pub fn capture_names(&self) -> CaptureNames<'_> {
        match self {
            Self::Rust(r) => CaptureNames::Rust(r.capture_names()),
            Self::Fancy(r) => CaptureNames::Fancy(r.capture_names()),
            Self::Pcre2(r) => CaptureNames::Pcre2(r.capture_names().iter()),
        }
    }

    pub fn replace(&self, s: &str, replacement: &str) -> String {
        match self {
            Self::Rust(r) => r.replace(s, replacement).to_string(),
            Self::Fancy(r) => r.replace(s, replacement).to_string(),
            Self::Pcre2(r) => replace_once_pcre2(r, s, replacement),
        }
    }

    pub fn replace_all(&self, s: &str, replacement: &str) -> String {
        match self {
            Self::Rust(r) => r.replace_all(s, replacement).to_string(),
            Self::Fancy(r) => r.replace_all(s, replacement).to_string(),
            Self::Pcre2(r) => replace_all_pcre2(r, s, replacement),
        }
    }

    pub fn find_iter_count(&self, s: &str) -> usize {
        match self {
            Self::Rust(r) => r.find_iter(s).count(),
            Self::Fancy(r) => r
                .find_iter(s)
                .filter(|m| m.is_ok())
                .count(),
            Self::Pcre2(r) => r
                .find_iter(s.as_bytes())
                .filter(|m| m.is_ok())
                .count(),
        }
    }

    /// `split` / `split EXPR, STR` — same semantics as [`regex::Regex::split`].
    pub fn split_strings(&self, s: &str) -> Vec<String> {
        match self {
            Self::Rust(r) => r.split(s).map(|x| x.to_string()).collect(),
            Self::Fancy(r) => r
                .split(s)
                .filter_map(|x| x.ok())
                .map(|x| x.to_string())
                .collect(),
            Self::Pcre2(r) => split_pcre2(r, s),
        }
    }

    pub fn splitn_strings(&self, s: &str, limit: usize) -> Vec<String> {
        match self {
            Self::Rust(r) => r.splitn(s, limit).map(|x| x.to_string()).collect(),
            Self::Fancy(r) => r
                .splitn(s, limit)
                .filter_map(|x| x.ok())
                .map(|x| x.to_string())
                .collect(),
            Self::Pcre2(r) => splitn_pcre2(r, s, limit),
        }
    }
}

pub enum CaptureIter<'r, 't> {
    Rust(regex::CaptureMatches<'r, 't>),
    Fancy(fancy_regex::CaptureMatches<'r, 't>),
    Pcre2 {
        hay: &'t str,
        it: pcre2::bytes::CaptureMatches<'r, 't>,
    },
}

impl<'r, 't> Iterator for CaptureIter<'r, 't> {
    type Item = PerlCaptures<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rust(it) => it.next().map(PerlCaptures::Rust),
            Self::Fancy(it) => loop {
                match it.next()? {
                    Ok(c) => return Some(PerlCaptures::Fancy(c)),
                    Err(_) => continue,
                }
            },
            Self::Pcre2 { hay, it } => loop {
                let c = match it.next()? {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                return Some(PerlCaptures::Pcre2 { caps: c, hay });
            },
        }
    }
}

pub enum CaptureNames<'a> {
    Rust(regex::CaptureNames<'a>),
    Fancy(fancy_regex::CaptureNames<'a>),
    Pcre2(std::slice::Iter<'a, Option<String>>),
}

impl<'a> Iterator for CaptureNames<'a> {
    type Item = Option<&'a str>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Rust(it) => it.next(),
            Self::Fancy(it) => it.next(),
            Self::Pcre2(it) => it.next().map(|n| n.as_deref()),
        }
    }
}

/// `$1`… flatten for `@^CAPTURE` / `^CAPTURE_ALL` rows.
pub fn numbered_capture_flat(caps: &PerlCaptures<'_>) -> Vec<PerlValue> {
    let mut cap_flat = Vec::new();
    for i in 1..caps.len() {
        if let Some(m) = caps.get(i) {
            cap_flat.push(PerlValue::string(m.text.to_string()));
        } else {
            cap_flat.push(PerlValue::UNDEF);
        }
    }
    cap_flat
}

fn replace_once_pcre2(re: &Arc<pcre2::bytes::Regex>, s: &str, replacement: &str) -> String {
    let b = s.as_bytes();
    let Ok(Some(caps)) = re.captures(b) else {
        return s.to_string();
    };
    let m0 = match caps.get(0) {
        Some(m) => m,
        None => return s.to_string(),
    };
    let mut out = String::new();
    out.push_str(&s[..m0.start()]);
    out.push_str(&expand_pcre_substitution(&caps, s, replacement));
    out.push_str(&s[m0.end()..]);
    out
}

fn replace_all_pcre2(re: &Arc<pcre2::bytes::Regex>, s: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut last = 0usize;
    let b = s.as_bytes();
    for caps_res in re.captures_iter(b) {
        let caps = match caps_res {
            Ok(c) => c,
            Err(_) => continue,
        };
        let m0 = match caps.get(0) {
            Some(m) => m,
            None => continue,
        };
        out.push_str(&s[last..m0.start()]);
        out.push_str(&expand_pcre_substitution(&caps, s, replacement));
        last = m0.end();
    }
    out.push_str(&s[last..]);
    out
}

/// Expansion aligned with [`regex::Regex::replace`] / `$1`, `$name`, `$&`, `$$`.
fn expand_pcre_substitution(
    caps: &pcre2::bytes::Captures<'_>,
    haystack: &str,
    replacement: &str,
) -> String {
    let mut out = String::with_capacity(replacement.len());
    let mut it = replacement.chars().peekable();
    while let Some(c) = it.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match it.peek() {
            Some('$') => {
                it.next();
                out.push('$');
            }
            Some('&') => {
                it.next();
                if let Some(m) = caps.get(0) {
                    push_match_utf8(&mut out, haystack, m);
                }
            }
            Some('0') => {
                it.next();
                if let Some(m) = caps.get(0) {
                    push_match_utf8(&mut out, haystack, m);
                }
            }
            Some('{') => {
                it.next();
                let mut name = String::new();
                while let Some(&ch) = it.peek() {
                    it.next();
                    if ch == '}' {
                        break;
                    }
                    name.push(ch);
                }
                if let Ok(idx) = name.parse::<usize>() {
                    if let Some(m) = caps.get(idx) {
                        push_match_utf8(&mut out, haystack, m);
                    }
                } else if let Some(m) = caps.name(&name) {
                    push_match_utf8(&mut out, haystack, m);
                }
            }
            Some(ch) if ch.is_ascii_digit() => {
                let d0 = *ch;
                it.next();
                let mut n = (d0 as u8 - b'0') as usize;
                while let Some(&d) = it.peek() {
                    if !d.is_ascii_digit() {
                        break;
                    }
                    it.next();
                    n = n.saturating_mul(10).saturating_add((d as u8 - b'0') as usize);
                }
                if let Some(m) = caps.get(n) {
                    push_match_utf8(&mut out, haystack, m);
                }
            }
            _ => out.push('$'),
        }
    }
    out
}

fn push_match_utf8(out: &mut String, haystack: &str, m: pcre2::bytes::Match<'_>) {
    if let Some(t) = haystack.get(m.start()..m.end()) {
        out.push_str(t);
    }
}

fn split_pcre2(re: &Arc<pcre2::bytes::Regex>, s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut last = 0usize;
    let b = s.as_bytes();
    for m in re.find_iter(b).filter_map(|r| r.ok()) {
        out.push(s[last..m.start()].to_string());
        last = m.end();
    }
    out.push(s[last..].to_string());
    out
}

fn splitn_pcre2(re: &Arc<pcre2::bytes::Regex>, s: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut last = 0usize;
    let b = s.as_bytes();
    let mut n = 0usize;
    for m in re.find_iter(b).filter_map(|r| r.ok()) {
        n += 1;
        if n >= limit {
            break;
        }
        out.push(s[last..m.start()].to_string());
        last = m.end();
    }
    out.push(s[last..].to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_engine_used_for_simple_pattern() {
        let r = PerlCompiledRegex::compile("ab+").unwrap();
        assert!(matches!(*r, PerlCompiledRegex::Rust(_)));
        assert!(r.is_match("xabby"));
    }

    #[test]
    fn fancy_fallback_for_backreference() {
        let r = PerlCompiledRegex::compile(r"(.)\1").unwrap();
        assert!(matches!(*r, PerlCompiledRegex::Fancy(_)));
        assert!(r.is_match("aa"));
        assert!(!r.is_match("ab"));
    }

    #[test]
    fn pcre2_used_when_both_prior_engines_reject() {
        // PCRE2 control verbs are not valid in `regex` or `fancy-regex`.
        let p = "(*SKIP)";
        assert!(regex::Regex::new(p).is_err());
        assert!(fancy_regex::Regex::new(p).is_err());
        let r = PerlCompiledRegex::compile(p).expect("pcre2 compiles (*SKIP)");
        assert!(matches!(*r, PerlCompiledRegex::Pcre2(_)));
    }
}
