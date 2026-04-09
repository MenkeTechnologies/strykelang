//! Line comments (`#`), `qw()`, and `printf` return value.

use crate::common::*;

#[test]
fn addition_with_line_comment_between_operands() {
    assert_eq!(eval_int("1 +\n#ignored\n2"), 3);
}

#[test]
fn qw_assigns_word_list_to_array() {
    assert_eq!(eval_int("my @a = qw(x y z); scalar @a"), 3);
}

#[test]
fn printf_returns_success_scalar() {
    // Perl `printf` returns true (1) on success, not byte count.
    assert_eq!(eval_int(r#"my $n = printf "%d", 42; $n"#), 1);
}
