//! Free-function recursive flatten of stryke `ClassInstance` /
//! `StructInstance` values into plain hashref / arrayref trees, for use
//! by serializers (`to_json`, `to_xml`, `to_yaml`, `to_toml`, `to_html`,
//! `ddump`) that take `&[StrykeValue]` and don't have a `&VMHelper` to
//! consult.
//!
//! Inheritance fields (parents declared via `extends`) are looked up
//! through a thread-local `CLASS_DEFS_REGISTRY` that the VM populates
//! on entry to `execute` and on each `ClassDecl` statement. When the
//! registry is empty (e.g. a serializer is called outside a normal VM
//! run), the helper falls back to the class's own field definitions
//! only — covers the no-inheritance case correctly.

use indexmap::IndexMap;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::ClassDef;
use crate::value::StrykeValue;

thread_local! {
    /// Per-thread registry of class definitions, keyed by class name.
    /// VM execution sites snapshot the helper's `class_defs` into this
    /// cell so the free serializers can reach the same MRO information
    /// without taking a `&VMHelper`.
    pub(crate) static CLASS_DEFS_REGISTRY: RefCell<HashMap<String, Arc<ClassDef>>> =
        RefCell::new(HashMap::new());
}

/// Replace this thread's class registry with `defs`. Returns the
/// previous registry so callers can restore it on exit (RAII pattern is
/// preferred — see [`ClassDefsGuard`]).
pub(crate) fn install_class_defs(
    defs: HashMap<String, Arc<ClassDef>>,
) -> HashMap<String, Arc<ClassDef>> {
    CLASS_DEFS_REGISTRY.with(|cell| std::mem::replace(&mut *cell.borrow_mut(), defs))
}

/// Add or update a single class definition in this thread's registry.
/// Used when a `class C { ... }` statement runs at the top level.
pub(crate) fn register_class_def(def: Arc<ClassDef>) {
    CLASS_DEFS_REGISTRY.with(|cell| {
        cell.borrow_mut().insert(def.name.clone(), def);
    });
}

/// Walk a class's full inheritance chain and return field names in MRO
/// order (parent fields first, then own). Mirrors
/// `VMHelper::collect_class_fields_full` but reads from the thread-
/// local registry. Returns an empty vec if the def has parents that
/// aren't registered (e.g. the serializer ran in an isolated context).
fn class_field_names(def: &ClassDef) -> Vec<String> {
    let mut names = Vec::new();
    for parent_name in &def.extends {
        let parent_def_opt =
            CLASS_DEFS_REGISTRY.with(|cell| cell.borrow().get(parent_name).cloned());
        if let Some(parent_def) = parent_def_opt {
            names.extend(class_field_names(&parent_def));
        }
    }
    for f in &def.fields {
        names.push(f.name.clone());
    }
    names
}

/// Recursively convert any `ClassInstance` / `StructInstance` /
/// `EnumInstance` reachable inside `v` into plain hashrefs (using the
/// field name as the key). Hashrefs and arrayrefs are walked in place;
/// every other value (numbers, strings, undef, code refs, blessed
/// non-hash refs, …) round-trips unchanged.
///
/// The intent is "make this value JSON-serializable end-to-end" — call
/// it once at the top of every serializer that doesn't already know
/// about stryke-native OO instances.
pub fn deep_normalize(v: &StrykeValue) -> StrykeValue {
    let mut visited: std::collections::HashSet<usize> = std::collections::HashSet::new();
    deep_normalize_inner(v, &mut visited)
}

fn deep_normalize_inner(
    v: &StrykeValue,
    visited: &mut std::collections::HashSet<usize>,
) -> StrykeValue {
    if let Some(c) = v.as_class_inst() {
        let names = class_field_names(&c.def);
        let values = c.get_values();
        let mut map = IndexMap::new();
        // If the registry didn't resolve some parents, the names vec
        // can be shorter than values. Iterate by min length so we still
        // emit something useful instead of panicking.
        let n = names.len().min(values.len());
        for i in 0..n {
            map.insert(names[i].clone(), deep_normalize_inner(&values[i], visited));
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(s) = v.as_struct_inst() {
        let values = s.get_values();
        let mut map = IndexMap::new();
        for (i, field) in s.def.fields.iter().enumerate() {
            if let Some(elem) = values.get(i) {
                map.insert(field.name.clone(), deep_normalize_inner(elem, visited));
            }
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(e) = v.as_enum_inst() {
        // Enum: emit `{ variant => "Name", value => recursive(payload) }`
        // when there's a payload; otherwise `{ variant => "Name" }`.
        // Lets serializers preserve enum identity instead of stringifying.
        let mut map = IndexMap::new();
        map.insert(
            "variant".to_string(),
            StrykeValue::string(e.variant_name().to_string()),
        );
        if !e.data.is_undef() {
            map.insert("value".to_string(), deep_normalize_inner(&e.data, visited));
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(r) = v.as_hash_ref() {
        // Cycle guard: pass back-edges through as UNDEF so serializers can
        // handle them (BUG-105 — was a stack overflow on self-referential
        // hashes/arrays).
        let addr = Arc::as_ptr(&r) as usize;
        if !visited.insert(addr) {
            return StrykeValue::UNDEF;
        }
        let inner = r.read().clone();
        let mut map = IndexMap::new();
        for (k, val) in inner.into_iter() {
            map.insert(k, deep_normalize_inner(&val, visited));
        }
        visited.remove(&addr);
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(r) = v.as_array_ref() {
        let addr = Arc::as_ptr(&r) as usize;
        if !visited.insert(addr) {
            return StrykeValue::UNDEF;
        }
        let inner = r.read().clone();
        let out: Vec<StrykeValue> = inner
            .iter()
            .map(|elem| deep_normalize_inner(elem, visited))
            .collect();
        visited.remove(&addr);
        return StrykeValue::array_ref(Arc::new(RwLock::new(out)));
    }
    v.clone()
}

/// Convenience: normalize the first arg in place and return a Vec the
/// caller can hand to its existing serializer logic. Use when the
/// serializer takes `&[StrykeValue]` and only the first element is the
/// data to serialize.
pub fn normalize_args_head(args: &[StrykeValue]) -> Vec<StrykeValue> {
    if args.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(args.len());
    out.push(deep_normalize(&args[0]));
    out.extend(args[1..].iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn aref(v: Vec<StrykeValue>) -> StrykeValue {
        StrykeValue::array_ref(Arc::new(RwLock::new(v)))
    }

    fn href(pairs: &[(&str, StrykeValue)]) -> StrykeValue {
        let mut m = IndexMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), v.clone());
        }
        StrykeValue::hash_ref(Arc::new(RwLock::new(m)))
    }

    #[test]
    fn deep_normalize_scalars_roundtrip_unchanged() {
        for v in [
            StrykeValue::integer(42),
            StrykeValue::float(3.5),
            StrykeValue::string("hi".into()),
            StrykeValue::UNDEF,
        ] {
            let out = deep_normalize(&v);
            assert_eq!(out.to_string(), v.to_string());
        }
    }

    #[test]
    fn deep_normalize_walks_nested_arrayref_hashref() {
        let inner = href(&[("x", StrykeValue::integer(1))]);
        let outer = aref(vec![inner, StrykeValue::integer(2)]);
        let out = deep_normalize(&outer);
        let arr = out.as_array_ref().expect("array_ref outer survives");
        let arr = arr.read();
        assert_eq!(arr.len(), 2);
        let h = arr[0].as_hash_ref().expect("nested hash_ref survives");
        let h = h.read();
        assert_eq!(h.get("x").unwrap().to_int(), 1);
        assert_eq!(arr[1].to_int(), 2);
    }

    #[test]
    fn deep_normalize_does_not_share_storage_with_input() {
        let arr = Arc::new(RwLock::new(vec![StrykeValue::integer(1)]));
        let v = StrykeValue::array_ref(arr.clone());
        let out = deep_normalize(&v);
        let out_arr = out.as_array_ref().expect("array_ref");
        out_arr.write().push(StrykeValue::integer(2));
        assert_eq!(
            arr.read().len(),
            1,
            "deep_normalize must clone, not alias, ref storage"
        );
    }

    #[test]
    fn normalize_args_head_normalizes_first_only() {
        let h = href(&[("k", StrykeValue::integer(7))]);
        let tail = StrykeValue::string("opt".into());
        let out = normalize_args_head(&[h, tail.clone()]);
        assert_eq!(out.len(), 2);
        assert!(out[0].as_hash_ref().is_some());
        assert_eq!(out[1].to_string(), tail.to_string());
    }

    #[test]
    fn normalize_args_head_empty_returns_empty() {
        assert!(normalize_args_head(&[]).is_empty());
    }

    #[test]
    fn register_class_def_appears_in_field_names() {
        use crate::ast::ClassDef;
        let prev = install_class_defs(HashMap::new());
        let def = Arc::new(ClassDef {
            name: "T".into(),
            is_abstract: false,
            is_final: false,
            extends: vec![],
            implements: vec![],
            fields: vec![],
            methods: vec![],
            static_fields: vec![],
        });
        register_class_def(def);
        let names = CLASS_DEFS_REGISTRY.with(|c| c.borrow().keys().cloned().collect::<Vec<_>>());
        assert!(names.contains(&"T".to_string()));
        // restore to keep registry isolated across tests
        install_class_defs(prev);
    }
}
