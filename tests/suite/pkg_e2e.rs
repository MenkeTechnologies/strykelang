//! End-to-end package-manager test: create a path-dep project, resolve it,
//! verify the lockfile + store contents, then exercise module resolution
//! programmatically (without going through the `s` CLI).
//!
//! Per-component unit tests live alongside their module
//! (`strykelang/pkg/{manifest,lockfile,store,resolver,commands}.rs`); this
//! file pins the surface contract that all five compose into a working
//! end-to-end pipeline.

use std::path::PathBuf;
use stryke::pkg::commands::{find_project_root, resolve_module};
use stryke::pkg::lockfile::{integrity_for_directory, Lockfile};
use stryke::pkg::manifest::{DepSpec, Manifest, PackageMeta};
use stryke::pkg::resolver::Resolver;
use stryke::pkg::store::Store;

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
fn git_dep_returns_unimplemented_diagnostic() {
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
            git: Some("https://example.com/lib".into()),
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
    assert!(msg.contains("git dep"), "got: {}", msg);
    assert!(msg.contains("phase 9"), "got: {}", msg);
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

    let resolved = resolve_module(&project, "mylib::Greet")
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
    let local = resolve_module(&project, "Local")
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
