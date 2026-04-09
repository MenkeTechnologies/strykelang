//! Thread-safe memoization for [`crate::ast::ExprKind::PcacheExpr`].

use std::sync::LazyLock;

use dashmap::DashMap;

use crate::value::PerlValue;

pub static GLOBAL_PCACHE: LazyLock<DashMap<String, PerlValue>> = LazyLock::new(DashMap::new);

pub fn cache_key(v: &PerlValue) -> String {
    v.to_string()
}
