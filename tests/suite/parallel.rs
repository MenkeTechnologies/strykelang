use crate::common::*;

#[test]
fn parallel_map() {
    let result = eval("my @a = pmap { $_ * 2 } (1,2,3,4,5); scalar @a");
    assert_eq!(result.to_int(), 5);
}

#[test]
fn parallel_grep() {
    let result = eval("my @a = pgrep { $_ % 2 == 0 } (1,2,3,4,5,6); scalar @a");
    assert_eq!(result.to_int(), 3);
}

#[test]
fn parallel_sort() {
    assert_eq!(
        eval_string(r#"join(",", psort { $a <=> $b } (5,3,1,4,2))"#),
        "1,2,3,4,5"
    );
}

#[test]
fn parallel_sort_default_string_order() {
    assert_eq!(
        eval_string(r#"join(",", psort ("c","a","b"))"#),
        "a,b,c"
    );
}

#[test]
fn parallel_for_runs() {
    assert_eq!(eval_int("pfor { $_ } (1,2,3); 99"), 99);
}
