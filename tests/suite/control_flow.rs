use crate::common::*;

#[test]
fn if_else() {
    assert_eq!(eval_int("my $x = 10; if ($x > 5) { 1 } else { 0 }"), 1);
    assert_eq!(eval_int("my $x = 3; if ($x > 5) { 1 } else { 0 }"), 0);
}

#[test]
fn if_elsif_chain() {
    assert_eq!(
        eval_int(
            "my $x = 2; if ($x == 0) { 0 } elsif ($x == 1) { 1 } elsif ($x == 2) { 2 } else { 9 }"
        ),
        2
    );
}

#[test]
fn unless() {
    assert_eq!(eval_int("my $x = 3; unless ($x > 5) { 1 } else { 0 }"), 1);
}

#[test]
fn while_loop() {
    assert_eq!(
        eval_int("my $i = 0; my $sum = 0; while ($i < 10) { $sum = $sum + $i; $i = $i + 1; } $sum"),
        45
    );
}

#[test]
fn until_loop() {
    assert_eq!(
        eval_int("my $i = 0; until ($i >= 4) { $i = $i + 1; } $i"),
        4
    );
}

#[test]
fn for_loop() {
    assert_eq!(
        eval_int("my $sum = 0; for (my $i = 0; $i < 5; $i = $i + 1) { $sum = $sum + $i; } $sum"),
        10
    );
}

#[test]
fn foreach_loop() {
    assert_eq!(
        eval_int("my $sum = 0; foreach my $x (1,2,3,4,5) { $sum = $sum + $x; } $sum"),
        15
    );
}

#[test]
fn foreach_uses_default_dollar_underscore() {
    assert_eq!(
        eval_int("my $sum = 0; foreach (1,2,3) { $sum = $sum + $_ } $sum"),
        6
    );
}

#[test]
fn postfix_foreach_statement() {
    assert_eq!(eval_int("my $sum = 0; $sum = $sum + $_ for 1,2,3; $sum"), 6);
}

#[test]
fn postfix_if() {
    assert_eq!(eval_int("my $x = 0; $x = 1 if 1 > 0; $x"), 1);
    assert_eq!(eval_int("my $x = 0; $x = 1 if 0 > 1; $x"), 0);
}

#[test]
fn postfix_unless() {
    assert_eq!(eval_int("my $x = 0; $x = 1 unless 0; $x"), 1);
}

#[test]
fn postfix_while_until() {
    assert_eq!(eval_int("my $x = 0; $x++ while $x < 4; $x"), 4);
    assert_eq!(eval_int("my $x = 0; $x++ until $x >= 4; $x"), 4);
}

#[test]
fn last_next() {
    assert_eq!(
        eval_int("my $sum = 0; for my $i (1..10) { last if $i > 5; $sum = $sum + $i; } $sum"),
        15
    );
    assert_eq!(
        eval_int("my $sum = 0; for my $i (1..10) { next if $i % 2 == 0; $sum = $sum + $i; } $sum"),
        25
    );
}
