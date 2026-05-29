//! Thread-safe memoization for [`crate::ast::ExprKind::PcacheExpr`].

use std::sync::LazyLock;

use dashmap::DashMap;

use crate::value::StrykeValue;
/// `GLOBAL_PCACHE` static.
pub static GLOBAL_PCACHE: LazyLock<DashMap<String, StrykeValue>> = LazyLock::new(DashMap::new);
/// `cache_key` — see implementation.
pub fn cache_key(v: &StrykeValue) -> String {
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::cache_key;
    use crate::value::StrykeValue;

    #[test]
    fn cache_key_matches_display() {
        let v = StrykeValue::integer(42);
        assert_eq!(cache_key(&v), v.to_string());
    }

    #[test]
    fn cache_key_equal_values_produce_equal_keys() {
        let a = StrykeValue::string("foo".to_string());
        let b = StrykeValue::string("foo".to_string());
        assert_eq!(cache_key(&a), cache_key(&b));
    }

    #[test]
    fn cache_key_undef() {
        let u = StrykeValue::UNDEF;
        assert_eq!(cache_key(&u), u.to_string());
    }
}
