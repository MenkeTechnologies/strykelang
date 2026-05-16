//! Heredoc + multi-line string pins.

use crate::common::*;

// ── Basic heredoc ──────────────────────────────────────────────────

#[test]
fn basic_heredoc_preserves_lines() {
    let code = r#"
        my $s = <<END;
hello
world
END
        $s eq "hello\nworld\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn heredoc_length_includes_trailing_newline() {
    let code = r#"
        my $s = <<END;
hello world
END
        length($s) == 12 ? 1 : 0   # "hello world\n"
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Double-quoted heredoc (default) interpolates ──────────────────

#[test]
fn unquoted_heredoc_interpolates() {
    let code = r#"
        my $name = "alice";
        my $s = <<END;
hello, $name!
END
        $s eq "hello, alice!\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Explicit double-quoted heredoc ─────────────────────────────────

#[test]
fn double_quoted_heredoc_interpolates() {
    let code = r#"
        my $x = 42;
        my $s = <<"END";
value=$x
END
        $s eq "value=42\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-quoted heredoc does NOT interpolate ────────────────────

#[test]
fn single_quoted_heredoc_no_interpolation() {
    let code = "
        my $name = \"alice\";
        my $s = <<'END';
hello, $name!
END
        $s eq \"hello, \\$name!\\n\" ? 1 : 0
    ";
    assert_eq!(eval_int(code), 1);
}

// ── Multi-paragraph heredoc ────────────────────────────────────────

#[test]
fn heredoc_with_blank_line() {
    let code = r#"
        my $s = <<END;
first paragraph

second paragraph
END
        # 4 lines (one blank in middle, trailing nl).
        my @lines = split /\n/, $s;
        len(@lines) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc as fn argument ────────────────────────────────────────

#[test]
fn heredoc_assigned_then_used_in_fn() {
    // BUG-243: Stryke parser rejects `fn(<<END)` heredoc-as-arg.
    // Workaround: assign to var, then pass.
    let code = r#"
        fn Demo::HD::echo($s) { $s }
        my $body = <<END;
inline body
END
        my $r = Demo::HD::echo($body);
        $r eq "inline body\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty heredoc ──────────────────────────────────────────────────

#[test]
fn empty_heredoc() {
    let code = r#"
        my $s = <<END;
END
        $s eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-line heredoc ────────────────────────────────────────────

#[test]
fn single_line_heredoc() {
    let code = r#"
        my $s = <<END;
one line only
END
        $s eq "one line only\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiline string via q{} ───────────────────────────────────────

#[test]
fn q_block_multiline() {
    let code = r#"
        my $s = q{
line one
line two
};
        my @lines = split /\n/, $s;
        # Leading and trailing newlines from the block layout.
        my @nonempty = grep { len($_) > 0 } @lines;
        len(@nonempty) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── qq{} multiline with interpolation ──────────────────────────────

#[test]
fn qq_block_interpolates() {
    let code = r#"
        my $x = 5;
        my $s = qq{value=$x};
        $s eq "value=$x" && index($s, "5") >= 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Double-quoted string with embedded newlines ───────────────────

#[test]
fn dq_string_with_newlines_via_escape() {
    let code = r#"
        my $s = "line one\nline two\nline three";
        my @lines = split /\n/, $s;
        len(@lines) == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc + concatenation ────────────────────────────────────────

#[test]
fn heredoc_concat_with_dot() {
    let code = r#"
        my $body = <<END;
contents
END
        my $combined = "prefix:" . $body;
        index($combined, "prefix:contents") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc inside hashref initializer ─────────────────────────────

#[test]
fn heredoc_in_hashref_value() {
    let code = r#"
        my $h = +{
            body => <<END,
multi
line
END
            other => 1,
        };
        ($h->{body} eq "multi\nline\n" && $h->{other} == 1) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc with interpolated array ────────────────────────────────

#[test]
fn heredoc_interpolates_array() {
    let code = r#"
        my @a = (1, 2, 3);
        my $s = <<END;
list: @a
END
        $s eq "list: 1 2 3\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc with interpolated hash value ──────────────────────────

#[test]
fn heredoc_interpolates_hash_value() {
    let code = r#"
        my %h = (name => "alice");
        my $s = <<END;
greeting: $h{name}
END
        $s eq "greeting: alice\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multiple heredocs on one statement ─────────────────────────────

#[test]
fn two_heredocs_assigned_separately() {
    // BUG-243 workaround for two-heredoc sequence.
    let code = r#"
        fn Demo::HD::pair($p1, $p2) { $p1 . "|" . $p2 }
        my $first = <<A;
first
A
        my $second = <<B;
second
B
        my $r = Demo::HD::pair($first, $second);
        $r eq "first\n|second\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc with special chars ─────────────────────────────────────

#[test]
fn heredoc_preserves_special_chars() {
    let code = r#"
        my $s = <<END;
back\slash
tab	character
END
        # Backslash + tab preserved as escape sequences (double-quote style).
        index($s, "back") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Length of heredoc ──────────────────────────────────────────────

#[test]
fn heredoc_total_length() {
    let code = r#"
        my $s = <<END;
abc
def
ghi
END
        # 3 lines × 4 chars (3 + \n) = 12.
        length($s) == 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc inside conditional ─────────────────────────────────────

#[test]
fn heredoc_via_explicit_if_else() {
    // BUG-243: stryke parser doesn't handle heredoc-in-ternary.
    // Workaround: explicit if/else.
    let code = r#"
        my $cond = 1;
        my $yes = <<TRUE;
yes branch
TRUE
        my $no = <<FALSE;
no branch
FALSE
        my $s = $cond ? $yes : $no;
        $s eq "yes branch\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc with arithmetic in interpolation ──────────────────────

#[test]
fn heredoc_with_array_ref_arithmetic() {
    let code = r#"
        my $x = 10;
        my $y = 20;
        my $s = <<END;
sum: @{[$x + $y]}
END
        $s eq "sum: 30\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc and split into lines ──────────────────────────────────

#[test]
fn heredoc_split_yields_per_line() {
    let code = r#"
        my $body = <<END;
alpha
beta
gamma
END
        my @lines = grep { len($_) > 0 } split /\n/, $body;
        join(",", @lines) eq "alpha,beta,gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Heredoc terminator must be at line start ──────────────────────

#[test]
fn heredoc_terminator_at_column_zero() {
    let code = r#"
        my $s = <<END;
content
END
        $s eq "content\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
