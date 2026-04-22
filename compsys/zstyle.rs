//! zstyle builtin implementation
//!
//! zstyle is the configuration system for zsh completion.
//! Patterns are matched against context strings like ':completion:*:*:*:*:*'

use std::collections::HashMap;

/// A single style definition
#[derive(Clone, Debug)]
pub struct ZStyle {
    /// Pattern to match context (e.g., ':completion:*:descriptions')
    pub pattern: String,
    /// Style name (e.g., 'format', 'menu', 'list-colors')
    pub name: String,
    /// Values for this style
    pub values: Vec<String>,
    /// Compiled pattern weight for matching precedence
    /// Higher weight = more specific = matched first
    pub weight: u64,
    /// Whether values should be evaluated (zstyle -e)
    pub eval: bool,
}

impl ZStyle {
    pub fn new(pattern: impl Into<String>, name: impl Into<String>, values: Vec<String>) -> Self {
        let pattern = pattern.into();
        let weight = calculate_weight(&pattern);
        Self {
            pattern,
            name: name.into(),
            values,
            weight,
            eval: false,
        }
    }

    pub fn with_eval(mut self) -> Self {
        self.eval = true;
        self
    }

    /// Get single value (first element)
    pub fn value(&self) -> Option<&str> {
        self.values.first().map(|s| s.as_str())
    }

    /// Get value as boolean
    pub fn as_bool(&self) -> Option<bool> {
        self.value()
            .map(|v| matches!(v.to_lowercase().as_str(), "yes" | "true" | "on" | "1"))
    }

    /// Get value as integer
    pub fn as_int(&self) -> Option<i64> {
        self.value().and_then(|v| v.parse().ok())
    }
}

/// Calculate pattern weight for matching precedence
/// Based on zsh's algorithm:
/// - More components = higher base weight
/// - Literal string component = 2 points
/// - Pattern component = 1 point
/// - Just '*' component = 0 points
fn calculate_weight(pattern: &str) -> u64 {
    let components: Vec<&str> = pattern.split(':').collect();
    let num_components = components.len() as u64;

    let mut specificity: u64 = 0;
    for comp in &components {
        if *comp == "*" || comp.is_empty() {
            // Just wildcard or empty = 0 points
        } else if comp.contains('*') || comp.contains('?') || comp.contains('[') {
            // Pattern = 1 point
            specificity += 1;
        } else {
            // Literal = 2 points
            specificity += 2;
        }
    }

    // Combine: high bits = num components, low bits = specificity
    (num_components << 32) | specificity
}

/// Storage for all styles
#[derive(Clone, Debug, Default)]
pub struct ZStyleStore {
    /// Styles grouped by name for faster lookup
    styles: HashMap<String, Vec<ZStyle>>,
}

impl ZStyleStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a style
    pub fn set(&mut self, pattern: &str, name: &str, values: Vec<String>, eval: bool) {
        let style = if eval {
            ZStyle::new(pattern, name, values).with_eval()
        } else {
            ZStyle::new(pattern, name, values)
        };

        let entries = self.styles.entry(name.to_string()).or_default();

        // Remove existing entry with same pattern
        entries.retain(|s| s.pattern != pattern);

        // Insert and sort by weight descending
        entries.push(style);
        entries.sort_by(|a, b| b.weight.cmp(&a.weight));
    }

    /// Delete a style
    pub fn delete(&mut self, pattern: &str, name: Option<&str>) {
        if let Some(name) = name {
            if let Some(entries) = self.styles.get_mut(name) {
                entries.retain(|s| s.pattern != pattern);
                if entries.is_empty() {
                    self.styles.remove(name);
                }
            }
        } else {
            // Delete all styles with this pattern
            for entries in self.styles.values_mut() {
                entries.retain(|s| s.pattern != pattern);
            }
            self.styles.retain(|_, v| !v.is_empty());
        }
    }

    /// Lookup a style value for a context
    pub fn lookup(&self, context: &str, name: &str) -> Option<&ZStyle> {
        self.styles.get(name).and_then(|entries| {
            entries
                .iter()
                .find(|s| pattern_matches(&s.pattern, context))
        })
    }

    /// Lookup and return values
    pub fn lookup_values(&self, context: &str, name: &str) -> Option<&[String]> {
        self.lookup(context, name).map(|s| s.values.as_slice())
    }

    /// Lookup as single string value
    pub fn lookup_str(&self, context: &str, name: &str) -> Option<&str> {
        self.lookup(context, name).and_then(|s| s.value())
    }

    /// Lookup as boolean
    pub fn lookup_bool(&self, context: &str, name: &str) -> Option<bool> {
        self.lookup(context, name).and_then(|s| s.as_bool())
    }

    /// Test if a style exists for context (zstyle -t)
    pub fn test(&self, context: &str, name: &str, patterns: &[String]) -> bool {
        if let Some(style) = self.lookup(context, name) {
            if patterns.is_empty() {
                // Just test existence
                true
            } else {
                // Test if any value matches any pattern
                for val in &style.values {
                    for pat in patterns {
                        if pattern_matches(pat, val) {
                            return true;
                        }
                    }
                }
                false
            }
        } else {
            false
        }
    }

    /// Get all style names
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.styles.keys().map(|s| s.as_str())
    }

    /// Get all patterns for a style
    pub fn patterns(&self, name: &str) -> Vec<&str> {
        self.styles
            .get(name)
            .map(|v| v.iter().map(|s| s.pattern.as_str()).collect())
            .unwrap_or_default()
    }

    /// Get all unique patterns
    pub fn all_patterns(&self) -> Vec<&str> {
        let mut patterns: Vec<&str> = self
            .styles
            .values()
            .flat_map(|v| v.iter().map(|s| s.pattern.as_str()))
            .collect();
        patterns.sort();
        patterns.dedup();
        patterns
    }

    /// Print all styles (for zstyle with no args or -L)
    pub fn print(&self, syntax: bool) -> Vec<String> {
        let mut output = Vec::new();

        // Collect all styles sorted
        let mut all: Vec<&ZStyle> = self.styles.values().flatten().collect();
        all.sort_by(|a, b| a.pattern.cmp(&b.pattern).then_with(|| a.name.cmp(&b.name)));

        for style in all {
            if syntax {
                let vals: Vec<String> = style.values.iter().map(|v| shell_quote(v)).collect();
                let eval_flag = if style.eval { "-e " } else { "" };
                output.push(format!(
                    "zstyle {}{} {} {}",
                    eval_flag,
                    shell_quote(&style.pattern),
                    shell_quote(&style.name),
                    vals.join(" ")
                ));
            } else {
                let vals: Vec<String> = style.values.iter().map(|v| shell_quote(v)).collect();
                let eval_mark = if style.eval { "(eval) " } else { "       " };
                output.push(format!(
                    "{}{}  {}",
                    eval_mark,
                    style.pattern,
                    vals.join(" ")
                ));
            }
        }

        output
    }
}

/// Match a pattern against a context string
/// Patterns use : as separator and * as wildcard
fn pattern_matches(pattern: &str, context: &str) -> bool {
    let pat_parts: Vec<&str> = pattern.split(':').collect();
    let ctx_parts: Vec<&str> = context.split(':').collect();

    pattern_match_parts(&pat_parts, &ctx_parts)
}

fn pattern_match_parts(pattern: &[&str], context: &[&str]) -> bool {
    let mut pi = 0;
    let mut ci = 0;

    while pi < pattern.len() && ci < context.len() {
        let pat = pattern[pi];
        let ctx = context[ci];

        if pat == "*" {
            // Wildcard matches any single component
            pi += 1;
            ci += 1;
        } else if glob_match_simple(ctx, pat) {
            pi += 1;
            ci += 1;
        } else {
            return false;
        }
    }

    // All pattern parts must be consumed
    // Context can have extra parts
    pi == pattern.len()
}

/// Simple glob matching for a single component
fn glob_match_simple(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return text == pattern;
    }

    let text_chars: Vec<char> = text.chars().collect();
    let pat_chars: Vec<char> = pattern.chars().collect();
    glob_match_impl(&text_chars, &pat_chars)
}

fn glob_match_impl(text: &[char], pattern: &[char]) -> bool {
    let mut ti = 0;
    let mut pi = 0;
    let mut star_pi = None;
    let mut star_ti = 0;

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == '?' || pattern[pi] == text[ti]) {
            ti += 1;
            pi += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }

    pi == pattern.len()
}

/// Quote a string for shell output
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    // Check if quoting needed
    let needs_quote = s.chars().any(|c| {
        matches!(
            c,
            ' ' | '\t'
                | '\n'
                | '\''
                | '"'
                | '\\'
                | '$'
                | '`'
                | '!'
                | '*'
                | '?'
                | '['
                | ']'
                | '('
                | ')'
                | '{'
                | '}'
                | '<'
                | '>'
                | '|'
                | '&'
                | ';'
        )
    });

    if !needs_quote {
        return s.to_string();
    }

    // Use single quotes, escaping any single quotes
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

/// Trait for easy style lookup
pub trait ZStyleLookup {
    fn lookup_style(&self, context: &str, name: &str) -> Option<&ZStyle>;
    fn lookup_style_str(&self, context: &str, name: &str) -> Option<&str>;
    fn lookup_style_bool(&self, context: &str, name: &str) -> Option<bool>;
    fn lookup_style_values(&self, context: &str, name: &str) -> Option<&[String]>;
}

impl ZStyleLookup for ZStyleStore {
    fn lookup_style(&self, context: &str, name: &str) -> Option<&ZStyle> {
        self.lookup(context, name)
    }

    fn lookup_style_str(&self, context: &str, name: &str) -> Option<&str> {
        self.lookup_str(context, name)
    }

    fn lookup_style_bool(&self, context: &str, name: &str) -> Option<bool> {
        self.lookup_bool(context, name)
    }

    fn lookup_style_values(&self, context: &str, name: &str) -> Option<&[String]> {
        self.lookup_values(context, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_calculation() {
        // More specific patterns should have higher weight
        let w1 = calculate_weight(":completion:*");
        let w2 = calculate_weight(":completion:*:descriptions");
        let w3 = calculate_weight(":completion:*:*:*:*:descriptions");

        assert!(w2 > w1, "more components = higher weight");
        assert!(w3 > w2, "even more components = even higher weight");

        let wa = calculate_weight(":completion:*:default");
        let wb = calculate_weight(":completion:*:*");
        assert!(wa > wb, "literal > wildcard");
    }

    #[test]
    fn test_pattern_matching() {
        assert!(pattern_matches(":completion:*", ":completion:foo"));
        assert!(pattern_matches(":completion:*:*", ":completion:foo:bar"));
        assert!(pattern_matches(
            ":completion:*:descriptions",
            ":completion:foo:descriptions"
        ));
        assert!(!pattern_matches(
            ":completion:*:descriptions",
            ":completion:foo:messages"
        ));
    }

    #[test]
    fn test_style_store() {
        let mut store = ZStyleStore::new();

        store.set(":completion:*", "menu", vec!["select".to_string()], false);
        store.set(
            ":completion:*:descriptions",
            "format",
            vec!["%d".to_string()],
            false,
        );

        assert_eq!(
            store.lookup_str(":completion:anything", "menu"),
            Some("select")
        );
        assert_eq!(
            store.lookup_str(":completion:foo:descriptions", "format"),
            Some("%d")
        );
        assert_eq!(store.lookup_str(":completion:foo:messages", "format"), None);
    }

    #[test]
    fn test_specificity() {
        let mut store = ZStyleStore::new();

        // Less specific
        store.set(":completion:*", "menu", vec!["no".to_string()], false);
        // More specific
        store.set(
            ":completion:*:*:*:default",
            "menu",
            vec!["yes".to_string()],
            false,
        );

        // More specific should win
        assert_eq!(
            store.lookup_str(":completion:foo:bar:baz:default", "menu"),
            Some("yes")
        );
        // Falls back to less specific
        assert_eq!(store.lookup_str(":completion:foo", "menu"), Some("no"));
    }

    #[test]
    fn test_shell_quote() {
        assert_eq!(shell_quote("simple"), "simple");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
    }
}
