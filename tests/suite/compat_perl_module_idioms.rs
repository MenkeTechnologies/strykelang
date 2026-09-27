//! `--compat` regressions for idioms that core Perl modules and ordinary Perl
//! code lean on. Every expected string is stock `perl` output for the same
//! program.
//!
//! `--compat` is process-global, so each case runs the real `st` binary in a
//! subprocess — the same way `parity/run_parity.sh` does.

use std::process::Command;

/// Run `st --compat -e CODE` and return stdout, asserting the run succeeded.
fn compat(code: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .args(["--compat", "-e", code])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    assert!(
        out.status.success(),
        "st --compat -e {code:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ── `shift->...` / `pop->...` ────────────────────────────────────────────────

#[test]
fn shift_arrow_dereferences_the_shifted_value() {
    assert_eq!(
        compat("sub x { shift->{x} } sub h { shift->[1] } print x({x => 7}), h([1, 9])"),
        "79"
    );
    assert_eq!(compat("sub l { pop->[-1] } print l([1], [5, 6])"), "6");
}

#[test]
fn shift_arrow_method_call_is_the_oo_accessor_idiom() {
    assert_eq!(
        compat(
            "package P; sub new { bless {v => $_[1]}, $_[0] } sub v { shift->{v} } \
             sub twice { shift->v * 2 } package main; print P->new(4)->twice"
        ),
        "8"
    );
}

// ── `map(...)` / `grep(...)` parenthesized call form ────────────────────────

#[test]
fn map_and_grep_accept_the_parenthesized_call_form() {
    assert_eq!(compat(r#"print join ",", map("<$_>", 1, 2)"#), "<1>,<2>");
    assert_eq!(compat(r#"print join ",", map({ $_ * 10 } 1, 2)"#), "10,20");
    assert_eq!(
        compat(r#"print join ",", grep(/a/, qw(ab cd ba))"#),
        "ab,ba"
    );
    assert_eq!(compat(r#"print join ",", grep({ $_ > 1 } 1, 2, 3)"#), "2,3");
    // The closing paren ends the argument list, so a slice can follow it.
    assert_eq!(
        compat(r#"print join ",", (map($_ + 1, 1, 2, 3))[0, 2]"#),
        "2,4"
    );
    // File::Basename's shape: `map("\Q$_\E", @_)`.
    assert_eq!(compat(r#"print join " ", map("\Q$_\E", "a.b")"#), r"a\.b");
}

// ── imported names beat stryke extensions of the same spelling ──────────────

/// Write `My/Shadow.pm` exporting `basename` (a stryke extension name) into a
/// fresh directory and return that directory for `-I`.
fn module_dir_exporting_basename() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("stryke-compat-import-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("My")).expect("mkdir");
    std::fs::write(
        dir.join("My/Shadow.pm"),
        "package My::Shadow;\nrequire Exporter;\nour @ISA = ('Exporter');\n\
         our @EXPORT_OK = qw(basename);\nsub basename { \"mine:$_[0]\" }\n1;\n",
    )
    .expect("write module");
    dir
}

#[test]
fn imported_sub_wins_over_the_stryke_extension_it_shadows() {
    let dir = module_dir_exporting_basename();
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .arg("--compat")
        .arg(format!("-I{}", dir.display()))
        .args(["-e", r#"use My::Shadow qw(basename); print basename("x")"#])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "mine:x");
}

#[test]
fn use_constant_names_are_callable_even_when_they_spell_an_extension() {
    assert_eq!(
        compat("use constant PI => 3; use constant { E => 2, TAU => 6 }; print PI, E, TAU"),
        "326"
    );
}

#[test]
fn sub_declared_inside_a_block_wins_over_a_builtin_of_the_same_name() {
    // `s1` is a stryke builtin; the program's own `sub s1` must be the one called.
    assert_eq!(
        compat(r#"our $T; { sub s1 { $T = shift } } s1("u"); print "[$T]""#),
        "[u]"
    );
}

// ── bare `our` seen from subs registered at run time ────────────────────────

#[test]
fn bare_our_is_visible_in_subs_nested_in_blocks_under_strict() {
    // File::Basename's shape: `our(...)` at file scope, read by a sub defined
    // inside a `BEGIN` block, in a non-main package, under `use strict`.
    assert_eq!(
        compat(
            r#"package My::B; use strict; our $T; our (@A, %H);
               BEGIN { my @u; sub set_t { my $old = $T; $T = shift; $old } }
               BEGIN { sub fill { push @A, @_; $A[5] = 1; $H{k} = 1;
                                  @H{qw(a b c)} = (1, 2, 3); delete $H{c};
                                  scalar(@A) . (exists $H{a} ? "E" : "N") . $#A } }
               set_t("x"); print set_t("y"), fill(1), " [$T] [$My::B::T] [@A[0,5]] [",
               join(",", sort keys %H), "]";"#
        ),
        "x6E5 [y] [y] [1 1] [a,b,k]"
    );
}

#[test]
fn bare_our_does_not_reset_a_value_stored_by_begin() {
    assert_eq!(
        compat(r#"our $K; BEGIN { $K = "kept" } our $K; print $K"#),
        "kept"
    );
}

// ── module subs run in their own package ───────────────────────────────────

#[test]
fn module_sub_resolves_its_own_helpers_and_package_vars() {
    let dir = std::env::temp_dir().join(format!("stryke-compat-home-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("My")).expect("mkdir");
    std::fs::write(
        dir.join("My/Home.pm"),
        "package My::Home;\nuse strict;\nrequire Exporter;\nour @ISA = ('Exporter');\n\
         our @EXPORT_OK = qw(f);\nour $V = 'v';\nsub helper { 'h' . __PACKAGE__ }\n\
         sub f { helper() . $V }\n1;\n",
    )
    .expect("write module");
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .arg("--compat")
        .arg(format!("-I{}", dir.display()))
        .args(["-e", r#"use My::Home qw(f); print My::Home::f(), " ", f()"#])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hMy::Homev hMy::Homev"
    );
}

// ── stryke's reflection-hash names are ordinary hashes under --compat ───────

#[test]
fn hashes_named_like_reflection_hashes_are_writable() {
    // `%e`, `%a`, `%c`, … are stryke reflection hashes (frozen) in default mode.
    assert_eq!(
        compat(
            r#"my %e; $e{q}++; $e{q}++; my %a = (x => 1); $a{y} = 2; delete $a{x};
               my %c; @c{1, 2} = (3, 4); print $e{q}, join(",", keys %a), join(",", sort keys %c)"#
        ),
        "2y1,2"
    );
}

// ── closures share the enclosing arrays and hashes ──────────────────────────

#[test]
fn closure_mutations_of_outer_arrays_and_hashes_are_shared() {
    assert_eq!(
        compat(
            r#"my @r; my %h; my $f = sub { push @r, 1; $h{x}++ }; $f->(); $f->();
               sub run { $_[0]->() } my @s; run(sub { push @s, 2 });
               push @C, 5; my $pc = sub { push @C, 7; scalar @C }; $pc->(); $pc->();
               print scalar(@r), $h{x}, scalar(@s), "[@C]""#
        ),
        "221[5 7 7]"
    );
}

#[test]
fn each_closure_factory_call_gets_its_own_array() {
    assert_eq!(
        compat(
            r#"sub mk { my @a; sub { push @a, @_; scalar @a } }
               my ($c1, $c2) = (mk(), mk()); $c1->(1); $c1->(2);
               print $c1->(3), $c2->(9)"#
        ),
        "31"
    );
}

#[test]
fn calling_a_closure_does_not_roll_back_a_package_array() {
    assert_eq!(
        compat(r#"package Q; our @A = (1); my $f = sub { 1 }; push @A, 2; $f->(); print "@A""#),
        "1 2"
    );
}

// ── `&name(ARGS)` ────────────────────────────────────────────────────────────

#[test]
fn ampersand_call_passes_exactly_the_parenthesized_args() {
    assert_eq!(
        compat(
            r#"sub show { "F(@_)" } sub pass_along { &show } sub empty_args { &show() }
               print &show(2, 3), "a", &show(4), pass_along(7), empty_args(8)"#
        ),
        "F(2 3)aF(4)F(7)F()"
    );
}

// ── `use Module;` with no import list ───────────────────────────────────────

#[test]
fn use_without_a_list_imports_the_modules_export_list() {
    // `use Module;` imports `@EXPORT`. The parser reads it from `Module.pm`, so
    // an exported `basename` is the module's sub, not the stryke extension
    // (`--compat` rejected the call as a disabled extension).
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("My")).unwrap();
    std::fs::write(
        dir.path().join("My/Paths.pm"),
        "package My::Paths;\n\
         # @EXPORT = qw(decoy);\n\
         use Exporter 'import';\n\
         our @EXPORT_OK = qw(extra);\n\
         our @EXPORT = ('basename', qw(tail_of));\n\
         sub basename { \"mine:$_[0]\" }\n\
         sub tail_of { substr($_[0], -1) }\n\
         sub extra { 'x' }\n\
         1;\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_st"))
        .args(["--compat", "-I"])
        .arg(dir.path())
        .args([
            "-e",
            r#"use My::Paths; print basename("/a/b"), " ", tail_of("xyz")"#,
        ])
        .env("STRYKE_CACHE", "0")
        .output()
        .expect("spawn st");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "mine:/a/b z",
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
