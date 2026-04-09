use indexmap::IndexMap;
use parking_lot::RwLock;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use crate::ast::Block;

/// Core Perl value type. Clone-cheap via Arc for references.
#[derive(Debug, Clone)]
pub enum PerlValue {
    Undef,
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<PerlValue>),
    Hash(IndexMap<String, PerlValue>),
    ArrayRef(Arc<RwLock<Vec<PerlValue>>>),
    HashRef(Arc<RwLock<IndexMap<String, PerlValue>>>),
    ScalarRef(Arc<RwLock<PerlValue>>),
    CodeRef(Arc<PerlSub>),
    Regex(Arc<regex::Regex>, String),
    Blessed(Arc<BlessedRef>),
    /// File handle (wraps an index into the interpreter's handle table)
    IOHandle(String),
}

#[derive(Debug, Clone)]
pub struct PerlSub {
    pub name: String,
    pub params: Vec<String>,
    pub body: Block,
    /// Captured lexical scope (for closures)
    pub closure_env: Option<Vec<(String, PerlValue)>>,
}

#[derive(Debug)]
pub struct BlessedRef {
    pub class: String,
    pub data: RwLock<PerlValue>,
}

impl Clone for BlessedRef {
    fn clone(&self) -> Self {
        Self {
            class: self.class.clone(),
            data: RwLock::new(self.data.read().clone()),
        }
    }
}

impl Default for PerlValue {
    fn default() -> Self {
        PerlValue::Undef
    }
}

impl PerlValue {
    // ── Truthiness (Perl rules) ──

    pub fn is_true(&self) -> bool {
        match self {
            PerlValue::Undef => false,
            PerlValue::Integer(n) => *n != 0,
            PerlValue::Float(f) => *f != 0.0,
            PerlValue::String(s) => !s.is_empty() && s != "0",
            PerlValue::Array(a) => !a.is_empty(),
            PerlValue::Hash(h) => !h.is_empty(),
            _ => true,
        }
    }

    // ── Numeric coercion ──

    pub fn to_number(&self) -> f64 {
        match self {
            PerlValue::Undef => 0.0,
            PerlValue::Integer(n) => *n as f64,
            PerlValue::Float(f) => *f,
            PerlValue::String(s) => parse_number(s),
            PerlValue::Array(a) => a.len() as f64,
            _ => 0.0,
        }
    }

    pub fn to_int(&self) -> i64 {
        match self {
            PerlValue::Undef => 0,
            PerlValue::Integer(n) => *n,
            PerlValue::Float(f) => *f as i64,
            PerlValue::String(s) => parse_number(s) as i64,
            PerlValue::Array(a) => a.len() as i64,
            _ => 0,
        }
    }

    // ── String coercion ──

    pub fn to_string(&self) -> String {
        match self {
            PerlValue::Undef => String::new(),
            PerlValue::Integer(n) => n.to_string(),
            PerlValue::Float(f) => format_float(*f),
            PerlValue::String(s) => s.clone(),
            PerlValue::Array(a) => a.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(""),
            PerlValue::Hash(h) => format!("{}/{}", h.len(), h.capacity()),
            PerlValue::ArrayRef(_) => "ARRAY(0x...)".to_string(),
            PerlValue::HashRef(_) => "HASH(0x...)".to_string(),
            PerlValue::ScalarRef(_) => "SCALAR(0x...)".to_string(),
            PerlValue::CodeRef(sub) => format!("CODE({})", sub.name),
            PerlValue::Regex(_, src) => format!("(?:{})", src),
            PerlValue::Blessed(b) => format!("{}=HASH(0x...)", b.class),
            PerlValue::IOHandle(name) => name.clone(),
        }
    }

    // ── Type checks ──

    pub fn type_name(&self) -> &str {
        match self {
            PerlValue::Undef => "undef",
            PerlValue::Integer(_) => "INTEGER",
            PerlValue::Float(_) => "FLOAT",
            PerlValue::String(_) => "STRING",
            PerlValue::Array(_) => "ARRAY",
            PerlValue::Hash(_) => "HASH",
            PerlValue::ArrayRef(_) => "ARRAY",
            PerlValue::HashRef(_) => "HASH",
            PerlValue::ScalarRef(_) => "SCALAR",
            PerlValue::CodeRef(_) => "CODE",
            PerlValue::Regex(_, _) => "Regexp",
            PerlValue::Blessed(b) => &b.class,
            PerlValue::IOHandle(_) => "GLOB",
        }
    }

    pub fn ref_type(&self) -> PerlValue {
        match self {
            PerlValue::ArrayRef(_) => PerlValue::String("ARRAY".into()),
            PerlValue::HashRef(_) => PerlValue::String("HASH".into()),
            PerlValue::ScalarRef(_) => PerlValue::String("SCALAR".into()),
            PerlValue::CodeRef(_) => PerlValue::String("CODE".into()),
            PerlValue::Regex(_, _) => PerlValue::String("Regexp".into()),
            PerlValue::Blessed(b) => PerlValue::String(b.class.clone()),
            _ => PerlValue::String(String::new()),
        }
    }

    // ── Comparison ──

    pub fn num_cmp(&self, other: &PerlValue) -> Ordering {
        let a = self.to_number();
        let b = other.to_number();
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }

    pub fn str_cmp(&self, other: &PerlValue) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }

    /// Return the value as a list (flatten arrays, hash to kv pairs).
    pub fn to_list(&self) -> Vec<PerlValue> {
        match self {
            PerlValue::Array(a) => a.clone(),
            PerlValue::Hash(h) => h
                .iter()
                .flat_map(|(k, v)| vec![PerlValue::String(k.clone()), v.clone()])
                .collect(),
            PerlValue::Undef => vec![],
            other => vec![other.clone()],
        }
    }

    /// Scalar context: arrays → length, hashes → "n/m" string.
    pub fn scalar_context(&self) -> PerlValue {
        match self {
            PerlValue::Array(a) => PerlValue::Integer(a.len() as i64),
            PerlValue::Hash(h) => {
                if h.is_empty() {
                    PerlValue::Integer(0)
                } else {
                    PerlValue::String(format!("{}/{}", h.len(), h.capacity()))
                }
            }
            other => other.clone(),
        }
    }
}

impl fmt::Display for PerlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

// ── Helpers ──

fn parse_number(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }
    // Perl extracts leading numeric portion
    let mut end = 0;
    let bytes = s.as_bytes();
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end < bytes.len() && bytes[end] == b'.' {
        end += 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        end += 1;
        if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
            end += 1;
        }
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
    }
    if end == 0 {
        return 0.0;
    }
    s[..end].parse::<f64>().unwrap_or(0.0)
}

fn format_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{}", f as i64)
    } else {
        // Perl uses %g-like formatting
        let s = format!("{}", f);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::PerlValue;
    use indexmap::IndexMap;
    use std::cmp::Ordering;

    #[test]
    fn undef_is_false() {
        assert!(!PerlValue::Undef.is_true());
    }

    #[test]
    fn string_zero_is_false() {
        assert!(!PerlValue::String("0".into()).is_true());
        assert!(PerlValue::String("00".into()).is_true());
    }

    #[test]
    fn empty_string_is_false() {
        assert!(!PerlValue::String(String::new()).is_true());
    }

    #[test]
    fn integer_zero_is_false_nonzero_true() {
        assert!(!PerlValue::Integer(0).is_true());
        assert!(PerlValue::Integer(-1).is_true());
    }

    #[test]
    fn to_int_parses_leading_number_from_string() {
        assert_eq!(PerlValue::String("42xyz".into()).to_int(), 42);
        assert_eq!(PerlValue::String("  -3.7foo".into()).to_int(), -3);
    }

    #[test]
    fn num_cmp_orders_as_numeric() {
        assert_eq!(
            PerlValue::Integer(2).num_cmp(&PerlValue::Integer(11)),
            Ordering::Less
        );
        assert_eq!(
            PerlValue::String("2foo".into()).num_cmp(&PerlValue::String("11".into())),
            Ordering::Less
        );
    }

    #[test]
    fn str_cmp_orders_as_strings() {
        assert_eq!(
            PerlValue::String("2".into()).str_cmp(&PerlValue::String("11".into())),
            Ordering::Greater
        );
    }

    #[test]
    fn scalar_context_array_and_hash() {
        assert_eq!(
            PerlValue::Array(vec![PerlValue::Integer(1), PerlValue::Integer(2)]).scalar_context(),
            PerlValue::Integer(2)
        );
        let mut h = IndexMap::new();
        h.insert("a".into(), PerlValue::Integer(1));
        let sc = PerlValue::Hash(h).scalar_context();
        assert!(matches!(sc, PerlValue::String(_)));
    }

    #[test]
    fn to_list_array_hash_and_scalar() {
        assert_eq!(
            PerlValue::Array(vec![PerlValue::Integer(7)]).to_list().len(),
            1
        );
        let mut h = IndexMap::new();
        h.insert("k".into(), PerlValue::Integer(1));
        let list = PerlValue::Hash(h).to_list();
        assert_eq!(list.len(), 2);
        assert_eq!(PerlValue::Integer(99).to_list(), vec![PerlValue::Integer(99)]);
    }
}
