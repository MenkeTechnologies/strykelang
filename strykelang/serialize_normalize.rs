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
    if let Some(c) = v.as_class_inst() {
        let names = class_field_names(&c.def);
        let values = c.get_values();
        let mut map = IndexMap::new();
        // If the registry didn't resolve some parents, the names vec
        // can be shorter than values. Iterate by min length so we still
        // emit something useful instead of panicking.
        let n = names.len().min(values.len());
        for i in 0..n {
            map.insert(names[i].clone(), deep_normalize(&values[i]));
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(s) = v.as_struct_inst() {
        let values = s.get_values();
        let mut map = IndexMap::new();
        for (i, field) in s.def.fields.iter().enumerate() {
            if let Some(elem) = values.get(i) {
                map.insert(field.name.clone(), deep_normalize(elem));
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
            map.insert("value".to_string(), deep_normalize(&e.data));
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(r) = v.as_hash_ref() {
        let inner = r.read().clone();
        let mut map = IndexMap::new();
        for (k, val) in inner.into_iter() {
            map.insert(k, deep_normalize(&val));
        }
        return StrykeValue::hash_ref(Arc::new(RwLock::new(map)));
    }
    if let Some(r) = v.as_array_ref() {
        let inner = r.read().clone();
        let out: Vec<StrykeValue> = inner.iter().map(deep_normalize).collect();
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
