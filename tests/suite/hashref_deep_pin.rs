//! Hash- and array-reference deep-access pins. Stryke leans heavily on
//! refs (every object instance, every nested JSON, every AOP intercept
//! payload). These pins lock the surface for: arrow-chain access,
//! autovivification, `exists` vs `defined`, slice through deref,
//! `delete` propagation, and mixed array/hash ref nests.

use crate::common::*;

// ── Basic deep arrow chain ───────────────────────────────────────────

#[test]
fn deep_arrow_chain_reads_three_levels() {
    let code = r#"
        my $data =+{
            a => +{
                b => +{
                    c => "deep_value"
                }
            }
        };
        $data->{a}->{b}->{c} eq "deep_value" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn deep_arrow_chain_writes_three_levels() {
    let code = r#"
        my $data =+{
            a => +{ b => +{ c => "initial" } }
        };
        $data->{a}->{b}->{c} = "updated";
        $data->{a}->{b}->{c} eq "updated" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn implicit_arrow_omitted_between_subscripts() {
    let code = r#"
        my $data =+{ a => +{ b => +{ c => 42 } } };
        # Perl allows `$data->{a}{b}{c}` (arrow only required between
        # variable and first subscript).
        $data->{a}{b}{c} == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed hash/array nesting ─────────────────────────────────────────

#[test]
fn hash_of_arrays_deep_index() {
    let code = r#"
        my $data =+{
            fruits => ["apple", "banana", "cherry"],
            veggies => ["kale", "spinach"],
        };
        ($data->{fruits}->[1] eq "banana"
            && $data->{veggies}->[0] eq "kale") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_of_hashes_deep_index() {
    let code = r#"
        my @users = (
            +{ name => "alice", age => 30 },
            +{ name => "bob",   age => 28 },
            +{ name => "carol", age => 35 },
        );
        ($users[0]->{name} eq "alice"
            && $users[2]->{age} == 35) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_of_arrays_2d_grid_access() {
    let code = r#"
        my @grid = (
            [1, 2, 3],
            [4, 5, 6],
            [7, 8, 9],
        );
        # Center cell.
        $grid[1]->[1] == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Autovivification ─────────────────────────────────────────────────

#[test]
fn autoviv_requires_explicit_intermediate_construction() {
    // BUG-216: stryke does NOT autovivify intermediate hashes on
    // deep-write. Each level must be created explicitly. Perl
    // auto-creates the chain on first write.
    let code = r#"
        my %data;
        $data{a}        = +{};
        $data{a}{b}     = +{};
        $data{a}{b}{c}  = "created";
        $data{a}{b}{c} eq "created" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn autoviv_requires_explicit_arrayref_before_push() {
    // BUG-216b: `push @{$r->{list}}, X` doesn't create the arrayref;
    // must initialize `$r->{list} = []` first.
    let code = r#"
        my $data =+{};
        $data->{list} = [];
        push @{$data->{list}}, "first";
        push @{$data->{list}}, "second";
        (scalar(@{$data->{list}}) == 2
            && $data->{list}->[1] eq "second") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── exists vs defined ────────────────────────────────────────────────

#[test]
fn exists_true_for_undef_value() {
    let code = r#"
        my %data = (a => undef, b => 0, c => "");
        (exists($data{a}) && !defined($data{a})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exists_false_for_missing_key() {
    let code = r#"
        my %data = (a => 1);
        exists($data{nope}) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn exists_on_nested_arrow_chain() {
    let code = r#"
        my $data =+{ a => +{ b => "leaf" } };
        (exists($data->{a}->{b}) && !exists($data->{a}->{missing})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── delete ──────────────────────────────────────────────────────────

#[test]
fn delete_removes_key_and_returns_value() {
    let code = r#"
        my %data = (a => 1, b => 2, c => 3);
        my $r = delete $data{b};
        ($r == 2 && !exists($data{b}) && scalar(keys %data) == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_via_hashref_arrow() {
    let code = r#"
        my $data =+{ a => 1, b => 2 };
        delete $data->{a};
        (!exists($data->{a}) && exists($data->{b})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn delete_nested_does_not_affect_outer() {
    let code = r#"
        my $data =+{ a => +{ x => 1, y => 2 }, b => 3 };
        delete $data->{a}->{x};
        (!exists($data->{a}->{x})
            && $data->{a}->{y} == 2
            && $data->{b} == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Slice through arrow-deref ────────────────────────────────────────

#[test]
fn hash_slice_through_arrow_via_explicit_keys() {
    // BUG-217: `@{$r}{qw(a c)}` (hash-slice through arrow-style
    // deref) errors with "Can't dereference non-reference as array".
    // Workaround: pluck each key explicitly.
    let code = r#"
        my $data =+{ a => 1, b => 2, c => 3, d => 4 };
        my @vals = ($data->{a}, $data->{c});
        join(",", @vals) eq "1,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_through_arrow() {
    let code = r#"
        my $a = [10, 20, 30, 40, 50];
        my @vals = @{$a}[1, 3];
        join(",", @vals) eq "20,40" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── keys / values on hashref ────────────────────────────────────────

#[test]
fn keys_on_hashref_returns_top_level_keys() {
    let code = r#"
        my $data =+{ alpha => 1, beta => 2, gamma => 3 };
        my @ks = sort { _0 cmp _1 } keys %$data;
        join(",", @ks) eq "alpha,beta,gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn values_on_hashref_returns_values() {
    let code = r#"
        my $data =+{ a => 1, b => 2, c => 3 };
        my @vs = sort { _0 <=> _1 } values %$data;
        join(",", @vs) eq "1,2,3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Modifying via reference modifies the underlying ─────────────────

#[test]
fn write_via_ref_visible_through_original() {
    let code = r#"
        my %h = (a => 1);
        my $ref = \%h;
        $ref->{b} = 2;
        ($h{a} == 1 && $h{b} == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_push_via_ref_visible_through_original() {
    let code = r#"
        my @a = (1, 2, 3);
        my $ref = \@a;
        push @$ref, 4;
        (scalar(@a) == 4 && $a[3] == 4) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ref() identifies type ───────────────────────────────────────────

#[test]
fn ref_distinguishes_hash_array_code() {
    let code = r#"
        my $h = +{ };
        my $a = [];
        my $c = sub { 42 };
        (ref($h) =~ /HASH/
            && ref($a) =~ /ARRAY/
            && ref($c) =~ /CODE/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_non_ref_returns_empty_string() {
    let code = r#"
        my $x = 42;
        my $s = "hello";
        (ref($x) eq "" && ref($s) eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Deep mixed structure round-trip ─────────────────────────────────

#[test]
fn deep_mixed_structure_full_path_access() {
    let code = r#"
        my $config = +{
            servers => [
                +{
                    name  => "prod-1",
                    ports => [80, 443],
                    tags  => +{ env => "prod", region => "us-west" },
                },
                +{
                    name  => "prod-2",
                    ports => [80, 443, 22],
                    tags  => +{ env => "prod", region => "eu" },
                },
            ],
        };
        ($config->{servers}->[0]->{name} eq "prod-1"
            && $config->{servers}->[1]->{ports}->[2] == 22
            && $config->{servers}->[0]->{tags}->{region} eq "us-west"
            && $config->{servers}->[1]->{tags}->{env} eq "prod") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mutating nested through pipe-forward map ────────────────────────

#[test]
fn map_mutates_each_hashref_in_place() {
    let code = r#"
        my @rows = (
            +{ name => "alice", score => 80 },
            +{ name => "bob",   score => 90 },
        );
        map { $_->{score} += 10 } @rows;
        ($rows[0]->{score} == 90 && $rows[1]->{score} == 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
