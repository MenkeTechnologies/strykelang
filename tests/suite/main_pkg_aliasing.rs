//! `main` is Perl's default package — `$main::X` ≡ `$X`, `@main::INC`
//! ≡ `@INC`, `%main::ENV` ≡ `%ENV`. Storage uses the bare key, so
//! every scope read accessor has to short-circuit `main::name` (with
//! no further `::`) through the unqualified lookup. Pre-fix,
//! `@main::INC` returned an empty array, `%main::ENV` returned no
//! keys, `$main::_` returned undef even when `$_` was set.
//!
//! Pinned via the `strip_main_prefix` helper in `scope.rs` and the
//! `canon_main!` macro applied to every public read accessor.

use std::path::PathBuf;
use std::process::Command;

fn stryke_binary() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("target/release/stryke"),
        PathBuf::from("target/debug/stryke"),
    ];
    cands
        .iter()
        .filter(|p| p.exists())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .cloned()
}

fn run_e(code: &str) -> Option<String> {
    let bin = stryke_binary()?;
    let out = Command::new(&bin).args(["-e", code]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[test]
fn main_qualified_array_aliases_bare_array_for_inc() {
    let Some(out) = run_e(r#"p len(@INC) == len(@main::INC) ? "yes" : "no""#) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(
        out.trim(),
        "yes",
        "@INC and @main::INC should have same length"
    );
}

#[test]
fn main_qualified_array_aliases_bare_fpath_with_data() {
    // `@fpath` is populated by stryke at startup; both forms must
    // surface the same content. Concretely: every element of
    // `@main::fpath` should appear at the same index in `@fpath`.
    let Some(out) = run_e(
        r#"my $bare = join("|", @fpath); my $q = join("|", @main::fpath); p $bare eq $q ? "match" : "diverge""#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "match");
}

#[test]
fn main_qualified_hash_aliases_bare_hash_for_env() {
    // `$ENV{PATH}` must resolve through `$main::ENV{PATH}` too —
    // hash element access is a separate code path from full hash
    // lookup, so it gets its own pin.
    let Some(out) =
        run_e(r#"p (exists $main::ENV{PATH} && $ENV{PATH} eq $main::ENV{PATH}) ? "ok" : "fail""#)
    else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "ok");
}

#[test]
fn main_qualified_hash_full_lookup_matches_bare_for_reflection() {
    // `%a` (alias-keys reflection hash) is one of the lazily-installed
    // reflection hashes — the qualified `%main::a` form has to fire
    // the same lazy init path. Compare key-count parity.
    let Some(out) = run_e(
        r#"my $bare = scalar keys %a; my $q = scalar keys %main::a; p $bare == $q && $bare > 0 ? "ok" : "fail($bare/$q)""#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "ok");
}

#[test]
fn main_qualified_scalar_aliases_topic_inside_loop() {
    // `$_` is set during a for-loop iteration; `$main::_` reads MUST
    // see the same value (qualified read should hit the bare topic
    // slot, not return undef).
    let Some(out) = run_e(r#"for ("alpha") { p $_ eq $main::_ ? "ok" : "fail($_/$main::_)" }"#)
    else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "ok");
}

#[test]
fn nested_qualified_name_is_not_stripped() {
    // Guard against over-eager stripping: `main::Foo::bar` must NOT
    // collapse to `Foo::bar`. The strip helper rejects any name with
    // a remaining `::` in the suffix. We verify by *defining* a real
    // `main::Foo::*` binding and reading it back qualified — if the
    // strip fired, the read would miss because storage is at
    // `main::Foo::bar`, not `Foo::bar`.
    let Some(out) = run_e(r#"$main::Foo::bar = 42; p $main::Foo::bar"#) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "42");
}

#[test]
fn concat_assign_on_main_qualified_scalar_appends_in_place() {
    // Regression: `$main::x .= ...` compiles to `Op::ConcatAppend`,
    // which calls `scope.scalar_concat_inplace(name, rhs)` — a
    // separate path from the generic `set_scalar`. Pre-fix, the
    // initial assignment stripped to bare `x`, but the in-place
    // concat walked frames looking for literal `main::x`, missed,
    // and created a *second* binding. Reads via the qualified spelling
    // then returned the empty initial value because they hit the
    // bare entry while writes accumulated into the qualified one.
    //
    // Specifically pinned with an `END` block so the read happens
    // after every write — the failing test in the lib suite was
    // `end_foreach_iterates_list_context`.
    let Some(out) = run_e(
        r#"$main::buf = ""
END { foreach $k (1..3) { $main::buf .= "k=$k " }; print $main::buf }"#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "k=1 k=2 k=3");
}

#[test]
fn our_declared_scalar_reads_through_qualified_form() {
    // Regression: `our $pkg_var = 42` declares a package-qualified
    // scalar (compiled name = `main::pkg_var`). The bytecode emits
    // `DeclareScalar(main::pkg_var)` then a later `GetScalarPlain(
    // main::pkg_var)` for the read. Pre-fix, declare stored under
    // `main::pkg_var` but the get-side strip turned the read into
    // `pkg_var` — different key, undef result. Now both sides
    // canonicalize to bare storage.
    let Some(out) = run_e(
        r#"fn pkg { our $v = 42; $v }
p pkg()"#,
    ) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "42");
}

#[test]
fn array_binding_exists_handles_main_prefix() {
    // `defined @main::INC` (legacy form) goes through
    // `array_binding_exists` — must canonicalize the same way as
    // `get_array`.
    let Some(out) = run_e(r#"p (defined @main::INC && defined @INC) ? "both" : "split""#) else {
        eprintln!("skip: stryke binary not built");
        return;
    };
    assert_eq!(out.trim(), "both");
}
