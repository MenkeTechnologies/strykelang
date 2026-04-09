//! Anonymous subs and lexical capture.

use crate::common::*;

#[test]
fn anon_sub_captures_outer_lexical() {
    assert_eq!(
        eval_int(
            "my $x = 10; \
             my $c = sub { $x + 5 }; \
             $c->()",
        ),
        15
    );
}
