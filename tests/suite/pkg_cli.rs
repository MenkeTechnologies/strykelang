//! CLI-level tests for the package manager subcommands. Complements
//! `pkg_e2e.rs` which exercises the `stryke::pkg::*` Rust API directly —
//! this file shells out to the actual binary so the `main.rs` dispatch
//! layer is also covered. Regression of any of these would mean a user's
//! `s new` / `s install` / `s tree` invocation broke.

use std::path::PathBuf;
use std::process::Command;

fn stryke() -> &'static str {
    env!("CARGO_BIN_EXE_st")
}

fn unique_tempdir(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let p = std::env::temp_dir().join(format!("stryke-pkg-cli-{}-{}-{}", tag, pid, nanos));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn assert_success(label: &str, out: &std::process::Output) {
    assert!(
        out.status.success(),
        "{label}: status={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_new_scaffolds_full_project_layout() {
    let tmp = unique_tempdir("new");
    let out = Command::new(stryke())
        .current_dir(&tmp)
        .args(["new", "myapp"])
        .output()
        .expect("spawn");
    assert_success("stryke new myapp", &out);

    // The full layout from RFC §"Project Root Stays Clean".
    let proj = tmp.join("myapp");
    assert!(proj.is_dir(), "myapp/ created");
    assert!(proj.join("stryke.toml").is_file(), "stryke.toml emitted");
    assert!(proj.join("main.stk").is_file(), "main.stk emitted");
    assert!(proj.join(".gitignore").is_file(), ".gitignore emitted");
    for sub in ["lib", "t", "benches", "bin", "examples"] {
        assert!(proj.join(sub).is_dir(), "{sub}/ subdir created");
    }

    // The default manifest must have name + version + a [bin] entry pointing
    // at main.stk so `s run` works out of the box.
    let manifest = std::fs::read_to_string(proj.join("stryke.toml")).unwrap();
    assert!(manifest.contains("name = \"myapp\""), "{manifest}");
    assert!(manifest.contains("version = \"0.1.0\""), "{manifest}");
    assert!(manifest.contains("main.stk"), "{manifest}");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_install_writes_lockfile_for_no_deps() {
    let tmp = unique_tempdir("install_nodeps");
    // Scaffold first.
    let new_out = Command::new(stryke())
        .current_dir(&tmp)
        .args(["new", "empty"])
        .output()
        .expect("spawn");
    assert_success("stryke new empty", &new_out);

    let proj = tmp.join("empty");
    let store = tmp.join(".stryke");
    let out = Command::new(stryke())
        .current_dir(&proj)
        .env("STRYKE_HOME", &store)
        .arg("install")
        .output()
        .expect("spawn");
    assert_success("stryke install (no deps)", &out);
    let lock = proj.join("stryke.lock");
    assert!(lock.is_file(), "stryke.lock written");
    let body = std::fs::read_to_string(&lock).unwrap();
    assert!(body.contains("version = 1"), "{body}");
    assert!(
        body.contains("Auto-generated"),
        "header comment present: {body}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_install_resolves_path_dep_into_store() {
    let tmp = unique_tempdir("install_path");

    // Build a path-dep at <tmp>/mylib/.
    let mylib = tmp.join("mylib");
    std::fs::create_dir_all(mylib.join("lib")).unwrap();
    std::fs::write(
        mylib.join("stryke.toml"),
        "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(mylib.join("lib/Greet.stk"), "1\n").unwrap();

    // Build a consumer at <tmp>/myapp/.
    let myapp = tmp.join("myapp");
    std::fs::create_dir_all(&myapp).unwrap();
    std::fs::write(
        myapp.join("stryke.toml"),
        format!(
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[deps.mylib]\npath = \"{}\"\n",
            mylib.display()
        ),
    )
    .unwrap();

    let store = tmp.join(".stryke");
    let out = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .arg("install")
        .output()
        .expect("spawn");
    assert_success("stryke install (path dep)", &out);
    assert!(
        store.join("store/mylib@1.0.0").is_dir(),
        "store entry created: {:?}",
        store.join("store/mylib@1.0.0")
    );
    assert!(
        store.join("store/mylib@1.0.0/lib/Greet.stk").is_file(),
        "package contents copied"
    );
    let lock = std::fs::read_to_string(myapp.join("stryke.lock")).unwrap();
    assert!(lock.contains("name = \"mylib\""), "{lock}");
    assert!(lock.contains("path+file://"), "{lock}");
    assert!(lock.contains("integrity = \"sha256-"), "{lock}");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_install_rejects_registry_dep_with_clear_message() {
    let tmp = unique_tempdir("install_reg");
    let myapp = tmp.join("myapp");
    std::fs::create_dir_all(&myapp).unwrap();
    std::fs::write(
        myapp.join("stryke.toml"),
        "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[deps]\nhttp = \"1.0\"\n",
    )
    .unwrap();

    let store = tmp.join(".stryke");
    let out = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .arg("install")
        .output()
        .expect("spawn");
    assert!(
        !out.status.success(),
        "registry dep must fail install (RFC phases 7-8 not wired)"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("registry dep") && stderr.contains("http"),
        "diagnostic must point at the dep + RFC phase: stderr={stderr}"
    );
    // Lockfile must NOT be written when resolution fails.
    assert!(
        !myapp.join("stryke.lock").exists(),
        "lockfile must not appear when install errors"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_tree_prints_dep_graph_from_lockfile() {
    let tmp = unique_tempdir("tree");
    let mylib = tmp.join("mylib");
    std::fs::create_dir_all(mylib.join("lib")).unwrap();
    std::fs::write(
        mylib.join("stryke.toml"),
        "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(mylib.join("lib/X.stk"), "1\n").unwrap();

    let myapp = tmp.join("myapp");
    std::fs::create_dir_all(&myapp).unwrap();
    std::fs::write(
        myapp.join("stryke.toml"),
        format!(
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[deps.mylib]\npath = \"{}\"\n",
            mylib.display()
        ),
    )
    .unwrap();

    let store = tmp.join(".stryke");
    let install = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .arg("install")
        .output()
        .expect("spawn");
    assert_success("install before tree", &install);

    let tree = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .arg("tree")
        .output()
        .expect("spawn");
    assert_success("stryke tree", &tree);
    let stdout = String::from_utf8_lossy(&tree.stdout);
    assert!(stdout.contains("myapp v0.1.0"), "root header: {stdout}");
    assert!(stdout.contains("mylib v1.0.0"), "child entry: {stdout}");
    assert!(
        stdout.contains("└──") || stdout.contains("├──"),
        "tree connector glyph: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_info_prints_lockfile_entry_for_dep() {
    let tmp = unique_tempdir("info");
    let mylib = tmp.join("mylib");
    std::fs::create_dir_all(mylib.join("lib")).unwrap();
    std::fs::write(
        mylib.join("stryke.toml"),
        "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n",
    )
    .unwrap();
    std::fs::write(mylib.join("lib/X.stk"), "1\n").unwrap();

    let myapp = tmp.join("myapp");
    std::fs::create_dir_all(&myapp).unwrap();
    std::fs::write(
        myapp.join("stryke.toml"),
        format!(
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[deps.mylib]\npath = \"{}\"\n",
            mylib.display()
        ),
    )
    .unwrap();

    let store = tmp.join(".stryke");
    Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .arg("install")
        .output()
        .expect("spawn");

    let info = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .args(["info", "mylib"])
        .output()
        .expect("spawn");
    assert_success("stryke info mylib", &info);
    let stdout = String::from_utf8_lossy(&info.stdout);
    assert!(stdout.contains("name:"), "name header present: {stdout}");
    assert!(stdout.contains("mylib"), "{stdout}");
    assert!(stdout.contains("1.0.0"), "{stdout}");
    assert!(stdout.contains("integrity:"), "integrity line: {stdout}");
    assert!(stdout.contains("sha256-"), "hash printed: {stdout}");
    assert!(stdout.contains("store path:"), "store path line: {stdout}");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_add_then_remove_round_trip() {
    let tmp = unique_tempdir("add_remove");

    // Path-dep target.
    let mylib = tmp.join("mylib");
    std::fs::create_dir_all(mylib.join("lib")).unwrap();
    std::fs::write(
        mylib.join("stryke.toml"),
        "[package]\nname = \"mylib\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(mylib.join("lib/X.stk"), "1\n").unwrap();

    // Scaffold consumer.
    let new_out = Command::new(stryke())
        .current_dir(&tmp)
        .args(["new", "myapp"])
        .output()
        .expect("spawn");
    assert_success("scaffold for add/remove", &new_out);
    let myapp = tmp.join("myapp");
    let store = tmp.join(".stryke");

    // Add via CLI.
    let add_out = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .args(["add", "mylib", &format!("--path={}", mylib.display())])
        .output()
        .expect("spawn");
    assert_success("stryke add mylib --path=...", &add_out);
    let manifest_after_add = std::fs::read_to_string(myapp.join("stryke.toml")).unwrap();
    assert!(
        manifest_after_add.contains("[deps.mylib]"),
        "dep written into manifest: {manifest_after_add}"
    );
    assert!(
        myapp.join("stryke.lock").is_file(),
        "lockfile written by add"
    );

    // Remove via CLI.
    let rm_out = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", &store)
        .args(["remove", "mylib"])
        .output()
        .expect("spawn");
    assert_success("stryke remove mylib", &rm_out);
    let manifest_after_remove = std::fs::read_to_string(myapp.join("stryke.toml")).unwrap();
    assert!(
        !manifest_after_remove.contains("[deps.mylib]"),
        "dep removed from manifest: {manifest_after_remove}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_tree_without_lockfile_errors_with_hint() {
    let tmp = unique_tempdir("tree_no_lock");
    let new_out = Command::new(stryke())
        .current_dir(&tmp)
        .args(["new", "myapp"])
        .output()
        .expect("spawn");
    assert_success("scaffold", &new_out);
    let myapp = tmp.join("myapp");

    let tree_out = Command::new(stryke())
        .current_dir(&myapp)
        .env("STRYKE_HOME", tmp.join(".stryke"))
        .arg("tree")
        .output()
        .expect("spawn");
    assert!(
        !tree_out.status.success(),
        "tree must error when no lockfile exists"
    );
    let stderr = String::from_utf8_lossy(&tree_out.stderr);
    assert!(
        stderr.contains("stryke.lock") && stderr.contains("install"),
        "diagnostic suggests `s install`: stderr={stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_subcommands_outside_project_fail_with_hint() {
    // `s install` / `s add` / `s tree` / `s info` from a directory that has
    // no stryke.toml above it must fail loudly, not silently succeed or
    // accidentally pollute the cwd.
    let tmp = unique_tempdir("no_project");
    let out = Command::new(stryke())
        .current_dir(&tmp)
        .arg("tree")
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "tree must fail outside a project");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("stryke.toml"),
        "diagnostic mentions missing manifest: stderr={stderr}"
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_completions_zsh_emits_compdef_header() {
    // `stryke completions zsh` is a stable contract — Homebrew, oh-my-zsh,
    // and zinit all consume this. The output must start with `#compdef`.
    let out = Command::new(stryke())
        .args(["completions", "zsh"])
        .output()
        .expect("spawn");
    assert_success("stryke completions zsh", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("#compdef"),
        "completions must start with #compdef: head={}",
        stdout.lines().next().unwrap_or("")
    );
}

#[test]
fn cli_convert_help_describes_perl_to_stryke() {
    let out = Command::new(stryke())
        .args(["convert", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke convert -h", &out);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("convert") && combined.contains("Perl"),
        "help text mentions convert direction: {combined}"
    );
}

#[test]
fn cli_deconvert_help_describes_stryke_to_perl() {
    let out = Command::new(stryke())
        .args(["deconvert", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke deconvert -h", &out);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.contains("deconvert") || combined.contains("Perl"),
        "help text describes deconvert: {combined}"
    );
}

// Per-subcommand `-h` regression: each new pkg subcommand must accept
// `-h` / `--help` AND must not have side effects when help is requested.
// `s new -h` previously interpreted `-h` as the project name and created
// a directory called `-h` in cwd.
#[test]
fn cli_new_dash_h_emits_help_without_creating_directory() {
    let tmp = unique_tempdir("new_dash_h");
    let out = Command::new(stryke())
        .current_dir(&tmp)
        .args(["new", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke new -h", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("usage: stryke new"), "help text: {stdout}");
    assert!(
        !tmp.join("-h").exists(),
        "BUG: `s new -h` created a directory named `-h` instead of printing help"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn cli_install_dash_h_emits_help() {
    let out = Command::new(stryke())
        .args(["install", "--help"])
        .output()
        .expect("spawn");
    assert_success("stryke install --help", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("--offline"),
        "help mentions --offline: {stdout}"
    );
}

#[test]
fn cli_add_dash_h_emits_help_with_flags() {
    let out = Command::new(stryke())
        .args(["add", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke add -h", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("--dev"), "help mentions --dev: {stdout}");
    assert!(stdout.contains("--path"), "help mentions --path: {stdout}");
    assert!(
        stdout.contains("--features"),
        "help mentions --features: {stdout}"
    );
}

#[test]
fn cli_remove_dash_h_emits_help() {
    let out = Command::new(stryke())
        .args(["remove", "--help"])
        .output()
        .expect("spawn");
    assert_success("stryke remove --help", &out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("usage: stryke remove"),
        "help text"
    );
}

#[test]
fn cli_tree_dash_h_emits_help() {
    let out = Command::new(stryke())
        .args(["tree", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke tree -h", &out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("usage: stryke tree"),
        "help text"
    );
}

#[test]
fn cli_info_dash_h_emits_help() {
    let out = Command::new(stryke())
        .args(["info", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke info -h", &out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("usage: stryke info"),
        "help text"
    );
}

#[test]
fn cli_pkg_dispatcher_help() {
    let out = Command::new(stryke())
        .args(["pkg", "-h"])
        .output()
        .expect("spawn");
    assert_success("stryke pkg -h", &out);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The dispatcher's help must list all 7 subcommands so users can discover them.
    for sub in ["init", "new", "install", "add", "remove", "tree", "info"] {
        assert!(
            stdout.contains(sub),
            "pkg help missing subcommand `{sub}`: {stdout}"
        );
    }
}

// Top-level `--help` must include every package-manager subcommand.
#[test]
fn cli_top_level_help_lists_pkg_subcommands() {
    let out = Command::new(stryke())
        .arg("--help")
        .output()
        .expect("spawn");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for sub in [
        "new ",
        "install ",
        "add NAME",
        "remove NAME",
        "tree ",
        "info NAME",
        "pkg ",
    ] {
        assert!(
            combined.contains(sub),
            "top-level --help missing subcommand fragment `{sub}`"
        );
    }
}
