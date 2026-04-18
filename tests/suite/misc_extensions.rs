use crate::common::*;

#[test]
fn misc_boolean_combinators() {
    assert_eq!(eval_int("both(1, 1)"), 1);
    assert_eq!(eval_int("both(1, 0)"), 0);
    assert_eq!(eval_int("both(1, 1, 1)"), 0); // only 2 args allowed for 'both'

    assert_eq!(eval_int("either(1, 0)"), 1);
    assert_eq!(eval_int("either(0, 0)"), 0);
    assert_eq!(eval_int("either(0, 1, 0)"), 1);

    assert_eq!(eval_int("neither(0, 0)"), 1);
    assert_eq!(eval_int("neither(1, 0)"), 0);

    assert_eq!(eval_int("xor_bool(1, 0)"), 1);
    assert_eq!(eval_int("xor_bool(1, 1)"), 0);
    assert_eq!(eval_int("xor_bool(0, 0)"), 0);

    assert_eq!(eval_int("bool_to_int(1)"), 1);
    assert_eq!(eval_int("bool_to_int(0)"), 0);
    assert_eq!(eval_int("b2i(42)"), 1); // 42 is true
}

#[test]
fn misc_collection_helpers() {
    assert_eq!(eval_string("join ',', riffle([1, 2], [3, 4])"), "1,3,2,4");
    assert_eq!(
        eval_string("join ',', intersperse([1, 2, 3], 0)"),
        "1,0,2,0,3"
    );
    assert_eq!(
        eval_string("join ',', every_nth([1, 2, 3, 4, 5, 6], 2)"),
        "1,3,5"
    );

    assert_eq!(eval_string("join ',', drop_n([1, 2, 3, 4], 2)"), "3,4");
    assert_eq!(eval_string("join ',', take_n([1, 2, 3, 4], 2)"), "1,2");

    assert_eq!(eval_string("join ',', rotate([1, 2, 3], 1)"), "2,3,1");
    assert_eq!(eval_string("join ',', swap_pairs([1, 2, 3, 4])"), "2,1,4,3");
}
