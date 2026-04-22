//! Thread-safe memoization for [`crate::ast::ExprKind::PcacheExpr`].

use std::sync::LazyLock;

use dashmap::DashMap;

use crate::value::PerlValue;

pub static GLOBAL_PCACHE: LazyLock<DashMap<String, PerlValue>> = LazyLock::new(DashMap::new);

pub fn cache_key(v: &PerlValue) -> String {
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::cache_key;
    use crate::value::PerlValue;

    #[test]
    fn cache_key_matches_display() {
        let v = PerlValue::integer(42);
        assert_eq!(cache_key(&v), v.to_string());
    }

    #[test]
    fn cache_key_equal_values_produce_equal_keys() {
        let a = PerlValue::string("foo".to_string());
        let b = PerlValue::string("foo".to_string());
        assert_eq!(cache_key(&a), cache_key(&b));
    }

    #[test]
    fn cache_key_undef() {
        let u = PerlValue::UNDEF;
        assert_eq!(cache_key(&u), u.to_string());
    }
}
