//! Dispatch-table pins: hash of coderefs called by string key.

use crate::common::*;

// ── Basic dispatch ─────────────────────────────────────────────────

#[test]
fn dispatch_table_via_hash_of_subs() {
    let code = r#"
        my %ops = (
            inc => sub { $_[0] + 1 },
            dec => sub { $_[0] - 1 },
            sq  => sub { $_[0] * $_[0] },
        );
        ($ops{inc}->(10) == 11
            && $ops{dec}->(10) == 9
            && $ops{sq}->(7) == 49) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dispatch_with_string_key_variable() {
    let code = r#"
        my %ops = (
            add => sub { $_[0] + $_[1] },
            mul => sub { $_[0] * $_[1] },
        );
        my $op = "mul";
        $ops{$op}->(6, 7) == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Missing key dispatch ──────────────────────────────────────────

#[test]
fn missing_key_via_default_handler() {
    let code = r#"
        my %ops = (
            known => sub { "ok" },
        );
        my $default = sub { "unknown" };
        my $op = "missing";
        my $r = (exists $ops{$op}) ? $ops{$op}->() : $default->();
        $r eq "unknown" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Variadic forwarding via @_ ────────────────────────────────────

#[test]
fn dispatch_forwards_at_underscore() {
    let code = r#"
        my %ops = (
            sumall => sub { my $s = 0; $s += $_ for @_; $s },
        );
        $ops{sumall}->(1, 2, 3, 4, 5) == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Method-style dispatch on hashref ──────────────────────────────

#[test]
fn dispatch_via_arrow_on_hashref_of_subs() {
    let code = r#"
        my $api = +{
            greet => sub { "hello, $_[0]" },
            farewell => sub { "bye, $_[0]" },
        };
        ($api->{greet}->("alice") eq "hello, alice"
            && $api->{farewell}->("bob") eq "bye, bob") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch with closure over enclosing state ────────────────────

#[test]
fn dispatch_with_closure_captures() {
    let code = r#"
        my $base = 100;
        my %ops = (
            add  => sub { $base + $_[0] },
            mult => sub { $base * $_[0] },
        );
        ($ops{add}->(5) == 105 && $ops{mult}->(2) == 200) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch enumeration ──────────────────────────────────────────

#[test]
fn enumerate_available_ops() {
    let code = r#"
        my %ops = (
            cmd_a => sub { 1 },
            cmd_b => sub { 2 },
            cmd_c => sub { 3 },
        );
        my @available = sort { _0 cmp _1 } keys %ops;
        join(",", @available) eq "cmd_a,cmd_b,cmd_c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Chain of dispatches ───────────────────────────────────────────

#[test]
fn pipeline_of_dispatched_ops() {
    let code = r#"
        my %ops = (
            dbl  => sub { $_[0] * 2 },
            inc  => sub { $_[0] + 1 },
            sqr  => sub { $_[0] * $_[0] },
        );
        my $v = 3;
        for my $name ("dbl", "inc", "sqr") {
            $v = $ops{$name}->($v);
        }
        # 3 → 6 → 7 → 49.
        $v == 49 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch returns hashref ──────────────────────────────────────

#[test]
fn dispatch_returns_structured_result() {
    let code = r#"
        my %ops = (
            stats => sub {
                my @nums = @_;
                my $sum = 0; $sum += $_ for @nums;
                +{ count => len(@nums), sum => $sum, avg => $sum / len(@nums) }
            },
        );
        my $r = $ops{stats}->(10, 20, 30, 40, 50);
        ($r->{count} == 5 && $r->{sum} == 150 && $r->{avg} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch with action selection from external input ───────────

#[test]
fn dispatch_from_input_string() {
    let code = r#"
        my %actions = (
            "start"  => sub { "started" },
            "stop"   => sub { "stopped" },
            "pause"  => sub { "paused" },
        );
        my @inputs = ("start", "stop", "pause", "unknown");
        my @results;
        for my $i (@inputs) {
            if (exists $actions{$i}) {
                push @results, $actions{$i}->();
            } else {
                push @results, "noop";
            }
        }
        join(",", @results) eq "started,stopped,paused,noop" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Update dispatch table at runtime ──────────────────────────────

#[test]
fn dynamic_dispatch_table_update() {
    let code = r#"
        my %ops;
        $ops{first} = sub { 1 };
        $ops{second} = sub { 2 };
        # Update an existing entry.
        $ops{first} = sub { 100 };
        ($ops{first}->() == 100 && $ops{second}->() == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Remove an entry ──────────────────────────────────────────────

#[test]
fn delete_dispatch_entry() {
    let code = r#"
        my %ops = (
            a => sub { 1 },
            b => sub { 2 },
        );
        delete $ops{a};
        (!exists $ops{a} && exists $ops{b}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Default-handler wrapper pattern ──────────────────────────────

#[test]
fn dispatch_with_or_default_pattern() {
    let code = r#"
        my %ops = (
            known => sub { "K" },
        );
        my $fb = sub { "fallback" };
        # Pattern: (op || fb)->(args)
        my $name = "unknown";
        my $f = $ops{$name} // $fb;
        $f->() eq "fallback" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Many-key dispatch ────────────────────────────────────────────

#[test]
fn many_key_dispatch_table_consistency() {
    let code = r#"
        my %ops;
        for my $i (1:50) {
            my $n = $i;
            $ops{"op_$i"} = sub { $n * 10 };
        }
        my $total = 0;
        for my $k (sort { _0 cmp _1 } keys %ops) {
            $total += $ops{$k}->();
        }
        # 10 + 20 + ... + 500 = 12750.
        $total == 12750 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Coderef with structured arg ──────────────────────────────────

#[test]
fn dispatch_with_hashref_arg() {
    let code = r#"
        my %ops = (
            compute => sub {
                my $req = $_[0];
                $req->{a} + $req->{b}
            },
        );
        my $r = $ops{compute}->(+{ a => 10, b => 32 });
        $r == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Nested dispatch tables ──────────────────────────────────────

#[test]
fn nested_dispatch_table_two_levels() {
    let code = r#"
        my %api = (
            users => +{
                create => sub { "user-created" },
                delete => sub { "user-deleted" },
            },
            orders => +{
                place  => sub { "order-placed" },
                cancel => sub { "order-cancelled" },
            },
        );
        my $r1 = $api{users}->{create}->();
        my $r2 = $api{orders}->{cancel}->();
        ($r1 eq "user-created" && $r2 eq "order-cancelled") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch result feeding another dispatch ─────────────────────

#[test]
fn dispatch_result_passed_to_next() {
    let code = r#"
        my %first = (
            wrap => sub { [$_[0]] },
        );
        my %second = (
            len_of => sub { len(@{$_[0]}) },
        );
        my $wrapped = $first{wrap}->("hello");
        my $n = $second{len_of}->($wrapped);
        $n == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Dispatch with multiple registered handlers (array of subs) ──

#[test]
fn array_of_subs_invokes_each() {
    let code = r#"
        my @handlers = (
            sub { 10 },
            sub { 20 },
            sub { 30 },
        );
        my $total = 0;
        for my $h (@handlers) {
            $total += $h->();
        }
        $total == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Self-modifying dispatch (op replaces itself) ────────────────

#[test]
fn op_can_modify_dispatch_table() {
    let code = r#"
        mysync %ops;
        $ops{first} = sub {
            $ops{first} = sub { "second_call" };
            "first_call"
        };
        my $r1 = $ops{first}->();
        my $r2 = $ops{first}->();
        ($r1 eq "first_call" && $r2 eq "second_call") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
