//! Coverage for the `stryke gen-docs` subcommand — the project-wide
//! Markdown module-doc generator. Pins:
//!   * the public-API decl kinds (fn, struct, enum, class, trait,
//!     package, `use constant`) each get their own Markdown section.
//!   * `## doc comment` blocks above a decl land as the decl's body.
//!   * the leading `##` block at the top of the file becomes the
//!     module-level description.
//!   * directory walking writes one `.md` per source + an `index.md`,
//!     mirroring the source layout; skips `.git` / `target` / dot dirs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn stryke_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stryke"))
}

/// Run `stryke gen-docs SRC --out OUT` on a single-file `src`. Writes
/// `src` to a tempfile inside a tempdir, runs the subcommand against
/// the tempdir (so the walker picks up exactly one file), and returns
/// the generated Markdown for that file.
fn gen_docs_single(src: &str) -> (String, i32, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    let stk_path = project.join("module.stk");
    std::fs::write(&stk_path, src).expect("write src");
    let out_dir = project.join("out");
    let out = Command::new(stryke_binary())
        .arg("gen-docs")
        .arg(project)
        .arg("--out")
        .arg(&out_dir)
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    let md = std::fs::read_to_string(out_dir.join("module.md"))
        .unwrap_or_else(|_| String::from("<no module.md written>"));
    (
        md,
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn gen_docs_emits_module_header_from_leading_doc_block() {
    let (md, rc, _err) = gen_docs_single(
        "## Project::Geom — 2D geometry primitives.\n\
         ## Provides points and rectangles.\n\
         package Project::Geom\n\
         1;\n",
    );
    assert_eq!(rc, 0);
    assert!(
        md.contains("# Module: Project::Geom"),
        "module title from package: {md}"
    );
    assert!(
        md.contains("2D geometry primitives"),
        "leading-doc block appears as module description: {md}"
    );
}

#[test]
fn gen_docs_emits_sections_for_every_public_decl_kind() {
    let (md, rc, _err) = gen_docs_single(
        "package Project::Demo\n\
         struct Point { x, y }\n\
         enum Color { Red, Green }\n\
         class Square impl SomeT { side: Int }\n\
         trait Drawable { fn draw }\n\
         use constant MAX => 100\n\
         fn add($a, $b) { $a + $b }\n",
    );
    assert_eq!(rc, 0, "md=\n{md}");
    for expected in [
        "## Packages",
        "## Constants",
        "## Traits",
        "## Structs",
        "## Enums",
        "## Classes",
        "## Subroutines",
        "### `struct Project::Demo::Point`",
        "### `enum Project::Demo::Color`",
        "### `class Project::Demo::Square`",
        "### `trait Project::Demo::Drawable`",
        "### `MAX`",
        "### `fn add($a, $b)`",
    ] {
        assert!(md.contains(expected), "missing `{expected}` in:\n{md}");
    }
}

#[test]
fn gen_docs_pairs_doc_comment_with_following_decl() {
    let (md, rc, _err) = gen_docs_single(
        "package Demo::Math\n\
         \n\
         ## Compute the area of a rectangle.\n\
         ## Returns Int.\n\
         fn area($r) { 1 }\n",
    );
    assert_eq!(rc, 0);
    let area_idx = md.find("### `fn area($r)`").expect("area heading");
    let tail = &md[area_idx..];
    assert!(tail.contains("Compute the area"));
    assert!(tail.contains("Returns Int"));
}

#[test]
fn gen_docs_lists_struct_fields_under_struct_heading() {
    let (md, rc, _err) = gen_docs_single("package D::S\nstruct Point { x, y, z }\n");
    assert_eq!(rc, 0);
    let idx = md.find("### `struct D::S::Point`").expect("struct heading");
    let tail = &md[idx..];
    assert!(tail.contains("Fields:"));
    assert!(tail.contains("- `x`"));
    assert!(tail.contains("- `y`"));
    assert!(tail.contains("- `z`"));
}

#[test]
fn gen_docs_lists_enum_variants_under_enum_heading() {
    let (md, rc, _err) = gen_docs_single("package D::E\nenum Op { Add, Sub, Mul }\n");
    assert_eq!(rc, 0);
    let idx = md.find("### `enum D::E::Op`").expect("enum heading");
    let tail = &md[idx..];
    assert!(tail.contains("Variants:"));
    assert!(tail.contains("- `Add`"));
    assert!(tail.contains("- `Sub`"));
    assert!(tail.contains("- `Mul`"));
}

#[test]
fn gen_docs_fallbacks_to_filename_when_no_package() {
    let (md, rc, _err) = gen_docs_single("fn util { 1 }\n");
    assert_eq!(rc, 0);
    assert!(
        md.starts_with("# Module: "),
        "expected `# Module:` header: {md}"
    );
    assert!(md.contains("### `fn util`"));
}

// ── `stryke gen-docs` directory walk + index.md ──────────────────

#[test]
fn gen_docs_subcommand_walks_directory_and_writes_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    std::fs::create_dir(project.join("lib")).unwrap();
    std::fs::write(
        project.join("lib/foo.stk"),
        "## Project::Foo — example.\n\
         package Project::Foo\n\
         fn bar { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        project.join("lib/baz.stk"),
        "## Project::Baz — another.\n\
         package Project::Baz\n\
         struct Item { name, qty }\n",
    )
    .unwrap();
    let out_dir = project.join("out");

    let status = Command::new(stryke_binary())
        .arg("gen-docs")
        .arg(project)
        .arg("--out")
        .arg(&out_dir)
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    assert!(
        status.status.success(),
        "exit code: {:?}\nstdout: {}\nstderr: {}",
        status.status.code(),
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr),
    );

    let foo_md = out_dir.join("lib/foo.md");
    let baz_md = out_dir.join("lib/baz.md");
    assert!(foo_md.exists(), "foo.md must be written: {foo_md:?}");
    assert!(baz_md.exists(), "baz.md must be written: {baz_md:?}");

    let foo = std::fs::read_to_string(&foo_md).unwrap();
    assert!(foo.contains("# Module: Project::Foo"));
    assert!(foo.contains("### `fn bar`"));

    let idx = std::fs::read_to_string(out_dir.join("index.md")).unwrap();
    assert!(idx.contains("# Module index"));
    assert!(idx.contains("[Project::Foo](lib/foo.md)"));
    assert!(idx.contains("[Project::Baz](lib/baz.md)"));
}

#[test]
fn gen_docs_subcommand_skips_target_and_dot_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path();
    std::fs::create_dir(project.join("lib")).unwrap();
    std::fs::create_dir(project.join("target")).unwrap();
    std::fs::create_dir(project.join(".git")).unwrap();
    std::fs::write(project.join("lib/keep.stk"), "package Demo\nfn keep { 1 }\n").unwrap();
    std::fs::write(project.join("target/skip.stk"), "package Skip\nfn no { 1 }\n").unwrap();
    std::fs::write(project.join(".git/secret.stk"), "package Secret\nfn no { 1 }\n").unwrap();

    let out_dir = project.join("out");
    let status = Command::new(stryke_binary())
        .arg("gen-docs")
        .arg(project)
        .arg("--out")
        .arg(&out_dir)
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    assert!(
        status.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    assert!(out_dir.join("lib/keep.md").exists());
    assert!(!out_dir.join("target/skip.md").exists());
    assert!(!out_dir.join(".git/secret.md").exists());

    let idx = std::fs::read_to_string(out_dir.join("index.md")).unwrap();
    assert!(idx.contains("[Demo]"));
    assert!(!idx.contains("[Skip]"));
    assert!(!idx.contains("[Secret]"));
}

#[test]
fn gen_docs_subcommand_help_prints_usage() {
    let out = Command::new(stryke_binary())
        .arg("gen-docs")
        .arg("--help")
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("usage: stryke gen-docs"));
    assert!(stdout.contains("--out"));
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn gen_docs_subcommand_no_flag_form_exists() {
    // Verify the `--gen-docs` flag form is intentionally absent —
    // `stryke gen-docs` is the only entry point. Running stryke with
    // `--gen-docs FILE` should NOT print Markdown docs; it should
    // error (clap will treat it as an unrecognized option in
    // free-form arg mode, which exits non-zero).
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(tmp.path(), "package T\nfn x { 1 }\n").unwrap();
    let out = Command::new(stryke_binary())
        .arg("--gen-docs")
        .arg(tmp.path())
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    // Either the binary refuses the flag (non-zero exit) OR it
    // silently ignored it and ran the script. EITHER way we must not
    // see a `# Module:` header on stdout — that would mean a stale
    // `--gen-docs` flag survived the removal.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("# Module:"),
        "`--gen-docs` flag must be removed; got Markdown on stdout:\n{stdout}",
    );
}
