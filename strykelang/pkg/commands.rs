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
use super::store::Store;
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
    println!("  NAME/.gitignore           ignores target/, *.stkc, *.pec");
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
        let body = "# stryke build artifacts\n/target/\n*.stkc\n*.pec\n";
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
    let mut m = Manifest::default();
    m.package = Some(PackageMeta {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: String::new(),
        authors: Vec::new(),
        license: String::new(),
        repository: String::new(),
        edition: "2026".to_string(),
    });
    let mut bin = IndexMap::new();
    bin.insert(name.to_string(), "main.stk".to_string());
    m.bin = bin;
    m
}

/// `s add NAME[@VER] [--dev|--group=NAME] [--path=...]` — append a dep to
/// `stryke.toml` and re-run install. Idempotent on the manifest level: adding
/// the same dep twice updates the version in place rather than duplicating.
pub fn cmd_add(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke add NAME[@VER] [--dev | --group=NAME] [--path=DIR] [--features=A,B]");
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
                return Ok(Some(nested_path));
            }
        }
    }
    Ok(None)
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
        println!("  install [--offline]       resolve deps + write stryke.lock");
        println!("  add NAME[@VER] [...]      add a dep to stryke.toml");
        println!("  remove NAME               drop a dep from stryke.toml");
        println!("  tree                      print resolved dep graph");
        println!("  info NAME                 show lockfile entry for a dep");
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
        "install" => cmd_install(&args[1..]),
        "tree" => cmd_tree(&args[1..]),
        "info" => cmd_info(&args[1..]),
        other => {
            eprintln!("s pkg: unknown subcommand `{}`", other);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
