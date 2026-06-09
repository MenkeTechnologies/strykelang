//! `s` subcommand implementations for the package manager.
//!
//! Each public function returns an exit code (`i32`) so `main.rs` can wire them
//! straight into `process::exit(...)`. User-facing diagnostics go to stderr;
//! machine-readable output (e.g. `s tree`) goes to stdout.

use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use super::lockfile::Lockfile;
use super::manifest::{DepSpec, DetailedDep, Manifest, PackageMeta};
use super::resolver::Resolver;
use super::store::{InstalledIndex, Store};
use super::PkgResult;

/// Filename of the project manifest.
pub const MANIFEST_FILE: &str = "stryke.toml";
/// Filename of the project lockfile.
pub const LOCKFILE_FILE: &str = "stryke.lock";

/// Walk up from `start` to find a directory containing `stryke.toml`. Returns
/// `None` if no manifest is reachable. Used by every command that operates on a
/// project (so `s add http` works from any subdirectory).
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(MANIFEST_FILE).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// True if `arg` is a help flag (`-h` or `--help`).
fn is_help_flag(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

fn print_new_help() {
    println!("usage: stryke new NAME");
    println!();
    println!("Scaffold a new stryke project at ./NAME/. Same layout as `stryke init`,");
    println!("but creates the directory for you.");
    println!();
    println!("The new project gets:");
    println!("  NAME/stryke.toml          manifest with [package] and [bin]");
    println!("  NAME/main.stk             entry point");
    println!("  NAME/lib/                 library modules (used by `use Foo::Bar`)");
    println!("  NAME/t/                   test files (run with `s test`)");
    println!("  NAME/benches/             benchmark files (run with `s bench`)");
    println!("  NAME/bin/                 additional executables");
    println!("  NAME/examples/            example programs");
    println!("  NAME/.gitignore           ignores target/");
}

fn print_init_help() {
    println!("usage: stryke init [NAME]");
    println!();
    println!("Scaffold the current directory as a stryke project. NAME defaults to the");
    println!("cwd's basename. Writes stryke.toml + main.stk + lib/, t/, benches/, bin/,");
    println!("examples/, .gitignore. Existing files are left alone.");
}

/// `s new NAME` — scaffold a new project at `./NAME/`.
pub fn cmd_new(name: &str) -> i32 {
    if is_help_flag(name) {
        print_new_help();
        return 0;
    }
    let project_dir = PathBuf::from(name);
    if project_dir.exists() {
        eprintln!("s new: {} already exists", name);
        return 1;
    }
    if let Err(e) = std::fs::create_dir_all(&project_dir) {
        eprintln!("s new: create {}: {}", project_dir.display(), e);
        return 1;
    }
    scaffold_project(&project_dir, name)
}

/// `s init [NAME]` — scaffold the current directory as a stryke project.
/// `NAME` defaults to the current directory's basename.
pub fn cmd_init(name: Option<&str>) -> i32 {
    if matches!(name, Some(n) if is_help_flag(n)) {
        print_init_help();
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s init: cwd: {}", e);
            return 1;
        }
    };
    let resolved_name = name
        .map(|s| s.to_string())
        .or_else(|| cwd.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "stryke_project".to_string());
    scaffold_project(&cwd, &resolved_name)
}

/// Shared scaffold logic for `s new` and `s init`.
fn scaffold_project(project_dir: &Path, name: &str) -> i32 {
    let mut created: Vec<String> = Vec::new();

    let manifest_path = project_dir.join(MANIFEST_FILE);
    if !manifest_path.exists() {
        let m = default_manifest_for(name);
        let s = match m.to_toml_string() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("s init: {}", e);
                return 1;
            }
        };
        if let Err(e) = std::fs::write(&manifest_path, s) {
            eprintln!("s init: write {}: {}", manifest_path.display(), e);
            return 1;
        }
        created.push(manifest_path.display().to_string());
    }

    let main_path = project_dir.join("main.stk");
    if !main_path.exists() {
        let body = format!("#!/usr/bin/env stryke\n\np \"hello from {}!\"\n", name);
        if let Err(e) = std::fs::write(&main_path, body) {
            eprintln!("s init: write {}: {}", main_path.display(), e);
            return 1;
        }
        created.push(main_path.display().to_string());
    }

    for sub in ["lib", "t", "benches", "bin", "examples"] {
        let d = project_dir.join(sub);
        if !d.exists() {
            if let Err(e) = std::fs::create_dir_all(&d) {
                eprintln!("s init: mkdir {}: {}", d.display(), e);
                return 1;
            }
            created.push(format!("{}/", d.display()));
        }
    }

    let test_path = project_dir.join("t/test_main.stk");
    if !test_path.exists() {
        let body = "#!/usr/bin/env stryke\n\nuse Test\n\nok 1, \"it works\"\n\ndone_testing()\n";
        if let Err(e) = std::fs::write(&test_path, body) {
            eprintln!("s init: write {}: {}", test_path.display(), e);
            return 1;
        }
        created.push(test_path.display().to_string());
    }

    let gi = project_dir.join(".gitignore");
    if !gi.exists() {
        let body = "# stryke build artifacts\n/target/\n";
        if let Err(e) = std::fs::write(&gi, body) {
            eprintln!("s init: write {}: {}", gi.display(), e);
            return 1;
        }
        created.push(gi.display().to_string());
    }

    for c in &created {
        eprintln!("  created {}", c);
    }
    eprintln!("\x1b[32m✓ Initialized stryke project `{}`\x1b[0m", name);
    eprintln!();
    eprintln!("  s install    # populate stryke.lock from stryke.toml");
    eprintln!("  s run        # run main.stk");
    eprintln!("  s test       # run tests in t/");
    0
}

fn default_manifest_for(name: &str) -> Manifest {
    let mut bin = IndexMap::new();
    bin.insert(name.to_string(), "main.stk".to_string());
    Manifest {
        package: Some(PackageMeta {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            license: String::new(),
            repository: String::new(),
            edition: "2026".to_string(),
        }),
        bin,
        ..Manifest::default()
    }
}

/// `s add NAME[@VER] [--dev|--group=NAME] [--path=...]` — append a dep to
/// `stryke.toml` and re-run install. Idempotent on the manifest level: adding
/// the same dep twice updates the version in place rather than duplicating.
pub fn cmd_add(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!(
            "usage: stryke add NAME[@VER] [--dev | --group=NAME] [--path=DIR] [--features=A,B]"
        );
        println!();
        println!("Add a dependency to stryke.toml and run `s install` to refresh stryke.lock.");
        println!();
        println!("Flags:");
        println!("  --dev            add as a [dev-deps] entry instead of [deps]");
        println!("  --group=NAME     add to [groups.NAME] (bundler-style)");
        println!("  --path=DIR       depend on a local checkout (no registry needed)");
        println!("  --features=A,B   enable feature flags A and B for this dep");
        println!();
        println!("Examples:");
        println!("  stryke add http@1.0");
        println!("  stryke add test-utils --dev");
        println!("  stryke add criterion --group=bench");
        println!("  stryke add mylib --path=../mylib");
        return 0;
    }
    let parsed = match parse_add_args(args) {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("s add: {}", msg);
            return 1;
        }
    };

    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s add: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s add: no stryke.toml found in this directory or any parent");
            return 1;
        }
    };

    let manifest_path = root.join(MANIFEST_FILE);
    let mut manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s add: {}", e);
            return 1;
        }
    };

    let target_map: &mut IndexMap<String, DepSpec> = match &parsed.kind {
        AddKind::Runtime => &mut manifest.deps,
        AddKind::Dev => &mut manifest.dev_deps,
        AddKind::Group(g) => manifest.groups.entry(g.clone()).or_default(),
    };
    target_map.insert(parsed.name.clone(), parsed.spec.clone());

    let body = match manifest.to_toml_string() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s add: {}", e);
            return 1;
        }
    };
    if let Err(e) = std::fs::write(&manifest_path, body) {
        eprintln!("s add: write {}: {}", manifest_path.display(), e);
        return 1;
    }
    eprintln!(
        "  added {}{} = {}",
        parsed.name,
        match &parsed.kind {
            AddKind::Runtime => "".to_string(),
            AddKind::Dev => " (dev)".to_string(),
            AddKind::Group(g) => format!(" (group:{})", g),
        },
        format_dep_for_log(&parsed.spec)
    );

    // Re-run install so the lockfile catches up. Failure here is reported but
    // doesn't roll back the manifest edit — the user can fix the dep and rerun.
    cmd_install(&[])
}

struct AddArgs {
    name: String,
    spec: DepSpec,
    kind: AddKind,
}

enum AddKind {
    Runtime,
    Dev,
    Group(String),
}

fn parse_add_args(args: &[String]) -> Result<AddArgs, String> {
    if args.is_empty() {
        return Err("usage: s add NAME[@VER] [--dev|--group=NAME] [--path=DIR]".into());
    }
    let mut positional: Vec<&String> = Vec::new();
    let mut kind = AddKind::Runtime;
    let mut path_override: Option<String> = None;
    let mut features: Vec<String> = Vec::new();
    for a in args {
        match a.as_str() {
            "--dev" => kind = AddKind::Dev,
            s if s.starts_with("--group=") => {
                kind = AddKind::Group(s["--group=".len()..].to_string())
            }
            s if s.starts_with("--path=") => path_override = Some(s["--path=".len()..].to_string()),
            s if s.starts_with("--features=") => {
                features = s["--features=".len()..]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            s if s.starts_with("--") => {
                return Err(format!("unknown flag {}", s));
            }
            _ => positional.push(a),
        }
    }
    if positional.len() != 1 {
        return Err(format!(
            "expected exactly one NAME[@VER] argument, got {}",
            positional.len()
        ));
    }
    let raw = positional[0].as_str();
    let (name, version) = match raw.split_once('@') {
        Some((n, v)) => (n.to_string(), Some(v.to_string())),
        None => (raw.to_string(), None),
    };
    let spec = if let Some(p) = path_override {
        DepSpec::Detailed(DetailedDep {
            path: Some(p),
            version,
            features,
            default_features: true,
            ..DetailedDep::default()
        })
    } else if !features.is_empty() {
        DepSpec::Detailed(DetailedDep {
            version: Some(version.clone().unwrap_or_else(|| "*".to_string())),
            features,
            default_features: true,
            ..DetailedDep::default()
        })
    } else {
        DepSpec::Version(version.unwrap_or_else(|| "*".to_string()))
    };
    Ok(AddArgs { name, spec, kind })
}

fn format_dep_for_log(spec: &DepSpec) -> String {
    match spec {
        DepSpec::Version(v) => format!("\"{}\"", v),
        DepSpec::Detailed(d) => {
            let mut bits = Vec::new();
            if let Some(v) = &d.version {
                bits.push(format!("version = \"{}\"", v));
            }
            if let Some(p) = &d.path {
                bits.push(format!("path = \"{}\"", p));
            }
            if let Some(g) = &d.git {
                bits.push(format!("git = \"{}\"", g));
            }
            if !d.features.is_empty() {
                bits.push(format!("features = {:?}", d.features));
            }
            format!("{{ {} }}", bits.join(", "))
        }
        DepSpec::Placeholder => "<placeholder>".into(),
    }
}

/// `s remove NAME` — drop the dep from `stryke.toml` and re-run install.
pub fn cmd_remove(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke remove NAME");
        println!();
        println!("Drop NAME from stryke.toml ([deps], [dev-deps], or [groups.*]) and");
        println!("rerun `s install` so stryke.lock matches.");
        return 0;
    }
    if args.len() != 1 {
        eprintln!("usage: s remove NAME");
        return 1;
    }
    let name = &args[0];
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s remove: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s remove: no stryke.toml found in this directory or any parent");
            return 1;
        }
    };
    let manifest_path = root.join(MANIFEST_FILE);
    let mut manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s remove: {}", e);
            return 1;
        }
    };
    let mut removed = false;
    if manifest.deps.shift_remove(name).is_some() {
        removed = true;
    }
    if manifest.dev_deps.shift_remove(name).is_some() {
        removed = true;
    }
    for (_g, group_map) in manifest.groups.iter_mut() {
        if group_map.shift_remove(name).is_some() {
            removed = true;
        }
    }
    if !removed {
        eprintln!("s remove: `{}` is not a direct dep", name);
        return 1;
    }
    let body = match manifest.to_toml_string() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s remove: {}", e);
            return 1;
        }
    };
    if let Err(e) = std::fs::write(&manifest_path, body) {
        eprintln!("s remove: write {}: {}", manifest_path.display(), e);
        return 1;
    }
    eprintln!("  removed {}", name);
    cmd_install(&[])
}

/// `s install [--offline]` — resolve manifest, install path/workspace deps into
/// the store, write `stryke.lock`. Registry/git deps return a clear error since
/// the wire protocol isn't wired yet (RFC phases 7-8).
pub fn cmd_install(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke install [--offline]");
        println!();
        println!("Resolve manifest deps, install path/workspace deps into ~/.stryke/store/,");
        println!("and write stryke.lock with deterministic ordering + SHA-256 integrity hashes.");
        println!();
        println!("Flags:");
        println!("  --offline    only use cached packages; never fetch from the network");
        return 0;
    }
    let _offline = args.iter().any(|a| a == "--offline");

    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s install: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s install: no stryke.toml found in this directory or any parent");
            return 1;
        }
    };

    let manifest_path = root.join(MANIFEST_FILE);
    let manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s install: {}", e);
            return 1;
        }
    };
    if let Err(e) = manifest.validate() {
        eprintln!("s install: {}", e);
        return 1;
    }

    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s install: {}", e);
            return 1;
        }
    };

    let r = Resolver {
        manifest: &manifest,
        manifest_dir: &root,
        store: &store,
    };
    let outcome = match r.resolve() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("s install: {}", e);
            return 1;
        }
    };

    if outcome.installed.is_empty() {
        eprintln!("  no deps to install");
    } else {
        for (name, version, _path) in &outcome.installed {
            eprintln!("  installed {}@{}", name, version);
        }
    }

    let mut lf = outcome.lockfile;
    let body = match lf.to_toml_string() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s install: {}", e);
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if let Err(e) = std::fs::write(&lock_path, body) {
        eprintln!("s install: write {}: {}", lock_path.display(), e);
        return 1;
    }
    eprintln!(
        "\x1b[32m✓ wrote {} ({} package{})\x1b[0m",
        lock_path.display(),
        lf.packages.len(),
        if lf.packages.len() == 1 { "" } else { "s" }
    );
    0
}

/// `s tree` — print the resolved dep graph from the lockfile in a human-friendly
/// format. Roots are the direct deps from `stryke.toml`; transitive deps render
/// indented underneath. Cycles are not possible (resolver rejects them).
pub fn cmd_tree(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke tree");
        println!();
        println!("Print the resolved dependency graph from stryke.lock as a tree, with the");
        println!("project at the root and direct + transitive deps underneath.");
        println!();
        println!("Run `s install` first to generate stryke.lock.");
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s tree: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s tree: no stryke.toml found");
            return 1;
        }
    };
    let manifest = match Manifest::from_path(&root.join(MANIFEST_FILE)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s tree: {}", e);
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if !lock_path.is_file() {
        eprintln!("s tree: stryke.lock not found — run `s install` first");
        return 1;
    }
    let lock = match Lockfile::from_path(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("s tree: {}", e);
            return 1;
        }
    };

    let pkg_label = manifest
        .package
        .as_ref()
        .map(|p| format!("{} v{}", p.name, p.version))
        .unwrap_or_else(|| "(workspace)".to_string());
    println!("{}", pkg_label);

    let direct_names: Vec<String> = manifest
        .deps
        .keys()
        .chain(manifest.dev_deps.keys())
        .chain(manifest.groups.values().flat_map(|g| g.keys()))
        .cloned()
        .collect();

    for (i, dep_name) in direct_names.iter().enumerate() {
        let last = i + 1 == direct_names.len();
        print_tree_entry(&lock, dep_name, "", last);
    }
    0
}

fn print_tree_entry(lock: &Lockfile, name: &str, prefix: &str, last: bool) {
    let connector = if last { "└── " } else { "├── " };
    let next_prefix = if last { "    " } else { "│   " };
    match lock.find(name) {
        Some(entry) => {
            println!("{}{}{} v{}", prefix, connector, entry.name, entry.version);
            for (i, dep_pin) in entry.deps.iter().enumerate() {
                let dep_name = dep_pin.split_once('@').map(|(n, _)| n).unwrap_or(dep_pin);
                let last_child = i + 1 == entry.deps.len();
                print_tree_entry(
                    lock,
                    dep_name,
                    &format!("{}{}", prefix, next_prefix),
                    last_child,
                );
            }
        }
        None => {
            println!("{}{}{} (not in lockfile)", prefix, connector, name);
        }
    }
}

/// `s info NAME` — print the manifest of an installed package from the store.
/// Reads `~/.stryke/store/NAME@VERSION/stryke.toml` (resolved via current
/// project's lockfile) and pretty-prints the metadata.
pub fn cmd_info(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke info NAME");
        println!();
        println!("Print the lockfile entry and store path for an installed dep. Shows name,");
        println!("version, source URL, integrity hash, enabled features, and transitive deps.");
        println!();
        println!("Run `s install` first to generate stryke.lock.");
        return 0;
    }
    if args.len() != 1 {
        eprintln!("usage: s info NAME");
        return 1;
    }
    let name = &args[0];
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s info: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s info: no stryke.toml found");
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if !lock_path.is_file() {
        eprintln!("s info: stryke.lock not found — run `s install` first");
        return 1;
    }
    let lock = match Lockfile::from_path(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("s info: {}", e);
            return 1;
        }
    };
    let entry = match lock.find(name) {
        Some(e) => e,
        None => {
            eprintln!("s info: `{}` is not in stryke.lock", name);
            return 1;
        }
    };
    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s info: {}", e);
            return 1;
        }
    };
    let pkg_dir = store.package_dir(&entry.name, &entry.version);
    println!("name:       {}", entry.name);
    println!("version:    {}", entry.version);
    println!("source:     {}", entry.source);
    println!("integrity:  {}", entry.integrity);
    if !entry.features.is_empty() {
        println!("features:   {}", entry.features.join(", "));
    }
    if !entry.deps.is_empty() {
        println!("deps:       {}", entry.deps.join(", "));
    }
    println!("store path: {}", pkg_dir.display());
    let nested_manifest = pkg_dir.join(MANIFEST_FILE);
    if nested_manifest.is_file() {
        if let Ok(m) = Manifest::from_path(&nested_manifest) {
            if let Some(meta) = &m.package {
                if !meta.description.is_empty() {
                    println!("description: {}", meta.description);
                }
                if !meta.license.is_empty() {
                    println!("license:    {}", meta.license);
                }
                if !meta.repository.is_empty() {
                    println!("repo:       {}", meta.repository);
                }
            }
        }
    }
    0
}

/// Programmatic entry point: load manifest + lockfile from a project root.
/// Used by module resolution to translate `use Foo::Bar` → store path.
pub fn load_project(root: &Path) -> PkgResult<(Manifest, Option<Lockfile>)> {
    let manifest = Manifest::from_path(&root.join(MANIFEST_FILE))?;
    let lock_path = root.join(LOCKFILE_FILE);
    let lockfile = if lock_path.is_file() {
        Some(Lockfile::from_path(&lock_path)?)
    } else {
        None
    };
    Ok((manifest, lockfile))
}

/// Module resolution helper — given a project root and a logical module name
/// like `"Foo::Bar"`, return the `.stk` source path if any of:
/// 1. `<root>/lib/Foo/Bar.stk` exists.
/// 2. The lockfile has an entry for the lower-cased first segment (`foo`),
///    and `~/.stryke/store/foo@VERSION/lib/Bar.stk` exists.
///
/// Returns `Ok(None)` if neither resolved (caller falls through to `@INC`).
pub fn resolve_module(root: &Path, logical_name: &str) -> PkgResult<Option<PathBuf>> {
    let segments: Vec<&str> = logical_name.split("::").collect();
    if segments.is_empty() {
        return Ok(None);
    }

    // 1. Project-local `lib/`.
    let local = root.join("lib").join(segments_to_path(&segments));
    if local.is_file() {
        // Project may declare [ffi] for its own cdylib in stryke.toml.
        try_load_ffi_for(root)?;
        return Ok(Some(local));
    }

    // 2. Lockfile-driven store lookup. Use the (lower-cased) first segment as
    //    the package name; remaining segments become the in-package path.
    let lock_path = root.join(LOCKFILE_FILE);
    if lock_path.is_file() {
        let lock = Lockfile::from_path(&lock_path)?;
        let pkg_name = segments[0].to_lowercase();
        if let Some(entry) = lock.find(&pkg_name) {
            let store = Store::user_default()?;
            let store_pkg = store.package_dir(&entry.name, &entry.version);
            let nested_path = if segments.len() == 1 {
                store_pkg.join("lib").join(format!("{}.stk", segments[0]))
            } else {
                store_pkg.join("lib").join(segments_to_path(&segments[1..]))
            };
            if nested_path.is_file() {
                try_load_ffi_for(&store_pkg)?;
                return Ok(Some(nested_path));
            }
        }
    }

    // 3. Global pin file. `~/.stryke/installed.toml` records every
    //    `s pkg install -g`-installed package. When the script runs outside
    //    a project (no walking up to a stryke.toml) or the project's lock
    //    doesn't pin this package, look it up here. This is the path that
    //    makes `s install -g github.com/MenkeTechnologies/stryke-gui` →
    //    standalone `use GUI` work.
    if let Ok(idx) = InstalledIndex::load_or_default() {
        let pkg_name = segments[0].to_lowercase();
        if let Some(entry) = idx.find(&pkg_name) {
            let store = Store::user_default()?;
            let store_pkg = store.package_dir(&entry.name, &entry.version);
            let nested_path = if segments.len() == 1 {
                store_pkg.join("lib").join(format!("{}.stk", segments[0]))
            } else {
                store_pkg.join("lib").join(segments_to_path(&segments[1..]))
            };
            if nested_path.is_file() {
                try_load_ffi_for(&store_pkg)?;
                return Ok(Some(nested_path));
            }
        }
    }

    Ok(None)
}

/// Side-load the package's `[ffi]` cdylib (if declared) into the FFI registry.
/// Idempotent across repeat `use NAMESPACE` calls within one process —
/// [`crate::rust_ffi::load_cdylib`] short-circuits when the lib was already
/// loaded. Returns `Ok(())` for packages without an `[ffi]` section.
fn try_load_ffi_for(pkg_dir: &Path) -> PkgResult<()> {
    let manifest_path = pkg_dir.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(());
    }
    let manifest = Manifest::from_path(&manifest_path)?;
    let Some(ffi) = manifest.ffi else {
        return Ok(());
    };
    if ffi.lib_name.is_empty() || ffi.exports.is_empty() {
        return Ok(());
    }
    let lib_filename = format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        ffi.lib_name,
        std::env::consts::DLL_SUFFIX
    );
    // Search order:
    //   1. lib/<lib_filename>    — production layout (release tarball drop).
    //   2. target/release/...    — dev layout after `cargo build --release`.
    //   3. target/debug/...      — dev layout after `cargo build`.
    // Letting `target/` also satisfy the lookup lets contributors iterate on
    // a stryke-* package without re-running `s pkg install -g .` after every
    // edit. Production installs always hit (1) because the release tarball
    // ships the cdylib at lib/.
    let candidates = [
        pkg_dir.join("lib").join(&lib_filename),
        pkg_dir.join("target").join("release").join(&lib_filename),
        pkg_dir.join("target").join("debug").join(&lib_filename),
    ];
    let lib_path = match candidates.iter().find(|p| p.is_file()) {
        Some(p) => p.clone(),
        None => {
            return Err(super::PkgError::Other(format!(
                "[ffi] cdylib `{}` not found under {}/lib or {}/target/{{release,debug}}/ \
                 — install with `s pkg install -g github.com/...` to fetch the prebuilt \
                 artifact, or run `cargo build --release` in the package dir for dev",
                lib_filename,
                pkg_dir.display(),
                pkg_dir.display()
            )));
        }
    };
    crate::rust_ffi::load_cdylib(&lib_path, &ffi.exports, 0)
        .map_err(|e| super::PkgError::Other(format!("[ffi] load {}: {}", lib_path.display(), e)))
}

fn segments_to_path(segments: &[&str]) -> PathBuf {
    let mut p = PathBuf::new();
    for (i, seg) in segments.iter().enumerate() {
        if i + 1 == segments.len() {
            p.push(format!("{}.stk", seg));
        } else {
            p.push(seg);
        }
    }
    p
}

/// `s clean` — wipe `target/` plus the per-project bytecode cache. Global
/// `~/.stryke/cache/` is preserved unless `--all` is passed (which also nukes
/// the store). Path-dep installs in `~/.stryke/store/` are kept by default
/// because they're trivially regenerated, but the user may not expect a
/// global wipe just from `s clean`.
pub fn cmd_clean(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke clean [--all]");
        println!();
        println!("Remove the local target/ directory and per-project bytecode cache.");
        println!();
        println!("Flags:");
        println!("  --all    additionally clear ~/.stryke/cache/ and ~/.stryke/store/");
        return 0;
    }
    let want_global = args.iter().any(|a| a == "--all");

    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s clean: cwd: {}", e);
            return 1;
        }
    };
    let root = find_project_root(&cwd).unwrap_or(cwd);
    let mut wiped: Vec<String> = Vec::new();
    for sub in ["target", ".stryke-cache"] {
        let d = root.join(sub);
        if d.exists() {
            if let Err(e) = std::fs::remove_dir_all(&d) {
                eprintln!("s clean: remove {}: {}", d.display(), e);
                return 1;
            }
            wiped.push(d.display().to_string());
        }
    }

    if want_global {
        if let Ok(store) = Store::user_default() {
            for d in [store.cache_dir(), store.store_dir(), store.git_dir()] {
                if d.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&d) {
                        eprintln!("s clean: remove {}: {}", d.display(), e);
                        return 1;
                    }
                    wiped.push(d.display().to_string());
                }
            }
        }
    }

    if wiped.is_empty() {
        eprintln!("  nothing to clean");
    } else {
        for w in &wiped {
            eprintln!("  removed {}", w);
        }
    }
    0
}

/// `s update [NAME]` — re-resolve the manifest and overwrite `stryke.lock`.
/// Today, with only path/workspace deps wired, this is `s install` with the
/// existing lockfile thrown out first. When the registry resolver lands, this
/// is where semver-aware version bumps will live.
pub fn cmd_update(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke update [NAME]");
        println!();
        println!("Re-resolve the dependency graph and rewrite stryke.lock. With registry");
        println!("deps unwired, this currently re-pins path/workspace dep integrity hashes.");
        println!();
        println!("NAME: when given, only that dep is re-resolved (others stay pinned).");
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s update: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s update: no stryke.toml found");
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if lock_path.exists() {
        if let Err(e) = std::fs::remove_file(&lock_path) {
            eprintln!("s update: remove {}: {}", lock_path.display(), e);
            return 1;
        }
    }
    eprintln!("  re-resolving dependency graph");
    cmd_install(&[])
}

/// `s outdated` — compare every dep's lockfile pin against its current
/// upstream state. For path deps that means rehashing the source dir; if the
/// integrity hash drifted, the dep is "outdated." Registry deps return a
/// "registry not wired" notice rather than silent green.
pub fn cmd_outdated(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke outdated");
        println!();
        println!("Show deps whose stryke.lock pin no longer matches the upstream state.");
        println!("Path deps: integrity hash recomputed against the source directory.");
        println!("Registry deps: not wired in this stryke version.");
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s outdated: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s outdated: no stryke.toml found");
            return 1;
        }
    };
    let manifest = match Manifest::from_path(&root.join(MANIFEST_FILE)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s outdated: {}", e);
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if !lock_path.is_file() {
        eprintln!("s outdated: stryke.lock not found — run `s install` first");
        return 1;
    }
    let lock = match Lockfile::from_path(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("s outdated: {}", e);
            return 1;
        }
    };

    let mut drifted: Vec<String> = Vec::new();
    let mut registry_skipped: Vec<String> = Vec::new();
    for (name, spec) in manifest.deps.iter() {
        if let Some(p) = spec.path() {
            let abs = if std::path::Path::new(p).is_absolute() {
                std::path::PathBuf::from(p)
            } else {
                root.join(p)
            };
            if let Ok(now) = super::lockfile::integrity_for_directory(&abs) {
                if let Some(entry) = lock.find(name) {
                    if entry.integrity != now {
                        drifted.push(format!(
                            "  {}@{}  pinned {}  current {}",
                            name, entry.version, entry.integrity, now
                        ));
                    }
                } else {
                    drifted.push(format!(
                        "  {} (path)  not in lockfile — run `s install`",
                        name
                    ));
                }
            }
        } else {
            registry_skipped.push(name.clone());
        }
    }

    if drifted.is_empty() && registry_skipped.is_empty() {
        eprintln!("\x1b[32m✓ all path deps are up to date\x1b[0m");
        return 0;
    }
    if !drifted.is_empty() {
        eprintln!("path deps with drift (run `s install` to re-pin):");
        for d in &drifted {
            eprintln!("{}", d);
        }
    }
    if !registry_skipped.is_empty() {
        eprintln!(
            "registry deps skipped — wire protocol not deployed yet ({}): {}",
            registry_skipped.len(),
            registry_skipped.join(", ")
        );
    }
    0
}

/// `s audit` — check the lockfile against a known-vulnerability advisory feed.
/// The feed itself is not deployed yet; today the command parses the lockfile,
/// reports the dep count, and emits an honest "no advisories — feed not yet
/// deployed" message rather than fake "you're safe" output.
pub fn cmd_audit(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke audit [--fail-on=high|critical]");
        println!();
        println!("Check stryke.lock against a vulnerability advisory feed. The feed itself");
        println!("is not deployed yet — this command currently reports the dep count and");
        println!("emits an honest 'no advisories' message rather than faking it.");
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s audit: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s audit: no stryke.toml found");
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if !lock_path.is_file() {
        eprintln!("s audit: stryke.lock not found — run `s install` first");
        return 1;
    }
    let lock = match Lockfile::from_path(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("s audit: {}", e);
            return 1;
        }
    };
    eprintln!(
        "  audited {} package{}",
        lock.packages.len(),
        if lock.packages.len() == 1 { "" } else { "s" }
    );
    eprintln!("\x1b[33m  advisory feed not yet deployed — no vulnerabilities reported\x1b[0m");
    0
}

/// `s run SCRIPT [ARGS...]` — npm-style task runner. Looks up SCRIPT in the
/// `[scripts]` table of the project's `stryke.toml` and executes it via the
/// system shell so pipes/redirects work. Any extra ARGS are appended.
///
/// This is distinct from the existing `stryke run main.stk` semantic: that
/// path runs a `.stk` file directly. Script names from `[scripts]` win when
/// both are possible (the user's manifest is authoritative).
pub fn cmd_run_script(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke run SCRIPT [ARGS...]");
        println!();
        println!("Look up SCRIPT in the [scripts] table of stryke.toml and execute it via");
        println!("the system shell. Any ARGS are appended to the script command line.");
        println!();
        println!("Without [scripts], `stryke run` falls back to running ./main.stk directly.");
        return 0;
    }
    if args.is_empty() {
        eprintln!("usage: s run SCRIPT [ARGS...]");
        return 1;
    }
    let script = &args[0];
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s run: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s run: no stryke.toml found in this directory or any parent");
            return 1;
        }
    };
    let manifest = match Manifest::from_path(&root.join(MANIFEST_FILE)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s run: {}", e);
            return 1;
        }
    };
    let cmd = match manifest.scripts.get(script) {
        Some(c) => c.clone(),
        None => {
            eprintln!("s run: no script `{}` in [scripts]", script);
            if !manifest.scripts.is_empty() {
                eprintln!(
                    "available: {}",
                    manifest
                        .scripts
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            return 1;
        }
    };
    let extra = &args[1..];
    let full = if extra.is_empty() {
        cmd.clone()
    } else {
        format!(
            "{} {}",
            cmd,
            extra
                .iter()
                .map(|a| shell_escape_simple(a))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    eprintln!("  $ {}", full);
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let status = std::process::Command::new(&shell)
        .arg("-c")
        .arg(&full)
        .current_dir(&root)
        .status();
    match status {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("s run: spawn {}: {}", shell, e);
            1
        }
    }
}

/// Minimal shell quoting — wrap in single quotes and escape any inner quotes.
fn shell_escape_simple(s: &str) -> String {
    if !s.contains(' ')
        && !s.contains('\'')
        && !s.contains('"')
        && !s.contains('$')
        && !s.contains('`')
    {
        return s.to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

/// `s vendor` — copy every dep in `stryke.lock` from the global store into
/// `./vendor/<name>@<version>/` so the project builds with `--offline` even
/// on a machine without `~/.stryke/store/` populated. Existing `vendor/`
/// content is replaced.
pub fn cmd_vendor(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke vendor");
        println!();
        println!("Copy every dep in stryke.lock from ~/.stryke/store/ into ./vendor/ so");
        println!("the project is offline-buildable. Useful for shipping a tarball that");
        println!("builds without registry access.");
        return 0;
    }
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s vendor: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s vendor: no stryke.toml found");
            return 1;
        }
    };
    let lock_path = root.join(LOCKFILE_FILE);
    if !lock_path.is_file() {
        eprintln!("s vendor: stryke.lock not found — run `s install` first");
        return 1;
    }
    let lock = match Lockfile::from_path(&lock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("s vendor: {}", e);
            return 1;
        }
    };
    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s vendor: {}", e);
            return 1;
        }
    };

    let vendor_dir = root.join("vendor");
    if vendor_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&vendor_dir) {
            eprintln!("s vendor: clear {}: {}", vendor_dir.display(), e);
            return 1;
        }
    }
    if let Err(e) = std::fs::create_dir_all(&vendor_dir) {
        eprintln!("s vendor: mkdir {}: {}", vendor_dir.display(), e);
        return 1;
    }

    let mut copied = 0_usize;
    for pkg in &lock.packages {
        let src = store.package_dir(&pkg.name, &pkg.version);
        if !src.is_dir() {
            eprintln!(
                "s vendor: {}@{} not in store — run `s install` first",
                pkg.name, pkg.version
            );
            return 1;
        }
        let dst = vendor_dir.join(format!("{}@{}", pkg.name, pkg.version));
        if let Err(e) = copy_tree(&src, &dst) {
            eprintln!("s vendor: copy {}: {}", src.display(), e);
            return 1;
        }
        copied += 1;
    }
    eprintln!(
        "\x1b[32m✓ vendored {} package{} into {}\x1b[0m",
        copied,
        if copied == 1 { "" } else { "s" },
        vendor_dir.display()
    );
    0
}

/// Recursive directory copy used by `s vendor`. Mirrors the resolver's logic
/// but lives here to keep it private to vendor (no symlinks-as-symlinks
/// requirement — vendor is a flat snapshot).
fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = entry.metadata()?;
        if meta.is_dir() {
            copy_tree(&from, &to)?;
        } else if meta.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let target = std::fs::read_link(&from)?;
                std::os::unix::fs::symlink(target, &to)?;
            }
            #[cfg(not(unix))]
            std::fs::copy(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// `s install -g PATH` — install a path-based package's `[bin]` entries into
/// `~/.stryke/bin/` as shebang wrappers. No registry needed — works today for
/// any local package with a manifest declaring binaries.
pub fn cmd_install_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) || args.is_empty() {
        println!("usage: stryke install -g SPEC");
        println!();
        println!("SPEC is one of:");
        println!("  PATH                                local dir with stryke.toml");
        println!("  gh:owner/repo[@VERSION]             GitHub release (prebuilt)");
        println!("  github.com/owner/repo[@VERSION]     GitHub release (prebuilt)");
        println!("  https://github.com/owner/repo[@VERSION]");
        println!();
        println!("Local installs copy the source into ~/.stryke/store/<name>@<version>/.");
        println!("GitHub installs download the prebuilt release tarball for the host");
        println!("triple (override with STRYKE_TARGET=...), verify its SHA-256, and");
        println!("extract into the store. Launchers in ~/.stryke/bin/ point at the");
        println!("store entry. When the package declares [ffi], its cdylib loads");
        println!("lazily on first `use <namespace>`.");
        return if args.is_empty() { 1 } else { 0 };
    }

    let spec = match InstallSpec::parse(&args[0]) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("s install -g: {}", msg);
            return 1;
        }
    };

    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s install -g: {}", e);
            return 1;
        }
    };
    if let Err(e) = store.ensure_layout() {
        eprintln!("s install -g: {}", e);
        return 1;
    }

    let (manifest, store_pkg_dir, source) = match spec {
        InstallSpec::Path(p) => match install_global_from_path(&store, &p) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("s install -g: {}", msg);
                return 1;
            }
        },
        InstallSpec::GitHub {
            owner,
            repo,
            version,
        } => match install_global_from_github(&store, &owner, &repo, version.as_deref()) {
            Ok(v) => v,
            Err(msg) => {
                eprintln!("s install -g: {}", msg);
                return 1;
            }
        },
    };

    // Launchers from [bin]. FFI-only packages may have empty [bin] — fine,
    // they're invoked via `use <namespace>` not via a CLI launcher.
    for (bin_name, entry) in &manifest.bin {
        let target = store_pkg_dir.join(entry);
        if !target.is_file() {
            eprintln!(
                "s install -g: bin `{}` -> {} does not exist",
                bin_name,
                target.display()
            );
            return 1;
        }
        let launcher = store.bin_dir().join(bin_name);
        if let Err(e) = write_launcher(&launcher, &target) {
            eprintln!("s install -g: write {}: {}", launcher.display(), e);
            return 1;
        }
        eprintln!("  installed {} -> {}", launcher.display(), target.display());
    }

    // Pin the install in ~/.stryke/installed.toml so standalone scripts
    // (no project dir) can resolve `use <namespace>` to this store entry
    // — that's the resolution path resolve_module's third arm walks.
    if let Some(pkg) = manifest.package.as_ref() {
        let mut idx = match InstalledIndex::load_or_default() {
            Ok(i) => i,
            Err(e) => {
                eprintln!("s install -g: load installed.toml: {}", e);
                return 1;
            }
        };
        // Warn loud when replacing a different pinned version so the user
        // notices the old store dir is now an orphan. Same-version reinstall
        // is silent (idempotent re-runs are normal).
        if let Some(prev) = idx.find(&pkg.name) {
            if prev.version != pkg.version {
                let old_dir = if let Ok(s) = Store::user_default() {
                    s.package_dir(&pkg.name, &prev.version)
                } else {
                    std::path::PathBuf::from(format!("{}@{}", pkg.name, prev.version))
                };
                eprintln!(
                    "  \x1b[33mreplacing pinned {} {} → {}\x1b[0m  ({} kept on disk; run `s pkg gc -g` to free)",
                    pkg.name,
                    prev.version,
                    pkg.version,
                    old_dir.display()
                );
            }
        }
        idx.upsert(&pkg.name, &pkg.version, &source);
        if let Err(e) = idx.save() {
            eprintln!("s install -g: write installed.toml: {}", e);
            return 1;
        }
    }

    let pkg_label = manifest
        .package
        .as_ref()
        .map(|p| format!("{}@{}", p.name, p.version))
        .unwrap_or_else(|| "package".to_string());
    let mode_label = if manifest.ffi.is_some() {
        " [ffi cdylib]"
    } else {
        ""
    };
    eprintln!(
        "\x1b[32m✓ {} installed{} → {}\x1b[0m",
        pkg_label,
        mode_label,
        store_pkg_dir.display()
    );
    if !manifest.bin.is_empty() {
        eprintln!("  (add {} to PATH)", store.bin_dir().display());
    }
    0
}

/// Parsed argument to `s install -g SPEC`.
enum InstallSpec {
    /// Local directory containing `stryke.toml`. Source-copied into the store.
    Path(PathBuf),
    /// GitHub release. Prebuilt tarball downloaded per host triple.
    GitHub {
        owner: String,
        repo: String,
        /// Tag to install (`v0.2.0`, `0.2.0`, etc.). `None` means latest.
        version: Option<String>,
    },
}

impl InstallSpec {
    fn parse(arg: &str) -> Result<InstallSpec, String> {
        let (head, version) = split_version_suffix(arg);
        if let Some(rest) = head.strip_prefix("gh:") {
            let (owner, repo) = parse_gh_owner_repo(rest)?;
            return Ok(InstallSpec::GitHub {
                owner,
                repo,
                version,
            });
        }
        for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
            if let Some(rest) = head.strip_prefix(prefix) {
                let (owner, repo) = parse_gh_owner_repo(rest)?;
                return Ok(InstallSpec::GitHub {
                    owner,
                    repo,
                    version,
                });
            }
        }
        let path = PathBuf::from(head);
        if !path.is_dir() {
            return Err(format!(
                "`{}` is not a directory or a recognized GitHub spec",
                head
            ));
        }
        Ok(InstallSpec::Path(path))
    }
}

/// Split `gh:foo/bar@v1.0` into `("gh:foo/bar", Some("v1.0"))`. The `@` only
/// counts as a version separator when followed by a digit or `v` so paths
/// containing `@` (e.g. `~/projects/team@2023/pkg`) are not misparsed.
fn split_version_suffix(arg: &str) -> (&str, Option<String>) {
    if let Some(at_pos) = arg.rfind('@') {
        let v = &arg[at_pos + 1..];
        let looks_like_version = !v.is_empty()
            && (v.starts_with('v') || v.chars().next().map_or(false, |c| c.is_ascii_digit()));
        if looks_like_version {
            return (&arg[..at_pos], Some(v.to_string()));
        }
    }
    (arg, None)
}

fn parse_gh_owner_repo(s: &str) -> Result<(String, String), String> {
    let cleaned = s.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = cleaned.splitn(2, '/');
    let owner = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or("expected owner/repo")?;
    let repo = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or("expected owner/repo")?;
    Ok((owner.to_string(), repo.to_string()))
}

fn install_global_from_path(
    store: &Store,
    src: &Path,
) -> Result<(Manifest, PathBuf, String), String> {
    let abs = src
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {}", src.display(), e))?;
    let manifest = Manifest::from_path(&abs.join(MANIFEST_FILE)).map_err(|e| e.to_string())?;
    let pkg = manifest
        .package
        .as_ref()
        .ok_or("manifest missing [package]")?;
    let dst = store
        .install_path_dep(&pkg.name, &pkg.version, &abs)
        .map_err(|e| e.to_string())?;
    let source = format!("path+file://{}", abs.display());
    Ok((manifest, dst, source))
}

fn install_global_from_github(
    store: &Store,
    owner: &str,
    repo: &str,
    requested_version: Option<&str>,
) -> Result<(Manifest, PathBuf, String), String> {
    let tag = match requested_version {
        Some(v) => v.to_string(),
        None => fetch_latest_release_tag(owner, repo)?,
    };
    let triple = host_target_triple()?;
    let asset_stem = repo.to_lowercase();
    let asset = format!("{}-{}-{}.tar.gz", asset_stem, tag, triple);
    let url_tar = format!(
        "https://github.com/{}/{}/releases/download/{}/{}",
        owner, repo, tag, asset
    );
    let url_sha = format!("{}.sha256", url_tar);

    eprintln!("  fetching {} ...", url_tar);

    let sha_text = ureq::get(&url_sha)
        .call()
        .map_err(|e| format!("GET {}: {}", url_sha, e))?
        .into_string()
        .map_err(|e| format!("read {}: {}", url_sha, e))?;
    let expected_sha = sha_text
        .split_whitespace()
        .next()
        .ok_or("empty sha256 file")?
        .to_string();

    use std::io::Read;
    let mut bytes = Vec::new();
    ureq::get(&url_tar)
        .call()
        .map_err(|e| format!("GET {}: {}", url_tar, e))?
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read {}: {}", url_tar, e))?;

    use sha2::Digest as _;
    let actual = hex::encode(sha2::Sha256::digest(&bytes));
    if !expected_sha.eq_ignore_ascii_case(&actual) {
        return Err(format!(
            "sha256 mismatch on {}: expected {} got {}",
            asset, expected_sha, actual
        ));
    }

    // Stage under cache/ then rename into store/ atomically so a failure
    // mid-extract doesn't leave a half-populated store entry.
    let stage_dir = store
        .cache_dir()
        .join(format!("install-stage-{}-{}-{}", asset_stem, tag, triple));
    if stage_dir.exists() {
        std::fs::remove_dir_all(&stage_dir)
            .map_err(|e| format!("clear {}: {}", stage_dir.display(), e))?;
    }
    std::fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("mkdir {}: {}", stage_dir.display(), e))?;
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes));
    tar::Archive::new(decoder)
        .unpack(&stage_dir)
        .map_err(|e| format!("extract {}: {}", asset, e))?;

    let manifest_dir = locate_manifest_dir(&stage_dir)?;
    let manifest = Manifest::from_path(&manifest_dir.join(MANIFEST_FILE))
        .map_err(|e| format!("installed stryke.toml: {}", e))?;
    let pkg = manifest
        .package
        .as_ref()
        .ok_or("installed manifest missing [package]")?;

    let dst = store.package_dir(&pkg.name, &pkg.version);
    if dst.exists() {
        std::fs::remove_dir_all(&dst)
            .map_err(|e| format!("clear existing store entry {}: {}", dst.display(), e))?;
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::rename(&manifest_dir, &dst).map_err(|e| {
        format!(
            "rename {} -> {}: {}",
            manifest_dir.display(),
            dst.display(),
            e
        )
    })?;
    let _ = std::fs::remove_dir_all(&stage_dir);

    let source = format!("github:{}/{}@{}", owner, repo, tag);
    Ok((manifest, dst, source))
}

/// GET `https://api.github.com/repos/{owner}/{repo}/releases/latest` and pull
/// the `tag_name` field. Returns the tag verbatim — `v0.2.0`, `0.2.0`, etc.
fn fetch_latest_release_tag(owner: &str, repo: &str) -> Result<String, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        owner, repo
    );
    let resp = ureq::get(&url)
        .set("User-Agent", "stryke-pkg")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("GET {}: {}", url, e))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("parse {}: {}", url, e))?;
    v.get("tag_name")
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| format!("no tag_name in {} response", url))
}

/// Host triple used to pick which release asset to download. Uses
/// `STRYKE_TARGET` when set (escape hatch for musl, cross builds, exotic
/// architectures). The default mapping covers the four triples that every
/// stryke-* package's release CI must publish.
fn host_target_triple() -> Result<String, String> {
    if let Ok(t) = std::env::var("STRYKE_TARGET") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let triple = match (arch, os) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        _ => {
            return Err(format!(
                "no prebuilt asset triple for host {}-{}; set STRYKE_TARGET=... to override",
                arch, os
            ));
        }
    };
    Ok(triple.to_string())
}

/// The release tarball convention is `tar czf - <pkgdir>/`, which produces an
/// archive with one top-level directory. Some packages publish a flat tarball
/// (manifest at the root) instead; we accept both shapes.
fn locate_manifest_dir(root: &Path) -> Result<PathBuf, String> {
    if root.join(MANIFEST_FILE).is_file() {
        return Ok(root.to_path_buf());
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| format!("read {}: {}", root.display(), e))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().is_dir() {
            subdirs.push(entry.path());
        }
    }
    if subdirs.len() != 1 {
        return Err(format!(
            "expected stryke.toml at tarball root or a single top-level dir in {}, found {} dirs",
            root.display(),
            subdirs.len()
        ));
    }
    let s = subdirs.remove(0);
    if !s.join(MANIFEST_FILE).is_file() {
        return Err(format!("no stryke.toml in {}", s.display()));
    }
    Ok(s)
}

/// Write a `#!/bin/sh` launcher that invokes `stryke <abs_target> "$@"`. We
/// don't symlink the .stk source because the launcher needs to call the
/// interpreter — symlinking the .stk would make the file appear as the
/// "binary" but `./~/.stryke/bin/foo` would just dump perl source.
fn write_launcher(
    launcher_path: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    if launcher_path.exists() {
        std::fs::remove_file(launcher_path)?;
    }
    let body = format!(
        "#!/bin/sh\nexec stryke {:?} \"$@\"\n",
        target.display().to_string()
    );
    std::fs::write(launcher_path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(launcher_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(launcher_path, perms)?;
    }
    Ok(())
}

/// `s uninstall -g NAME` — remove a package installed by `s install -g`.
/// Drops the global pin in `~/.stryke/installed.toml` (so `use NAME` from a
/// standalone script no longer resolves), removes every launcher in
/// `~/.stryke/bin/` declared by the package's manifest, and leaves the
/// store entry in place (re-install hits the same path without re-fetching).
pub fn cmd_uninstall_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) || args.is_empty() {
        println!("usage: stryke uninstall -g NAME");
        println!();
        println!("Remove a package installed by `stryke install -g`. NAME is the");
        println!("package name (the `[package].name` field of its stryke.toml, or");
        println!("equivalently a launcher in ~/.stryke/bin/). Drops the entry from");
        println!("~/.stryke/installed.toml and removes any associated launchers.");
        return if args.is_empty() { 1 } else { 0 };
    }
    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s uninstall -g: {}", e);
            return 1;
        }
    };
    let name = &args[0];

    // Pull the pin first so we can find every [bin] launcher to remove.
    let mut idx = match InstalledIndex::load_or_default() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s uninstall -g: load installed.toml: {}", e);
            return 1;
        }
    };
    let index_entry = idx.find(name).cloned();
    let mut removed_anything = false;

    if let Some(entry) = index_entry {
        let store_pkg = store.package_dir(&entry.name, &entry.version);
        let manifest_path = store_pkg.join(MANIFEST_FILE);
        if manifest_path.is_file() {
            if let Ok(m) = Manifest::from_path(&manifest_path) {
                for bin_name in m.bin.keys() {
                    let launcher = store.bin_dir().join(bin_name);
                    if launcher.exists() {
                        match std::fs::remove_file(&launcher) {
                            Ok(_) => {
                                eprintln!("  removed launcher {}", launcher.display());
                                removed_anything = true;
                            }
                            Err(e) => {
                                eprintln!(
                                    "s uninstall -g: remove {}: {}",
                                    launcher.display(),
                                    e
                                );
                                return 1;
                            }
                        }
                    }
                }
            }
        }
        idx.remove(name);
        if let Err(e) = idx.save() {
            eprintln!("s uninstall -g: write installed.toml: {}", e);
            return 1;
        }
        eprintln!("  unpinned {} (store entry kept at {})", name, store_pkg.display());
        removed_anything = true;
    } else {
        // No pin — fall back to single-launcher removal for packages that
        // pre-date the installed-index machinery.
        let target = store.bin_dir().join(name);
        if target.exists() {
            if let Err(e) = std::fs::remove_file(&target) {
                eprintln!("s uninstall -g: remove {}: {}", target.display(), e);
                return 1;
            }
            eprintln!("  removed launcher {}", target.display());
            removed_anything = true;
        }
    }

    if !removed_anything {
        eprintln!("s uninstall -g: {} not installed", name);
        return 1;
    }
    0
}

/// `s use -g NAME@VERSION` — switch which version of an already-installed
/// package the global pin points at, without re-fetching. Errors loud when
/// the requested store dir doesn't exist — the user must `install -g` first.
///
/// Lets users keep multiple side-by-side versions on disk and flip between
/// them for standalone scripts. Inside a project the lockfile still wins
/// over the global pin, so this has no effect on project-scoped resolution.
pub fn cmd_use_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) || args.is_empty() {
        println!("usage: stryke use -g NAME@VERSION");
        println!();
        println!("Switch the global pin in ~/.stryke/installed.toml to a different");
        println!("version of an already-installed package. The version must already");
        println!("exist as ~/.stryke/store/<name>@<version>/ — this command does NOT");
        println!("download anything; use `s pkg install -g <spec>@<version>` to fetch.");
        println!();
        println!("Standalone scripts that `use <Namespace>` will resolve to this");
        println!("version. Inside a project the stryke.lock still wins.");
        return if args.is_empty() { 1 } else { 0 };
    }
    let spec = &args[0];
    let (name, version) = match spec.split_once('@') {
        Some((n, v)) if !n.is_empty() && !v.is_empty() => (n.to_string(), v.to_string()),
        _ => {
            eprintln!("s use -g: spec must be NAME@VERSION, got `{}`", spec);
            return 1;
        }
    };

    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s use -g: {}", e);
            return 1;
        }
    };
    let store_pkg = store.package_dir(&name, &version);
    if !store_pkg.is_dir() {
        eprintln!(
            "s use -g: {}@{} is not installed (no {} directory). \
             Run `s pkg install -g <spec>@{}` to fetch it first.",
            name,
            version,
            store_pkg.display(),
            version
        );
        return 1;
    }

    let mut idx = match InstalledIndex::load_from(&store) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s use -g: load installed.toml: {}", e);
            return 1;
        }
    };
    let previous = idx.find(&name).map(|p| p.version.clone());
    let source = idx
        .find(&name)
        .map(|p| p.source.clone())
        .unwrap_or_else(|| format!("local-pin:store/{}@{}", name, version));
    idx.upsert(&name, &version, &source);
    if let Err(e) = idx.save_to(&store) {
        eprintln!("s use -g: write installed.toml: {}", e);
        return 1;
    }

    match previous {
        Some(prev) if prev != version => {
            eprintln!(
                "\x1b[32m✓ {} pin {} → {}\x1b[0m  ({} still on disk; run `s pkg gc -g` to free it)",
                name,
                prev,
                version,
                store.package_dir(&name, &prev).display()
            );
        }
        Some(_) => {
            eprintln!(
                "\x1b[32m✓ {} pin already at {}\x1b[0m",
                name, version
            );
        }
        None => {
            eprintln!(
                "\x1b[32m✓ pinned {}@{}\x1b[0m  (was not in installed.toml)",
                name, version
            );
        }
    }
    0
}

/// `s gc -g [--dry-run]` — remove every `~/.stryke/store/<name>@<version>/`
/// not currently pinned in `installed.toml`. Returns the total bytes freed.
/// `--dry-run` reports what would be removed without touching disk.
///
/// Project lockfiles can pin versions that the global index does not — but
/// only the active project's lockfile is reachable from here (no way to find
/// every checkout on disk). Run this from outside any project, or accept
/// that store entries needed by a project lockfile may be deleted and have
/// to be re-fetched by `s install` next time.
pub fn cmd_gc_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke gc -g [--dry-run]");
        println!();
        println!("Remove every ~/.stryke/store/<name>@<version>/ directory not pinned");
        println!("by ~/.stryke/installed.toml. Project lockfiles are not consulted —");
        println!("a store entry deleted here will be re-fetched on the next `s install`");
        println!("inside a project that pinned it.");
        println!();
        println!("Flags:");
        println!("  --dry-run   list what would be removed without touching disk");
        return 0;
    }
    let dry_run = args.iter().any(|a| a == "--dry-run" || a == "-n");

    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s gc -g: {}", e);
            return 1;
        }
    };
    let idx = match InstalledIndex::load_from(&store) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s gc -g: load installed.toml: {}", e);
            return 1;
        }
    };

    let orphans = scan_orphan_store_dirs(&store, &idx);
    if orphans.is_empty() {
        eprintln!("  no orphan store entries");
        return 0;
    }

    let mut total_bytes: u64 = 0;
    let mut total_count: usize = 0;
    for (name, version, path) in &orphans {
        let bytes = dir_size_recursive(path);
        total_bytes += bytes;
        total_count += 1;
        if dry_run {
            eprintln!(
                "  would remove {}@{}  ({} KB)",
                name,
                version,
                (bytes + 512) / 1024
            );
        } else {
            match std::fs::remove_dir_all(path) {
                Ok(_) => eprintln!(
                    "  removed {}@{}  ({} KB)",
                    name,
                    version,
                    (bytes + 512) / 1024
                ),
                Err(e) => {
                    eprintln!("s gc -g: remove {}: {}", path.display(), e);
                    return 1;
                }
            }
        }
    }
    let verb = if dry_run { "would free" } else { "freed" };
    eprintln!(
        "\x1b[32m✓ {} {} orphan{} ({} KB total)\x1b[0m",
        verb,
        total_count,
        if total_count == 1 { "" } else { "s" },
        (total_bytes + 512) / 1024
    );
    0
}

/// Sum every regular file under `path` recursively. Used by `s gc -g` so its
/// output names how much disk each removal frees. Symlinks are not followed
/// (matches the install side's `copy_dir` symlink-preserves behavior).
fn dir_size_recursive(path: &Path) -> u64 {
    let mut total: u64 = 0;
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size_recursive(&entry.path());
        } else if meta.is_file() {
            total += meta.len();
        }
    }
    total
}

/// `s list -g` — list every launcher in `~/.stryke/bin/`.
pub fn cmd_list_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke list -g");
        println!();
        println!("Three sections of global state, all under ~/.stryke/:");
        println!("  packages  — entries in installed.toml (the pin source-of-truth)");
        println!("  launchers — files in bin/ (created by install -g on packages with [bin])");
        println!("  orphans   — store/<name>@<ver>/ dirs not pinned by installed.toml");
        println!();
        println!("Run `s pkg gc -g` to remove orphans.");
        return 0;
    }
    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s list -g: {}", e);
            return 1;
        }
    };
    let idx = match InstalledIndex::load_from(&store) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s list -g: load installed.toml: {}", e);
            return 1;
        }
    };

    // ── packages: every pinned entry ──
    println!("packages:");
    if idx.packages.is_empty() {
        println!("  (none)");
    } else {
        for pkg in &idx.packages {
            println!("  {}@{}  {}", pkg.name, pkg.version, pkg.source);
        }
    }

    // ── launchers: files in bin/ (FFI-only packages may have none) ──
    println!("launchers:");
    let bin_dir = store.bin_dir();
    let mut launcher_names: Vec<String> = Vec::new();
    if bin_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&bin_dir) {
            for entry in entries.flatten() {
                if let Some(n) = entry.file_name().to_str() {
                    launcher_names.push(n.to_string());
                }
            }
        }
    }
    launcher_names.sort();
    if launcher_names.is_empty() {
        println!("  (none)");
    } else {
        for n in &launcher_names {
            println!("  {}", n);
        }
    }

    // ── orphans: store/<name>@<ver>/ not matching any pin ──
    println!("orphans:");
    let orphans = scan_orphan_store_dirs(&store, &idx);
    if orphans.is_empty() {
        println!("  (none)");
    } else {
        for (name, version, _path) in &orphans {
            println!("  {}@{}", name, version);
        }
    }
    0
}

/// Return every `~/.stryke/store/<name>@<version>/` whose `(name, version)` is
/// not the currently-pinned entry in [`InstalledIndex`]. A store dir whose
/// name isn't in the index at all is also an orphan. Used by `s pkg list -g`
/// and `s pkg gc -g`.
fn scan_orphan_store_dirs(store: &Store, idx: &InstalledIndex) -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    let store_dir = store.store_dir();
    let entries = match std::fs::read_dir(&store_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.flatten() {
        let name = match e.file_name().to_str().map(|s| s.to_string()) {
            Some(n) => n,
            None => continue,
        };
        let (pkg_name, version) = match name.split_once('@') {
            Some((n, v)) => (n.to_string(), v.to_string()),
            None => continue, // malformed entry — skip rather than crash
        };
        let pinned = idx.find(&pkg_name).map(|p| &p.version);
        let is_pinned = pinned.map(|v| v == &version).unwrap_or(false);
        if !is_pinned {
            out.push((pkg_name, version, e.path()));
        }
    }
    out.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));
    out
}

/// `s search NAME` — registry-dependent, not deployed yet. Honest stub so the
/// CLI shape matches the RFC. When the registry endpoint exists, this hits
/// the `/api/v1/index/{name}` path and prints matches.
pub fn cmd_search(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke search NAME");
        println!();
        println!("Query the stryke registry for packages matching NAME. The registry");
        println!("endpoint is not deployed yet — this command emits a clear diagnostic");
        println!("rather than silent failure.");
        return 0;
    }
    if args.is_empty() {
        eprintln!("usage: s search NAME");
        return 1;
    }
    eprintln!(
        "s search: registry endpoint not deployed yet (RFC §\"Registry Protocol\"). \
         Query was `{}`.",
        args[0]
    );
    1
}

/// `s publish` — registry-dependent stub. When the registry exists, this
/// reads the manifest, packages the source as a tarball, computes the
/// integrity hash, and POSTs to `/api/v1/packages/{name}/{version}`.
pub fn cmd_publish(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke publish [--registry=URL] [--dry-run]");
        println!();
        println!("Package the project as a tarball and push to the stryke registry. The");
        println!("registry endpoint is not deployed yet — this command currently performs");
        println!("the local pack step (under --dry-run) and stops.");
        return 0;
    }
    let dry_run = args.iter().any(|a| a == "--dry-run");
    if !dry_run {
        eprintln!(
            "s publish: registry endpoint not deployed yet (RFC §\"Registry Protocol\"). \
             Pass --dry-run to exercise the local pack step."
        );
        return 1;
    }
    // Dry-run: validate the manifest and report what would be sent.
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s publish: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s publish: no stryke.toml found");
            return 1;
        }
    };
    let manifest = match Manifest::from_path(&root.join(MANIFEST_FILE)) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s publish: {}", e);
            return 1;
        }
    };
    if let Err(e) = manifest.validate() {
        eprintln!("s publish: {}", e);
        return 1;
    }
    let pkg = match manifest.package.as_ref() {
        Some(p) => p,
        None => {
            eprintln!("s publish: workspace roots can't be published — pick a member");
            return 1;
        }
    };
    let integrity = match super::lockfile::integrity_for_directory(&root) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s publish: hash {}: {}", root.display(), e);
            return 1;
        }
    };
    eprintln!("  would publish {} v{}", pkg.name, pkg.version);
    eprintln!("  source dir: {}", root.display());
    eprintln!("  integrity:  {}", integrity);
    eprintln!("  (dry run — no upload performed)");
    0
}

/// `s yank VERSION` — registry-dependent stub. When the registry exists, this
/// POSTs to `/api/v1/packages/{name}/{version}/yank`.
pub fn cmd_yank(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke yank VERSION");
        println!();
        println!("Mark a published version as do-not-resolve. Registry endpoint not");
        println!("deployed yet — this command emits a clear diagnostic rather than");
        println!("silent failure. Yanked versions are never deleted (immutable registry).");
        return 0;
    }
    if args.is_empty() {
        eprintln!("usage: s yank VERSION");
        return 1;
    }
    eprintln!(
        "s yank: registry endpoint not deployed yet (RFC §\"Registry Protocol\"). \
         Version was `{}`.",
        args[0]
    );
    1
}

/// Convenience wrapper: route a top-level `s pkg <subcommand>` invocation. Not
/// the primary surface (each subcommand is wired individually in `main.rs`),
/// but useful when porting from prototype shells.
pub fn dispatch(args: &[String]) -> i32 {
    let want_help = args.first().map(|a| is_help_flag(a)).unwrap_or(false);
    if args.is_empty() || want_help {
        println!("usage: stryke pkg <subcommand> [args]");
        println!();
        println!("Package-manager subcommand dispatcher. The same handlers are also");
        println!("reachable as top-level commands (e.g. `stryke install` ≡ `stryke pkg install`).");
        println!();
        println!("Subcommands:");
        println!("  init [NAME]               scaffold project in cwd");
        println!("  new NAME                  scaffold project at ./NAME/");
        println!("  install [--offline]       resolve deps + write stryke.lock (alias: i)");
        println!("  install -g SPEC           install a package globally (PATH | gh:owner/repo | github.com/...)");
        println!("  uninstall -g NAME         drop a global pin + its launchers (alias: un)");
        println!("  use -g NAME@VERSION       switch which installed version a standalone `use` resolves to");
        println!("  list -g                   list global packages, launchers, and orphans (alias: ls)");
        println!("  gc -g [--dry-run]         delete ~/.stryke/store/ entries no longer pinned");
        println!("  add NAME[@VER] [...]      add a dep to stryke.toml");
        println!("  remove NAME               drop a dep from stryke.toml");
        println!("  update [NAME]             re-resolve and rewrite stryke.lock (alias: up, upgrade)");
        println!("  outdated                  report deps drifted from their lock pin");
        println!("  audit                     check lockfile against advisory feed");
        println!("  tree                      print resolved dep graph");
        println!("  info NAME                 show lockfile entry for a dep");
        println!("  vendor                    snapshot store deps to ./vendor/");
        println!("  clean [--all]             wipe target/ (and optionally global caches)");
        println!("  search NAME               registry query (registry not deployed)");
        println!("  publish [--dry-run]       publish to registry (registry not deployed) (alias: pub)");
        println!("  yank VERSION              yank a version (registry not deployed)");
        println!("  run SCRIPT [ARGS...]      run a [scripts] entry");
        println!();
        println!("Run `stryke <subcommand> -h` for per-subcommand flags.");
        return if args.is_empty() { 1 } else { 0 };
    }
    match args[0].as_str() {
        "init" => cmd_init(args.get(1).map(|s| s.as_str())),
        "new" => match args.get(1) {
            Some(name) => cmd_new(name),
            None => {
                eprintln!("usage: s pkg new NAME");
                1
            }
        },
        "add" => cmd_add(&args[1..]),
        "remove" => cmd_remove(&args[1..]),
        // `s i` is a convenience alias for `s install` (matches `cargo install`'s
        // implicit shorthand). All flag behavior (`-g`, `--offline`, etc.) is
        // identical — we just dispatch to the same handler.
        "install" | "i" => {
            // Detect `-g` for global install; falls through to lock-driven install otherwise.
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_install_global(&filtered)
            } else {
                cmd_install(&args[1..])
            }
        }
        "uninstall" | "un" => {
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_uninstall_global(&filtered)
            } else {
                eprintln!("s uninstall: pass -g for global tools (no per-project uninstall yet)");
                1
            }
        }
        "list" | "ls" => {
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_list_global(&filtered)
            } else {
                eprintln!("s list: pass -g to list global tools");
                1
            }
        }
        "use" => {
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_use_global(&filtered)
            } else {
                eprintln!("s use: pass -g to switch a global pin (no per-project `use` yet)");
                1
            }
        }
        "gc" => {
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_gc_global(&filtered)
            } else {
                eprintln!("s gc: pass -g to garbage-collect orphan global store entries");
                1
            }
        }
        "tree" => cmd_tree(&args[1..]),
        "info" => cmd_info(&args[1..]),
        "update" | "up" | "upgrade" => cmd_update(&args[1..]),
        "outdated" => cmd_outdated(&args[1..]),
        "audit" => cmd_audit(&args[1..]),
        "vendor" => cmd_vendor(&args[1..]),
        "clean" => cmd_clean(&args[1..]),
        "search" => cmd_search(&args[1..]),
        "publish" | "pub" => cmd_publish(&args[1..]),
        "yank" => cmd_yank(&args[1..]),
        "run" => cmd_run_script(&args[1..]),
        other => {
            eprintln!("s pkg: unknown subcommand `{}`", other);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that mutate `STRYKE_HOME` are serialized through this mutex —
    /// the env var is process-global, so parallel test execution would race
    /// (one test's clearing wipes another test's setting mid-run). Grab the
    /// guard at the top of any STRYKE_HOME-touching test; drop it via scope.
    static STRYKE_HOME_MUTEX: Mutex<()> = Mutex::new(());

    fn tempdir(tag: &str) -> PathBuf {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let p = std::env::temp_dir().join(format!("stryke-cmd-{}-{}-{}", tag, pid, nanos));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn try_load_ffi_for_noop_without_section() {
        let d = tempdir("ffi-noop");
        std::fs::write(
            d.join(MANIFEST_FILE),
            "[package]\nname=\"plain\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        // No [ffi] table — must succeed silently.
        try_load_ffi_for(&d).unwrap();
    }

    #[test]
    fn try_load_ffi_for_missing_manifest_is_ok() {
        // Bare store entry (e.g. mid-install partial state) — no manifest.
        // Must not panic; FFI load is a no-op for plain dirs.
        let d = tempdir("ffi-no-manifest");
        try_load_ffi_for(&d).unwrap();
    }

    #[test]
    fn try_load_ffi_for_errors_when_lib_missing() {
        let d = tempdir("ffi-missing-lib");
        std::fs::write(
            d.join(MANIFEST_FILE),
            "[package]\nname=\"gui\"\nversion=\"0.1.0\"\n\n\
             [ffi]\nlib-name=\"stryke_gui\"\nnamespace=\"GUI\"\nexports=[\"gui__mouse_pos\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join("lib")).unwrap();
        let err = try_load_ffi_for(&d).unwrap_err();
        // Error must mention the platform-specific filename and point at the
        // search locations so the user knows what to install / build.
        let msg = format!("{}", err);
        let expected_filename = format!(
            "{}{}{}",
            std::env::consts::DLL_PREFIX,
            "stryke_gui",
            std::env::consts::DLL_SUFFIX
        );
        assert!(
            msg.contains(&expected_filename),
            "expected {} in: {}",
            expected_filename,
            msg
        );
        assert!(msg.contains("not found"), "got: {}", msg);
        assert!(msg.contains("install"), "got: {}", msg);
    }

    #[test]
    fn scan_orphan_store_dirs_flags_unpinned() {
        let root = tempdir("scan-orphans");
        let store = Store::at(&root);
        store.ensure_layout().unwrap();
        for v in ["0.1.0", "0.2.0", "0.3.0"] {
            std::fs::create_dir_all(store.package_dir("gui", v)).unwrap();
        }
        std::fs::create_dir_all(store.package_dir("aws", "0.1.0")).unwrap();
        let mut idx = InstalledIndex::new();
        // Pin gui@0.2.0 and aws@0.1.0. Expect gui@0.1.0 and gui@0.3.0 as orphans.
        idx.upsert("gui", "0.2.0", "test");
        idx.upsert("aws", "0.1.0", "test");

        let orphans = scan_orphan_store_dirs(&store, &idx);
        let names: Vec<(String, String)> = orphans
            .into_iter()
            .map(|(n, v, _)| (n, v))
            .collect();
        assert_eq!(
            names,
            vec![
                ("gui".to_string(), "0.1.0".to_string()),
                ("gui".to_string(), "0.3.0".to_string()),
            ]
        );
    }

    #[test]
    fn scan_orphan_store_dirs_flags_unindexed_names() {
        // A store dir whose name isn't in the index at all counts as an orphan.
        let root = tempdir("scan-orphans-unindexed");
        let store = Store::at(&root);
        store.ensure_layout().unwrap();
        std::fs::create_dir_all(store.package_dir("stale-pkg", "1.0.0")).unwrap();
        let idx = InstalledIndex::new();
        let orphans = scan_orphan_store_dirs(&store, &idx);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].0, "stale-pkg");
    }

    #[test]
    fn cmd_use_global_switches_pin() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let root = tempdir("use-g-switch");
        std::env::set_var("STRYKE_HOME", &root);
        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        std::fs::create_dir_all(store.package_dir("gui", "0.1.0")).unwrap();
        std::fs::create_dir_all(store.package_dir("gui", "0.2.0")).unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.1.0", "test");
        idx.save_to(&store).unwrap();

        let rc = cmd_use_global(&["gui@0.2.0".to_string()]);
        let reloaded = InstalledIndex::load_from(&store).unwrap();
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(rc, 0);
        assert_eq!(reloaded.find("gui").unwrap().version, "0.2.0");
    }

    #[test]
    fn cmd_use_global_errors_when_store_missing() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let root = tempdir("use-g-missing");
        std::env::set_var("STRYKE_HOME", &root);
        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Only 0.1.0 exists. Asking for 0.2.0 must fail without writing.
        std::fs::create_dir_all(store.package_dir("gui", "0.1.0")).unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.1.0", "test");
        idx.save_to(&store).unwrap();

        let rc = cmd_use_global(&["gui@0.2.0".to_string()]);
        let reloaded = InstalledIndex::load_from(&store).unwrap();
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(rc, 1, "must fail when the version isn't in the store");
        assert_eq!(reloaded.find("gui").unwrap().version, "0.1.0", "pin must be untouched");
    }

    #[test]
    fn cmd_use_global_rejects_bad_spec() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let root = tempdir("use-g-bad-spec");
        std::env::set_var("STRYKE_HOME", &root);
        let rc = cmd_use_global(&["gui-without-version".to_string()]);
        std::env::remove_var("STRYKE_HOME");
        assert_eq!(rc, 1);
    }

    #[test]
    fn cmd_gc_global_removes_orphans_only() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let root = tempdir("gc-g-removes");
        std::env::set_var("STRYKE_HOME", &root);
        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        for v in ["0.1.0", "0.2.0"] {
            std::fs::create_dir_all(store.package_dir("gui", v)).unwrap();
            std::fs::write(store.package_dir("gui", v).join("marker"), b"x").unwrap();
        }
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.2.0", "test");
        idx.save_to(&store).unwrap();

        let rc = cmd_gc_global(&[]);
        let pinned_exists = store.package_dir("gui", "0.2.0").is_dir();
        let orphan_gone = !store.package_dir("gui", "0.1.0").exists();
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(rc, 0);
        assert!(pinned_exists, "pinned version must remain");
        assert!(orphan_gone, "orphan must be removed");
    }

    #[test]
    fn cmd_gc_global_dry_run_keeps_files() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let root = tempdir("gc-g-dry");
        std::env::set_var("STRYKE_HOME", &root);
        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        std::fs::create_dir_all(store.package_dir("gui", "0.1.0")).unwrap();
        // Empty index → both store dirs are orphans, but --dry-run must not delete.
        InstalledIndex::new().save_to(&store).unwrap();

        let rc = cmd_gc_global(&["--dry-run".to_string()]);
        let still_present = store.package_dir("gui", "0.1.0").is_dir();
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(rc, 0);
        assert!(still_present, "--dry-run must not touch disk");
    }

    #[test]
    fn installed_index_round_trip_via_save_load_from() {
        // Verify the explicit-store API end-to-end at the commands layer
        // (the version of this test in store.rs covers the load/save path
        // directly; this one verifies upsert + find as the resolver uses
        // them). Both avoid STRYKE_HOME so parallel runs don't race.
        let root = tempdir("installed-index-cmds");
        let store = Store::at(&root);
        store.ensure_layout().unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.2.0", "github:MenkeTechnologies/stryke-gui");
        idx.save_to(&store).unwrap();
        let reloaded = InstalledIndex::load_from(&store).unwrap();
        assert_eq!(reloaded.find("gui").unwrap().version, "0.2.0");
    }

    #[test]
    fn install_spec_parses_gh_short() {
        match InstallSpec::parse("gh:MenkeTechnologies/stryke-gui").unwrap() {
            InstallSpec::GitHub {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "MenkeTechnologies");
                assert_eq!(repo, "stryke-gui");
                assert!(version.is_none());
            }
            _ => panic!("expected GitHub"),
        }
    }

    #[test]
    fn install_spec_parses_https_with_version() {
        match InstallSpec::parse("https://github.com/foo/bar@v1.2.3").unwrap() {
            InstallSpec::GitHub {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "foo");
                assert_eq!(repo, "bar");
                assert_eq!(version.as_deref(), Some("v1.2.3"));
            }
            _ => panic!("expected GitHub"),
        }
    }

    #[test]
    fn install_spec_strips_git_suffix() {
        match InstallSpec::parse("github.com/o/r.git").unwrap() {
            InstallSpec::GitHub { owner, repo, .. } => {
                assert_eq!(owner, "o");
                assert_eq!(repo, "r");
            }
            _ => panic!("expected GitHub"),
        }
    }

    #[test]
    fn install_spec_unknown_scheme_is_path_if_dir() {
        let d = tempdir("install-spec-path");
        match InstallSpec::parse(d.to_str().unwrap()).unwrap() {
            InstallSpec::Path(p) => assert_eq!(p, PathBuf::from(d.to_str().unwrap())),
            _ => panic!("expected Path"),
        }
    }

    #[test]
    fn install_spec_unknown_scheme_errors_if_not_dir() {
        let r = InstallSpec::parse("/definitely/not/a/dir/here");
        assert!(r.is_err());
    }

    #[test]
    fn split_version_suffix_basic() {
        assert_eq!(
            split_version_suffix("gh:foo/bar@v1.0"),
            ("gh:foo/bar", Some("v1.0".into()))
        );
        assert_eq!(
            split_version_suffix("gh:foo/bar@0.2.0"),
            ("gh:foo/bar", Some("0.2.0".into()))
        );
        assert_eq!(split_version_suffix("gh:foo/bar"), ("gh:foo/bar", None));
    }

    #[test]
    fn split_version_suffix_keeps_non_version_at() {
        // `@scope/pkg` style — not a version, do not split.
        assert_eq!(
            split_version_suffix("/Users/wizard/projects/team@alpha/pkg"),
            ("/Users/wizard/projects/team@alpha/pkg", None)
        );
    }

    #[test]
    fn host_target_triple_honors_env() {
        std::env::set_var("STRYKE_TARGET", "x86_64-unknown-linux-musl");
        let t = host_target_triple().unwrap();
        std::env::remove_var("STRYKE_TARGET");
        assert_eq!(t, "x86_64-unknown-linux-musl");
    }

    #[test]
    fn locate_manifest_dir_finds_root_level() {
        let d = tempdir("locate-root");
        std::fs::write(
            d.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let r = locate_manifest_dir(&d).unwrap();
        assert_eq!(r, d);
    }

    #[test]
    fn locate_manifest_dir_descends_single_top_dir() {
        let d = tempdir("locate-nested");
        let inner = d.join("stryke-gui-v0.2.0-aarch64-apple-darwin");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            inner.join(MANIFEST_FILE),
            "[package]\nname=\"gui\"\nversion=\"0.2.0\"\n",
        )
        .unwrap();
        let r = locate_manifest_dir(&d).unwrap();
        assert_eq!(r, inner);
    }

    #[test]
    fn locate_manifest_dir_rejects_multiple_top_dirs() {
        let d = tempdir("locate-multi");
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        assert!(locate_manifest_dir(&d).is_err());
    }

    #[test]
    fn find_project_root_walks_up() {
        let root = tempdir("root");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let nested = root.join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let canonical_nested = nested.canonicalize().unwrap();
        let found = find_project_root(&canonical_nested).unwrap();
        let canonical_found = found.canonicalize().unwrap();
        assert_eq!(canonical_found, canonical_root);
    }

    #[test]
    fn resolve_module_local_lib_hit() {
        let root = tempdir("proj");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib/Foo")).unwrap();
        std::fs::write(root.join("lib/Foo/Bar.stk"), "# bar").unwrap();
        let r = resolve_module(&root, "Foo::Bar").unwrap().unwrap();
        assert!(r.ends_with("lib/Foo/Bar.stk"), "got {:?}", r);
    }

    #[test]
    fn resolve_module_falls_back_when_nothing_resolves() {
        let root = tempdir("proj");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let r = resolve_module(&root, "Foo::Bar").unwrap();
        assert!(r.is_none());
    }

    // ── Library-resolution tests (L1, L4) ─────────────────────────────
    //
    // These prove the three IDE-facing surfaces — linter, completion,
    // hover — all see installed-package subs through the same 3-arm
    // resolver (`static_analysis::resolve_require_path_from_file`,
    // `lsp::installed_package_completions`, `lsp::hover_markdown_for_word`).
    // STRYKE_HOME-mutating tests grab STRYKE_HOME_MUTEX so parallel
    // execution doesn't race on the process-global env var.

    /// Create a fake installed package at `<STRYKE_HOME>/store/<name>@<ver>/`
    /// with a single `lib/<Name>.stk` declaring the supplied sub names.
    /// Each sub gets a `## doc-line` directly above it so doc extraction
    /// has content to surface. Returns the store package path.
    fn fake_installed_pkg(
        store: &Store,
        name: &str,
        version: &str,
        namespace: &str,
        subs: &[&str],
    ) -> PathBuf {
        let pkg = store.package_dir(name, version);
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        let mut stk = format!("package {}\n\n", namespace);
        for s in subs {
            stk.push_str(&format!("## Mock docstring for {}::{}.\n", namespace, s));
            stk.push_str(&format!("fn {}::{} {{ 1 }}\n\n", namespace, s));
        }
        std::fs::write(pkg.join("lib").join(format!("{}.stk", namespace)), stk).unwrap();
        std::fs::write(
            pkg.join(MANIFEST_FILE),
            format!(
                "[package]\nname=\"{}\"\nversion=\"{}\"\n\n\
                 [ffi]\nlib-name=\"x\"\nnamespace=\"{}\"\nexports=[]\n",
                name, version, namespace
            ),
        )
        .unwrap();
        pkg
    }

    /// L1a — resolve_require_path_from_file finds an installed-package
    /// .stk file via the global pin when no local file resolves.
    #[test]
    fn resolve_require_path_resolves_installed_package() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("resolve-installed");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        let pkg_dir = fake_installed_pkg(&store, "gui", "1.0.0", "GUI", &["mouse_pos"]);
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // Script file lives in a directory with no stryke.toml — arm 1 + 2
        // miss, arm 3 (installed.toml) should fire.
        let script_dir = tempdir("resolve-script");
        let script_path = script_dir.join("script.stk");
        std::fs::write(&script_path, "use GUI\n").unwrap();

        let resolved = crate::static_analysis::resolve_require_path_from_file(
            script_path.to_str().unwrap(),
            "GUI",
        );
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(resolved.as_deref(), Some(pkg_dir.join("lib/GUI.stk").as_path()));
    }

    /// L1b — project-local `lib/GUI.stk` shadows the globally-pinned
    /// store entry (matches `resolve_module` arm 1 → wins over arm 3).
    #[test]
    fn resolve_require_path_local_shadows_installed() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("resolve-shadow");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        fake_installed_pkg(&store, "gui", "1.0.0", "GUI", &["mouse_pos"]);
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // Project with its own lib/GUI.stk that takes priority.
        let proj = tempdir("resolve-proj");
        std::fs::write(
            proj.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(proj.join("lib")).unwrap();
        let local_gui = proj.join("lib/GUI.stk");
        std::fs::write(&local_gui, "package GUI\nfn GUI::local_only { 1 }\n").unwrap();

        let script_path = proj.join("main.stk");
        std::fs::write(&script_path, "use GUI\n").unwrap();

        let resolved = crate::static_analysis::resolve_require_path_from_file(
            script_path.to_str().unwrap(),
            "GUI",
        );
        std::env::remove_var("STRYKE_HOME");

        // Resolved path must be the project-local file, NOT the store.
        assert_eq!(
            resolved.as_deref().and_then(|p| p.canonicalize().ok()),
            local_gui.canonicalize().ok()
        );
    }

    /// L1c — project lockfile (arm 2) wins over global pin (arm 3) when
    /// both name the same package but at different versions.
    #[test]
    fn resolve_require_path_lockfile_wins_over_global_pin() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("resolve-lock");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        let pinned = fake_installed_pkg(&store, "gui", "0.2.0", "GUI", &["mouse_pos"]);
        let _newer = fake_installed_pkg(&store, "gui", "0.1.0", "GUI", &["mouse_pos"]);

        // Global pin says 0.2.0, project lockfile pins 0.1.0.
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "0.2.0", "test");
        idx.save_to(&store).unwrap();

        let proj = tempdir("resolve-lock-proj");
        std::fs::write(
            proj.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            proj.join(LOCKFILE_FILE),
            "version = 1\nstryke = \"0.0.0\"\nresolved = \"2026-01-01T00:00:00Z\"\n\n\
             [[package]]\nname=\"gui\"\nversion=\"0.1.0\"\nsource=\"path+file:///x\"\n\
             integrity=\"sha256-0\"\nfeatures=[]\ndeps=[]\n",
        )
        .unwrap();

        let script_path = proj.join("main.stk");
        std::fs::write(&script_path, "use GUI\n").unwrap();

        let resolved = crate::static_analysis::resolve_require_path_from_file(
            script_path.to_str().unwrap(),
            "GUI",
        );
        std::env::remove_var("STRYKE_HOME");

        // Must be the 0.1.0 store entry (lockfile wins), NOT the 0.2.0
        // global pin.
        let expected_v01 = store.package_dir("gui", "0.1.0").join("lib/GUI.stk");
        let pinned_v02 = pinned.join("lib/GUI.stk");
        assert_eq!(resolved.as_deref(), Some(expected_v01.as_path()));
        assert_ne!(resolved.as_deref(), Some(pinned_v02.as_path()));
    }

    /// L4a — installed_package_completions surfaces every entry in
    /// installed.toml when no filter is supplied.
    #[test]
    fn installed_package_completions_lists_all_when_unfiltered() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("compl-all");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        fake_installed_pkg(&store, "gui", "1.0.0", "GUI", &[]);
        fake_installed_pkg(&store, "aws", "1.0.0", "AWS", &[]);
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.upsert("aws", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        let items = crate::lsp::installed_package_completions("");
        std::env::remove_var("STRYKE_HOME");

        let labels: Vec<String> = items.iter().map(|c| c.label.clone()).collect();
        assert!(labels.iter().any(|l| l == "GUI"), "got: {:?}", labels);
        assert!(labels.iter().any(|l| l == "AWS"), "got: {:?}", labels);
    }

    /// L4b — case-insensitive prefix match: typing `gu` or `Gu` both
    /// match the `GUI` namespace.
    #[test]
    fn installed_package_completions_case_insensitive_prefix() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("compl-prefix");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        fake_installed_pkg(&store, "gui", "1.0.0", "GUI", &[]);
        fake_installed_pkg(&store, "aws", "1.0.0", "AWS", &[]);
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.upsert("aws", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        let lower = crate::lsp::installed_package_completions("gu");
        let upper = crate::lsp::installed_package_completions("Gu");
        std::env::remove_var("STRYKE_HOME");

        assert_eq!(lower.len(), 1);
        assert_eq!(lower[0].label, "GUI");
        assert_eq!(upper.len(), 1);
        assert_eq!(upper[0].label, "GUI");
    }

    /// L4c — when no stryke.toml `[ffi].namespace` is declared, the
    /// completion falls back to scanning the first `lib/*.stk` for a
    /// `package X` line.
    #[test]
    fn installed_package_completions_falls_back_to_package_decl() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("compl-fallback");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        let pkg = store.package_dir("mypkg", "1.0.0");
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        // Manifest WITHOUT an [ffi] section — namespace must come from
        // the `package MyPkg` line in the lib file.
        std::fs::write(
            pkg.join(MANIFEST_FILE),
            "[package]\nname=\"mypkg\"\nversion=\"1.0.0\"\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("lib/MyPkg.stk"),
            "package MyPkg\nfn MyPkg::hello { 1 }\n",
        )
        .unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("mypkg", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        let items = crate::lsp::installed_package_completions("");
        std::env::remove_var("STRYKE_HOME");

        let labels: Vec<String> = items.iter().map(|c| c.label.clone()).collect();
        assert!(labels.iter().any(|l| l == "MyPkg"), "got: {:?}", labels);
    }

    /// L4d — `use Gui|` and `require Foo|` both register as use-context
    /// (drives `use<TAB>` → installed-package mode).
    #[test]
    fn line_completion_is_use_context_recognizes_keywords() {
        let line = "use Gui";
        assert!(crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "require Foo";
        assert!(crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "    use Foo::Bar";
        assert!(crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "use ";
        assert!(crate::lsp::line_completion_is_use_context(line, line.len()));
    }

    /// L4e — non-use lines or use-with-arguments (e.g. `use overload '+'`)
    /// must NOT trigger use-context. Otherwise sigil completion or
    /// string-arg edits get hijacked.
    #[test]
    fn line_completion_is_use_context_rejects_non_use_and_args() {
        let line = "my $x = 1";
        assert!(!crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "use overload '+'";
        assert!(!crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "GUI::mouse_pos";
        assert!(!crate::lsp::line_completion_is_use_context(line, line.len()));
        let line = "p use_count";
        assert!(!crate::lsp::line_completion_is_use_context(line, line.len()));
    }

    /// L1+L4 regression — `use GUI; GUI::mouse_pos()` from a script
    /// outside any project must NOT fire UndefinedSubroutine under the
    /// IDE LSP's strict-vars path. The bug shipped through v0.16.32 +
    /// v0.16.33: `resolve_require_path_from_file` was correct but
    /// `StmtKind::Use` in `collect_declarations_stmt` only called
    /// `declare_sub(module)` — it never chased the resolved file, so
    /// the GUI:: sub declarations never landed in the analyzer's
    /// scope. Added `follow_require(module)` in v0.16.34 to mirror
    /// what `require Foo::Bar` does.
    #[test]
    fn analyzer_chases_use_into_installed_package() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("analyze-use-chase");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        fake_installed_pkg(
            &store,
            "gui",
            "1.0.0",
            "GUI",
            &["mouse_pos", "keyboard_keys"],
        );
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // Script sits in a non-project tempdir so neither arm 1
        // (project lib/) nor arm 2 (lockfile) can hit. Arm 3
        // (installed.toml) is the only candidate.
        let script_dir = tempdir("analyze-use-script");
        let script_path = script_dir.join("main.stk");
        let text = "use GUI\nmy ($x, $y) = GUI::mouse_pos()\nmy @k = GUI::keyboard_keys()\n";
        std::fs::write(&script_path, text).unwrap();

        let program = crate::parse_with_file(text, script_path.to_str().unwrap())
            .expect("script should parse");
        // Mirror what the IDE LSP's compute_diagnostics does:
        // analyze_program_with_strict(program, path, true).
        let result = crate::static_analysis::analyze_program_with_strict(
            &program,
            script_path.to_str().unwrap(),
            true,
        );
        std::env::remove_var("STRYKE_HOME");

        if let Err(e) = result {
            panic!(
                "linter should accept GUI::* calls after `use GUI` chases into \
                 the installed package, but got: kind={:?} message={}",
                e.kind, e.message
            );
        }
    }

    /// L5 — when the analyzer chases into a cdylib package's `lib/X.stk`
    /// wrapper, the wrapper's body calls FFI exports (`gui__mouse_pos`,
    /// `gui__keyboard_keys`, etc.) that have no .stk-side declaration —
    /// they're `#[no_mangle] extern "C" fn`s in the cdylib registered
    /// at runtime by `rust_ffi::load_cdylib`. v0.16.34 read the sibling
    /// stryke.toml's `[ffi].exports` and pre-declares each export as a
    /// known sub before walking the file's statements. Without this
    /// pre-pass every `gui__*` bareword call would fire as
    /// UndefinedSubroutine inside the IDE LSP's strict-vars path.
    #[test]
    fn analyzer_pre_declares_ffi_exports_when_chasing_into_package() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("analyze-ffi-exports");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();

        // Hand-craft a fake gui package with a .stk wrapper whose body
        // calls FFI exports. fake_installed_pkg's default wrapper only
        // declares `fn`s — overwrite it with one that calls `gui__*`
        // names from inside each sub body so the analyzer has to walk
        // those call sites with strict mode on.
        let pkg = store.package_dir("gui", "1.0.0");
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        std::fs::write(
            pkg.join("stryke.toml"),
            "[package]\nname=\"gui\"\nversion=\"1.0.0\"\n\n\
             [ffi]\nlib-name=\"x\"\nnamespace=\"GUI\"\n\
             exports=[\"gui__mouse_pos\", \"gui__keyboard_keys\"]\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("lib/GUI.stk"),
            "package GUI\n\n\
             fn GUI::mouse_pos { gui__mouse_pos(\"{}\") }\n\
             fn GUI::keyboard_keys { gui__keyboard_keys(\"{}\") }\n",
        )
        .unwrap();

        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // User's script — outside any project, so arm 3 fires.
        let script_dir = tempdir("analyze-ffi-script");
        let script_path = script_dir.join("main.stk");
        let text = "use GUI\nmy ($x, $y) = GUI::mouse_pos()\n";
        std::fs::write(&script_path, text).unwrap();

        let program = crate::parse_with_file(text, script_path.to_str().unwrap())
            .expect("script should parse");
        let result = crate::static_analysis::analyze_program_with_strict(
            &program,
            script_path.to_str().unwrap(),
            true,
        );
        std::env::remove_var("STRYKE_HOME");

        if let Err(e) = result {
            panic!(
                "linter should pre-declare [ffi].exports when chasing into the \
                 cdylib package — gui__mouse_pos / gui__keyboard_keys must NOT \
                 fire UndefinedSubroutine. Got: kind={:?} message={}",
                e.kind, e.message
            );
        }
    }

    /// L6 — end-to-end IDE scenario: hover on the package name `GUI`
    /// from a user script that does `use GUI`, with the package's
    /// `lib/GUI.stk` shipping the conventional rustdoc layout (file-level
    /// `##` block, blank separator, `package GUI`). v0.16.36's walker
    /// fix made the blank separator survivable; this test pins that the
    /// cross-file chase + walker combo surfaces the file-header docs to
    /// the IDE hover card.
    #[test]
    fn hover_chases_use_to_show_package_module_docs() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("hover-pkg-module-docs");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();

        // Hand-craft the package — the default fake_installed_pkg helper
        // doesn't lay out file-level docs, but the real stryke-gui 0.2.2
        // ships this exact shape (## header, blank, `package GUI`).
        let pkg = store.package_dir("gui", "1.0.0");
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        std::fs::write(
            pkg.join("stryke.toml"),
            "[package]\nname=\"gui\"\nversion=\"1.0.0\"\n\n\
             [ffi]\nlib-name=\"x\"\nnamespace=\"GUI\"\nexports=[]\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("lib/GUI.stk"),
            "## lib/GUI.stk — GUI automation for stryke (`use GUI`).\n\
             ##\n\
             ## Thin wrapper around the stryke-gui cdylib's exports.\n\
             \n\
             package GUI\n\
             \n\
             fn GUI::mouse_pos { 1 }\n",
        )
        .unwrap();

        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // User script — outside any project, just `use GUI`. The IDE
        // would hover over the `GUI` token on this line.
        let script_dir = tempdir("hover-pkg-script");
        let script_path = script_dir.join("main.stk");
        let text = "use GUI\n";
        std::fs::write(&script_path, text).unwrap();

        let hover =
            crate::lsp::hover_markdown_for_word("GUI", text, script_path.to_str().unwrap());
        std::env::remove_var("STRYKE_HOME");

        let md = hover.expect("hover should resolve the GUI package via cross-file chase");
        assert!(
            md.contains("GUI automation"),
            "package hover must surface the file-header `## …` block: {}",
            md
        );
        assert!(
            md.contains("Thin wrapper around the stryke-gui cdylib"),
            "multi-line file header must come through intact: {}",
            md
        );
        assert!(
            md.contains("declared in"),
            "header line ('declared in <path> at line N') should still be appended: {}",
            md
        );
    }

    /// L7 — end-to-end IDE hover on a library SUB (`GUI::mouse_pos`)
    /// whose `##` docstring sits a blank line above the `fn` decl —
    /// the conventional layout the user's stryke-gui 0.2.2 ships with.
    /// Pre-v0.16.36 this returned the header alone; post-fix it surfaces
    /// the docstring too.
    #[test]
    fn hover_chases_use_to_show_fn_docs_with_blank_separator() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("hover-fn-blank-sep");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();

        let pkg = store.package_dir("gui", "1.0.0");
        std::fs::create_dir_all(pkg.join("lib")).unwrap();
        std::fs::write(
            pkg.join("stryke.toml"),
            "[package]\nname=\"gui\"\nversion=\"1.0.0\"\n\n\
             [ffi]\nlib-name=\"x\"\nnamespace=\"GUI\"\n\
             exports=[\"gui__mouse_pos\"]\n",
        )
        .unwrap();
        // Conventional layout: blank line between docstring and fn.
        std::fs::write(
            pkg.join("lib/GUI.stk"),
            "package GUI\n\n\
             ## Current cursor position → ($x, $y).\n\
             \n\
             fn GUI::mouse_pos { gui__mouse_pos(\"{}\") }\n",
        )
        .unwrap();

        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        let script_dir = tempdir("hover-fn-script");
        let script_path = script_dir.join("main.stk");
        let text = "use GUI\nmy ($x, $y) = GUI::mouse_pos()\n";
        std::fs::write(&script_path, text).unwrap();

        let hover = crate::lsp::hover_markdown_for_word(
            "GUI::mouse_pos",
            text,
            script_path.to_str().unwrap(),
        );
        std::env::remove_var("STRYKE_HOME");

        let md = hover.expect("hover should resolve `GUI::mouse_pos` cross-file");
        assert!(
            md.contains("Current cursor position"),
            "fn hover must surface the `##` docstring even with a blank \
             separator above the `fn` decl: {}",
            md
        );
    }

    /// L2 — hover_markdown_for_word chases `use GUI` into the store
    /// file and surfaces the `## …` doc block declared above the sub.
    #[test]
    fn hover_chases_use_directive_into_store() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("hover-cross");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        fake_installed_pkg(&store, "gui", "1.0.0", "GUI", &["mouse_pos"]);
        let mut idx = InstalledIndex::new();
        idx.upsert("gui", "1.0.0", "test");
        idx.save_to(&store).unwrap();

        // Script lives in a non-project dir.
        let script_dir = tempdir("hover-script");
        let script_path = script_dir.join("main.stk");
        let text = "use GUI\n";
        std::fs::write(&script_path, text).unwrap();

        let hover = crate::lsp::hover_markdown_for_word(
            "GUI::mouse_pos",
            text,
            script_path.to_str().unwrap(),
        );
        std::env::remove_var("STRYKE_HOME");

        let md = hover.expect("hover should resolve `GUI::mouse_pos` cross-file");
        // Doc block from fake_installed_pkg is "Mock docstring for GUI::mouse_pos."
        assert!(
            md.contains("Mock docstring for GUI::mouse_pos"),
            "hover did not include the chased doc: {}",
            md
        );
    }
}
