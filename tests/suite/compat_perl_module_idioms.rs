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
    assert_eq!(compat(r#"print join ",", grep(/a/, qw(ab cd ba))"#), "ab,ba");
    assert_eq!(compat(r#"print join ",", grep({ $_ > 1 } 1, 2, 3)"#), "2,3");
    // The closing paren ends the argument list, so a slice can follow it.
    assert_eq!(compat(r#"print join ",", (map($_ + 1, 1, 2, 3))[0, 2]"#), "2,4");
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
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
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
