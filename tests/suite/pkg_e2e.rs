//! End-to-end package-manager test: create a path-dep project, resolve it,
//! verify the lockfile + store contents, then exercise module resolution
//! programmatically (without going through the `s` CLI).
//!
//! Per-component unit tests live alongside their module
//! (`strykelang/pkg/{manifest,lockfile,store,resolver,commands}.rs`); this
//! file pins the surface contract that all five compose into a working
//! end-to-end pipeline.

use std::path::PathBuf;
use std::sync::Mutex;
use stryke::pkg::commands::{find_project_root, resolve_module};
use stryke::pkg::lockfile::{integrity_for_directory, Lockfile};
use stryke::pkg::manifest::{DepSpec, Manifest, PackageMeta};
use stryke::pkg::resolver::Resolver;
use stryke::pkg::store::Store;

/// `STRYKE_HOME` is a process-global env var that the resolver / store
/// honor for `Store::user_default()`. Any test that mutates it races
/// every other STRYKE_HOME-mutating test under `cargo test`'s default
/// parallel runner. Grab this mutex for the whole duration of such a
/// test; release happens automatically on scope exit. Mirrors the
/// `STRYKE_HOME_MUTEX` in the `pkg::commands::tests` unit module.
static STRYKE_HOME_MUTEX: Mutex<()> = Mutex::new(());

fn tempdir(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let p = std::env::temp_dir().join(format!("stryke-pkg-e2e-{}-{}-{}", tag, pid, nanos));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn make_path_dep(name: &str, version: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = tempdir(name);
    std::fs::write(
        dir.join("stryke.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\n",
            name, version
        ),
    )
    .unwrap();
    for (rel, body) in files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }
    dir
}

#[test]
fn full_pipeline_path_dep_resolves_and_locks() {
    let mylib = make_path_dep(
        "mylib",
        "1.0.0",
        &[("lib/Greet.stk", "sub greet { 'hi' }\n1\n")],
    );

    let project = tempdir("project");
    let mut manifest = Manifest {
        package: Some(PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            edition: "2026".into(),
            ..Default::default()
        }),
        ..Manifest::default()
    };
    manifest.deps.insert(
        "mylib".to_string(),
        DepSpec::path_dep(mylib.to_string_lossy().to_string()),
    );

    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &project,
        store: &store,
    };
    let outcome = r.resolve().expect("resolve");

    assert_eq!(outcome.lockfile.packages.len(), 1);
    let entry = &outcome.lockfile.packages[0];
    assert_eq!(entry.name, "mylib");
    assert_eq!(entry.version, "1.0.0");
    assert!(entry.integrity.starts_with("sha256-"));
    assert!(entry.source.starts_with("path+file://"));

    // Store contains the package layout.
    let installed = store.package_dir("mylib", "1.0.0");
    assert!(installed.is_dir());
    assert!(installed.join("lib/Greet.stk").is_file());
    assert!(installed.join("stryke.toml").is_file());

    // Integrity matches what we'd recompute.
    let recompute = integrity_for_directory(&mylib).unwrap();
    assert_eq!(entry.integrity, recompute);
}

#[test]
fn lockfile_is_byte_stable_across_resolves() {
    let mylib = make_path_dep("mylib", "1.0.0", &[("lib/X.stk", "1\n")]);
    let project = tempdir("project");
    let mut manifest = Manifest {
        package: Some(PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        }),
        ..Manifest::default()
    };
    manifest.deps.insert(
        "mylib".to_string(),
        DepSpec::path_dep(mylib.to_string_lossy().to_string()),
    );

    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &project,
        store: &store,
    };
    let mut a = r.resolve().unwrap().lockfile;
    let mut b = r.resolve().unwrap().lockfile;
    // The `resolved` timestamp drifts but everything else should match.
    a.resolved = "fixed".into();
    b.resolved = "fixed".into();
    assert_eq!(a.to_toml_string().unwrap(), b.to_toml_string().unwrap());
}

#[test]
fn registry_dep_returns_unimplemented_diagnostic() {
    let project = tempdir("project");
    let mut manifest = Manifest {
        package: Some(PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        }),
        ..Manifest::default()
    };
    manifest
        .deps
        .insert("http".to_string(), DepSpec::version("1.0"));

    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &project,
        store: &store,
    };
    let err = r.resolve().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("registry dep"), "got: {}", msg);
    assert!(msg.contains("http"), "got: {}", msg);
    assert!(msg.contains("phases 7"), "got: {}", msg);
}

#[test]
fn non_github_git_dep_rejected_with_rewrite_hint() {
    // Post-d480adc6 `s install` no longer source-clones — it fetches
    // prebuilt release tarballs from github.com. A `git = "…"` dep
    // that doesn't point at github.com must be rejected up front
    // with a hint to rewrite as `github = "OWNER/REPO"` or `path`.
    let project = tempdir("project");
    let mut manifest = Manifest {
        package: Some(PackageMeta {
            name: "myapp".into(),
            version: "0.1.0".into(),
            ..Default::default()
        }),
        ..Manifest::default()
    };
    manifest.deps.insert(
        "lib".to_string(),
        DepSpec::Detailed(stryke::pkg::manifest::DetailedDep {
            git: Some("file:///nonexistent/stryke-git-dep-test/repo.git".into()),
            default_features: true,
            ..Default::default()
        }),
    );

    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &project,
        store: &store,
    };
    let err = r.resolve().unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("github.com"), "got: {}", msg);
    assert!(msg.contains("lib"), "got: {}", msg);
    assert!(msg.contains("github = "), "rewrite hint missing; got: {}", msg);
}

#[test]
fn module_resolution_via_lockfile_finds_store_path() {
    let mylib = make_path_dep(
        "mylib",
        "1.0.0",
        &[("lib/Greet.stk", "sub greet { 'hi' }\n1\n")],
    );

    let project = tempdir("project");
    std::fs::write(
        project.join("stryke.toml"),
        format!(
            "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n\n[deps.mylib]\npath = \"{}\"\n",
            mylib.display()
        ),
    )
    .unwrap();

    // Run the resolver to populate the store + lockfile.
    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);
    let manifest = Manifest::from_path(&project.join("stryke.toml")).unwrap();
    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &project,
        store: &store,
    };
    let outcome = r.resolve().unwrap();
    let mut lf = outcome.lockfile;
    let lock_body = lf.to_toml_string().unwrap();
    std::fs::write(project.join("stryke.lock"), lock_body).unwrap();

    let resolved = resolve_module(&project, "mylib::Greet", None)
        .unwrap()
        .expect("resolved");
    let canonical_resolved = resolved.canonicalize().unwrap();
    let canonical_expected = store
        .package_dir("mylib", "1.0.0")
        .join("lib/Greet.stk")
        .canonicalize()
        .unwrap();
    assert_eq!(canonical_resolved, canonical_expected);

    // Also verify the project-local lib/ takes precedence when present.
    std::fs::create_dir_all(project.join("lib")).unwrap();
    std::fs::write(project.join("lib/Local.stk"), "1\n").unwrap();
    let local = resolve_module(&project, "Local", None)
        .unwrap()
        .expect("resolved");
    assert!(local.ends_with("lib/Local.stk"), "got {:?}", local);

    std::env::remove_var("STRYKE_HOME");
}

#[test]
fn find_project_root_walks_up_from_subdirectory() {
    let root = tempdir("walk_up");
    std::fs::write(
        root.join("stryke.toml"),
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let nested = root.join("a/b/c/d");
    std::fs::create_dir_all(&nested).unwrap();

    let canonical_root = root.canonicalize().unwrap();
    let canonical_nested = nested.canonicalize().unwrap();
    let found = find_project_root(&canonical_nested).unwrap();
    let canonical_found = found.canonicalize().unwrap();
    assert_eq!(canonical_found, canonical_root);
}

#[test]
fn missing_package_or_workspace_fails_validation() {
    let m = Manifest::from_str("").unwrap();
    assert!(m.validate().is_err());
}

/// Helper for the version-pin tests: drop a fake `<store>/<name>@<ver>/lib/<Name>.stk`
/// extraction containing the supplied body. No FFI manifest — these
/// tests exercise resolver paths only, not cdylib load.
fn install_fake_store_pkg(store: &Store, name: &str, version: &str, file_name: &str, body: &str) {
    let pkg = store.package_dir(name, version);
    std::fs::create_dir_all(pkg.join("lib")).unwrap();
    std::fs::write(pkg.join("lib").join(format!("{}.stk", file_name)), body).unwrap();
}

/// `use Module VERSION` (Perl-style pin) at the use site must override
/// the lockfile pin and land on the EXACT pinned store extraction —
/// not the lockfile-resolved version, not the global newest.
#[test]
fn use_site_pin_overrides_lockfile_inside_project() {
    let store_root = tempdir("store");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);
    install_fake_store_pkg(&store, "foo", "1.0", "Foo", "# foo v1.0\n");
    install_fake_store_pkg(&store, "foo", "2.0", "Foo", "# foo v2.0\n");

    let project = tempdir("pinproject");
    std::fs::write(
        project.join("stryke.toml"),
        "[package]\nname=\"app\"\nversion=\"0.1.0\"\n[deps]\nfoo = \"1.0\"\n",
    )
    .unwrap();
    // Lockfile pins v1.0, but use-site pin says v2.0 → v2.0 wins.
    let mut lf = Lockfile::new();
    lf.packages.push(stryke::pkg::lockfile::LockedPackage {
        name: "foo".into(),
        version: "1.0".into(),
        source: "test".into(),
        integrity: "sha256-x".into(),
        features: vec![],
        deps: vec![],
    });
    std::fs::write(project.join("stryke.lock"), lf.to_toml_string().unwrap()).unwrap();

    let resolved = resolve_module(&project, "Foo", Some("2.0"))
        .unwrap()
        .expect("use-site pin should resolve");
    assert!(
        resolved.ends_with("foo@2.0/lib/Foo.stk"),
        "use-site pin must win over lockfile pin; got {:?}",
        resolved
    );

    // Same lockfile, no pin → respects the lockfile (v1.0).
    let resolved_no_pin = resolve_module(&project, "Foo", None)
        .unwrap()
        .expect("unpinned should follow lockfile");
    assert!(
        resolved_no_pin.ends_with("foo@1.0/lib/Foo.stk"),
        "unpinned must follow stryke.lock; got {:?}",
        resolved_no_pin
    );

    std::env::remove_var("STRYKE_HOME");
}

/// Outside-project end-to-end: three versions of a package in the
/// store, no `stryke.toml`, no `installed.toml`. Resolver must pick
/// the highest semver — 2.0 beats 1.99, 0.10.0 beats 0.3.0.
#[test]
fn outside_project_resolver_picks_highest_semver() {
    let store_root = tempdir("highstore");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);
    // Lexicographic vs numeric distinction: `1.99` is text-greater
    // than `2.0`, so a lexicographic compare would pick the wrong
    // one. The resolver uses numeric tuple compare.
    for v in ["1.99", "2.0", "0.5"] {
        install_fake_store_pkg(&store, "foo", v, "Foo", &format!("# foo v{}\n", v));
    }

    let proj = tempdir("noproject");
    // No stryke.toml — outside-project path.
    let resolved = resolve_module(&proj, "Foo", None)
        .unwrap()
        .expect("outside-project should resolve to highest");
    assert!(
        resolved.ends_with("foo@2.0/lib/Foo.stk"),
        "2.0 > 1.99 numerically; got {:?}",
        resolved
    );

    std::env::remove_var("STRYKE_HOME");
}

/// Static-analyzer mirror — `resolve_require_path_from_file_versioned`
/// must find the SAME file the runtime resolver would load for a
/// `use Module VERSION` pin. Drift between the two would mean the
/// linter chases one file and the runtime loads another.
#[test]
fn analyzer_mirrors_runtime_for_use_site_pin() {
    let store_root = tempdir("mirrorstore");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);
    install_fake_store_pkg(&store, "foo", "1.0", "Foo", "# foo v1.0\n");
    install_fake_store_pkg(&store, "foo", "2.0", "Foo", "# foo v2.0\n");

    let proj = tempdir("mirrorproj");
    let script_path = proj.join("script.stk");
    std::fs::write(&script_path, "use Foo 2.0;\n").unwrap();

    let runtime_hit = resolve_module(&proj, "Foo", Some("2.0")).unwrap();
    let analyzer_hit =
        stryke::static_analysis::resolve_require_path_from_file_versioned(
            script_path.to_str().unwrap(),
            "Foo",
            Some("2.0"),
        );

    let runtime_path = runtime_hit.expect("runtime should resolve");
    let analyzer_path = analyzer_hit.expect("analyzer should resolve");
    assert_eq!(
        runtime_path.canonicalize().unwrap(),
        analyzer_path.canonicalize().unwrap(),
        "linter and runtime must point at the same file under a pin"
    );
    assert!(
        runtime_path.ends_with("foo@2.0/lib/Foo.stk"),
        "both should land on the pinned version; got {:?}",
        runtime_path
    );

    std::env::remove_var("STRYKE_HOME");
}

/// `use Module VERSION` with the namespace bridge: `use GUI 0.3.0`
/// should land on `<store>/stryke-gui@0.3.0/` because the bare `gui`
/// name doesn't have an extraction. Matches the lockfile / index
/// fallbacks' namespace-bridge behavior.
#[test]
fn use_site_pin_bridges_stryke_prefix() {
    let store_root = tempdir("bridgestore");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);
    install_fake_store_pkg(&store, "stryke-gui", "0.3.0", "GUI", "# gui v0.3.0\n");

    let proj = tempdir("bridgeproj");
    let resolved = resolve_module(&proj, "GUI", Some("0.3.0"))
        .unwrap()
        .expect("use-site pin should bridge to stryke-<name>");
    assert!(
        resolved.ends_with("stryke-gui@0.3.0/lib/GUI.stk"),
        "GUI 0.3.0 must find stryke-gui@0.3.0; got {:?}",
        resolved
    );

    std::env::remove_var("STRYKE_HOME");
}

/// LSP hover for a package name in `use Foo VERSION` must chase the
/// PINNED file in the store, not whatever the unpinned resolver
/// would have found. Without version threading in `walk_required_files`
/// the cross-file chase landed on the highest-installed file even
/// when the use-site explicitly pinned a different version — the
/// IDE then showed docs / goto-def from a file the runtime would
/// never load. Fixed by threading `Option<String>` through
/// `require_specs_from_program` and into the versioned resolver.
#[test]
fn lsp_hover_on_pinned_use_lands_on_pinned_version() {
    let store_root = tempdir("hoverpinstore");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);

    // Two versions, distinct doc strings — hover must pick the
    // pinned version's doc, not the other.
    install_fake_store_pkg(
        &store,
        "foo",
        "1.0",
        "Foo",
        "## v1.0 STALE_DOC_MARKER\npackage Foo\nfn Foo::greet { 1 }\n",
    );
    install_fake_store_pkg(
        &store,
        "foo",
        "2.13",
        "Foo",
        "## v2.13 FRESH_DOC_MARKER\npackage Foo\nfn Foo::greet { 1 }\n",
    );

    let script_dir = tempdir("hoverpinscript");
    let script_path = script_dir.join("main.stk");
    let script_src = "use Foo 2.13;\nFoo::greet();\n";
    std::fs::write(&script_path, script_src).unwrap();

    let hover = stryke::lsp::hover_markdown_for_word(
        "Foo",
        script_src,
        script_path.to_str().unwrap(),
    )
    .expect("hover should resolve Foo");
    std::env::remove_var("STRYKE_HOME");

    assert!(
        hover.contains("FRESH_DOC_MARKER"),
        "hover must surface the v2.13 doc; got: {}",
        hover
    );
    assert!(
        !hover.contains("STALE_DOC_MARKER"),
        "hover must NOT surface the v1.0 doc; got: {}",
        hover
    );
}

/// Plain `use Foo` (no pin) outside a project must hover-land on the
/// HIGHEST installed version — same contract as the runtime resolver.
/// Catches drift between what the user actually loads and what their
/// hover surfaces.
#[test]
fn lsp_hover_unpinned_use_outside_project_picks_highest_version() {
    let store_root = tempdir("hoverhigheststore");
    let store = Store::at(&store_root);
    let _g = STRYKE_HOME_MUTEX.lock().unwrap();
    std::env::set_var("STRYKE_HOME", &store_root);

    install_fake_store_pkg(
        &store,
        "bar",
        "1.0",
        "Bar",
        "## v1.0 STALE_DOC_MARKER\npackage Bar\nfn Bar::run { 1 }\n",
    );
    install_fake_store_pkg(
        &store,
        "bar",
        "2.0",
        "Bar",
        "## v2.0 FRESH_DOC_MARKER\npackage Bar\nfn Bar::run { 1 }\n",
    );

    let script_dir = tempdir("hoverhighestscript");
    let script_path = script_dir.join("main.stk");
    let script_src = "use Bar;\nBar::run();\n";
    std::fs::write(&script_path, script_src).unwrap();

    let hover = stryke::lsp::hover_markdown_for_word(
        "Bar",
        script_src,
        script_path.to_str().unwrap(),
    )
    .expect("hover should resolve Bar");
    std::env::remove_var("STRYKE_HOME");

    assert!(
        hover.contains("FRESH_DOC_MARKER"),
        "hover must surface the highest-version doc (v2.0); got: {}",
        hover
    );
    assert!(
        !hover.contains("STALE_DOC_MARKER"),
        "hover must NOT surface the older v1.0 doc; got: {}",
        hover
    );
}

#[test]
fn lockfile_canonicalize_sorts_packages_alphabetically() {
    let mut lf = Lockfile::new();
    lf.packages.push(stryke::pkg::lockfile::LockedPackage {
        name: "zeta".into(),
        version: "0.1.0".into(),
        source: "registry+https://x".into(),
        integrity: "sha256-x".into(),
        features: vec![],
        deps: vec!["beta@1.0.0".into(), "alpha@1.0.0".into()],
    });
    lf.packages.push(stryke::pkg::lockfile::LockedPackage {
        name: "alpha".into(),
        version: "1.0.0".into(),
        source: "registry+https://x".into(),
        integrity: "sha256-y".into(),
        features: vec![],
        deps: vec![],
    });
    lf.canonicalize();
    assert_eq!(lf.packages[0].name, "alpha");
    assert_eq!(lf.packages[1].name, "zeta");
    // Per-package deps are sorted too.
    assert_eq!(lf.packages[1].deps, vec!["alpha@1.0.0", "beta@1.0.0"]);
}
