//! Extra tests for `PerlValue` to ensure correct Perl-like semantics.

use crate::value::PerlValue;
use parking_lot::RwLock;
use std::sync::Arc;

#[test]
fn test_perl_value_truthiness() {
    // Basics
    assert!(!PerlValue::UNDEF.is_true());
    assert!(PerlValue::integer(1).is_true());
    assert!(!PerlValue::integer(0).is_true());
    assert!(PerlValue::integer(-1).is_true());

    // Strings
    assert!(PerlValue::string("true".into()).is_true());
    assert!(PerlValue::string("1".into()).is_true());
    assert!(!PerlValue::string("0".into()).is_true());
    assert!(!PerlValue::string("".into()).is_true());
    // Perl quirk: "00" is true, but "0" is false.
    assert!(PerlValue::string("00".into()).is_true());
    assert!(PerlValue::string("0.0".into()).is_true());

    // Floats
    assert!(PerlValue::float(1.0).is_true());
    assert!(PerlValue::float(0.1).is_true());
    assert!(!PerlValue::float(0.0).is_true());
    assert!(!PerlValue::float(-0.0).is_true());
}

#[test]
fn test_numeric_conversions() {
    // String to Int
    assert_eq!(PerlValue::string("42".into()).to_int(), 42);
    assert_eq!(PerlValue::string("42.5".into()).to_int(), 42);
    assert_eq!(PerlValue::string("  42  ".into()).to_int(), 42);
    assert_eq!(PerlValue::string("42abc".into()).to_int(), 42);
    assert_eq!(PerlValue::string("abc42".into()).to_int(), 0);

    // Float to Int
    assert_eq!(PerlValue::float(42.9).to_int(), 42);
    assert_eq!(PerlValue::float(-42.9).to_int(), -42);

    // Undef to Int
    assert_eq!(PerlValue::UNDEF.to_int(), 0);
}

#[test]
fn test_string_conversions() {
    assert_eq!(PerlValue::integer(42).to_string(), "42");
    assert_eq!(PerlValue::float(42.5).to_string(), "42.5");
    assert_eq!(PerlValue::UNDEF.to_string(), "");

    // Float that looks like an integer
    assert_eq!(PerlValue::float(42.0).to_string(), "42");
}

#[test]
fn test_cloning_semantics() {
    // Array (Value type) should deep copy on clone
    let v1 = PerlValue::array(vec![PerlValue::integer(10)]);
    let _v2 = v1.clone();

    if let Some(mut arr) = v1.as_array_vec() {
        arr[0] = PerlValue::integer(20);
        assert_eq!(arr[0].to_int(), 20);
    }

    // v1's internal value should still be 10 if we get it again
    if let Some(arr) = v1.as_array_vec() {
        assert_eq!(arr[0].to_int(), 10);
    }
}

#[test]
fn test_comparison_logic() {
    let ten = PerlValue::integer(10);
    let two_str = PerlValue::string("2".into());

    // Numeric comparison (ten > two_str)
    assert!(ten.to_number() > two_str.to_number());

    // String comparison ("10" < "2")
    assert!(ten.to_string() < two_str.to_string());
}

#[test]
fn test_deep_copy_on_clone() {
    // Verify that clone() on a HeapObject::Array produces a NEW Arc with a cloned Vec.
    let arr = vec![PerlValue::integer(1)];
    let v1 = PerlValue::array(arr);
    let v2 = v1.clone();

    // The NaN-boxed bit patterns (pointers) should be different because of the deep copy.
    assert_ne!(v1.0, v2.0);

    // But they should have the same content
    assert_eq!(v1.to_string(), v2.to_string());
}

#[test]
fn test_shallow_clone_shares_ptr() {
    let v1 = PerlValue::array(vec![PerlValue::integer(1)]);
    let v2 = v1.shallow_clone();

    assert_eq!(v1.0, v2.0); // Should be exactly the same pointer
}

#[test]
fn test_array_ref_shares_on_clone() {
    // ArrayRef should NOT deep copy on clone because it's a reference type.
    let shared_vec = Arc::new(RwLock::new(vec![PerlValue::integer(1)]));
    let v1 = PerlValue::array_ref(shared_vec);
    let v2 = v1.clone();

    assert_eq!(v1.0, v2.0); // Should be same pointer
}

#[test]
fn test_nested_cloning() {
    // Array containing another Array
    let inner = PerlValue::array(vec![PerlValue::integer(1)]);
    let outer = PerlValue::array(vec![inner]);

    let cloned = outer.clone();
    assert_ne!(outer.0, cloned.0);

    let outer_arr = outer.as_array_vec().unwrap();
    let cloned_arr = cloned.as_array_vec().unwrap();

    // The inner arrays should also have been deep copied
    assert_ne!(outer_arr[0].0, cloned_arr[0].0);
}

#[test]
fn test_hash_cloning() {
    let mut map = indexmap::IndexMap::new();
    map.insert("key".to_string(), PerlValue::integer(1));
    let h1 = PerlValue::hash(map);
    let h2 = h1.clone();

    assert_ne!(h1.0, h2.0); // Deep copy

    if let Some(m1) = h1.as_hash_map() {
        assert_eq!(m1.get("key").unwrap().to_int(), 1);
    }
}
