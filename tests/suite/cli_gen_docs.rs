//! Coverage for `stryke --gen-docs FILE` — Markdown module-doc
//! generator. Pins:
//!   * the public-API decl kinds (fn, struct, enum, class, trait,
//!     package, `use constant`) each get their own Markdown section.
//!   * `## doc comment` blocks above a decl land as the decl's body.
//!   * the leading `##` block at the top of the file becomes the
//!     module-level description.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn stryke_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stryke"))
}

/// Run `stryke --gen-docs PATH` on `src` written to a tempfile.
/// Returns `(stdout, stderr, exit_code)`.
fn run_gen_docs(src: &str) -> (String, String, i32) {
    let tmp = tempfile::NamedTempFile::new().expect("tempfile");
    {
        let mut f = std::fs::File::create(tmp.path()).expect("create");
        f.write_all(src.as_bytes()).expect("write");
    }
    let out = Command::new(stryke_binary())
        .arg("--gen-docs")
        .arg(tmp.path())
        .stdin(Stdio::null())
        .output()
        .expect("spawn");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn gen_docs_emits_module_header_from_leading_doc_block() {
    let (stdout, _stderr, rc) = run_gen_docs(
        "## Project::Geom — 2D geometry primitives.\n\
         ## Provides points and rectangles.\n\
         package Project::Geom\n\
         1;\n",
    );
    assert_eq!(rc, 0);
    assert!(
        stdout.contains("# Module: Project::Geom"),
        "module title from package: {stdout}"
    );
    assert!(
        stdout.contains("2D geometry primitives"),
        "leading-doc block appears as module description: {stdout}"
    );
}

#[test]
fn gen_docs_emits_sections_for_every_public_decl_kind() {
    let (stdout, _stderr, rc) = run_gen_docs(
        "package Project::Demo\n\
         struct Point { x, y }\n\
         enum Color { Red, Green }\n\
         class Square impl SomeT { side: Int }\n\
         trait Drawable { fn draw }\n\
         use constant MAX => 100\n\
         fn add($a, $b) { $a + $b }\n",
    );
    assert_eq!(rc, 0, "stdout=\n{stdout}");
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
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }
}

#[test]
fn gen_docs_pairs_doc_comment_with_following_decl() {
    let (stdout, _stderr, rc) = run_gen_docs(
        "package Demo::Math\n\
         \n\
         ## Compute the area of a rectangle.\n\
         ## Returns Int.\n\
         fn area($r) { 1 }\n",
    );
    assert_eq!(rc, 0);
    // Both doc-comment lines must appear under the `area` heading.
    let area_idx = stdout.find("### `fn area($r)`").expect("area heading");
    let tail = &stdout[area_idx..];
    assert!(tail.contains("Compute the area"));
    assert!(tail.contains("Returns Int"));
}

#[test]
fn gen_docs_lists_struct_fields_under_struct_heading() {
    let (stdout, _stderr, rc) = run_gen_docs(
        "package D::S\nstruct Point { x, y, z }\n",
    );
    assert_eq!(rc, 0);
    let idx = stdout.find("### `struct D::S::Point`").expect("struct heading");
    let tail = &stdout[idx..];
    assert!(tail.contains("Fields:"));
    assert!(tail.contains("- `x`"));
    assert!(tail.contains("- `y`"));
    assert!(tail.contains("- `z`"));
}

#[test]
fn gen_docs_lists_enum_variants_under_enum_heading() {
    let (stdout, _stderr, rc) = run_gen_docs(
        "package D::E\nenum Op { Add, Sub, Mul }\n",
    );
    assert_eq!(rc, 0);
    let idx = stdout.find("### `enum D::E::Op`").expect("enum heading");
    let tail = &stdout[idx..];
    assert!(tail.contains("Variants:"));
    assert!(tail.contains("- `Add`"));
    assert!(tail.contains("- `Sub`"));
    assert!(tail.contains("- `Mul`"));
}

// ── `stryke gen-docs` subcommand (project-wide) ──────────────────

/// `stryke gen-docs DIR` walks the directory, generates one `.md`
/// per source file under `docs/` (or `--out DIR`), and writes an
/// `index.md` listing every module.
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

    // Per-file outputs.
    let foo_md = out_dir.join("lib/foo.md");
    let baz_md = out_dir.join("lib/baz.md");
    assert!(foo_md.exists(), "foo.md must be written: {foo_md:?}");
    assert!(baz_md.exists(), "baz.md must be written: {baz_md:?}");

    let foo = std::fs::read_to_string(&foo_md).unwrap();
    assert!(foo.contains("# Module: Project::Foo"));
    assert!(foo.contains("### `fn bar`"));

    // index.md lists both modules in sorted (path) order.
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
    std::fs::write(
        project.join("lib/keep.stk"),
        "package Demo\nfn keep { 1 }\n",
    )
    .unwrap();
    // These should NOT appear in the output.
    std::fs::write(
        project.join("target/skip.stk"),
        "package Skip\nfn no { 1 }\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".git/secret.stk"),
        "package Secret\nfn no { 1 }\n",
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
    assert!(status.status.success(), "stderr: {}", String::from_utf8_lossy(&status.stderr));

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
fn gen_docs_fallbacks_to_filename_when_no_package() {
    let (stdout, _stderr, rc) = run_gen_docs("fn util { 1 }\n");
    assert_eq!(rc, 0);
    // Title falls back to the tempfile's basename — verify the prefix
    // exists rather than guessing the random suffix.
    assert!(
        stdout.starts_with("# Module: "),
        "expected `# Module:` header: {stdout}"
    );
    assert!(stdout.contains("### `fn util`"));
}
