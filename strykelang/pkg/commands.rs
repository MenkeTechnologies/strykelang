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
use super::{PkgError, PkgResult};

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
            "usage: stryke add SPEC [--dev | --group=NAME] [--path=DIR] [--features=A,B]"
        );
        println!();
        println!("Add a dependency to stryke.toml and run `s install` to refresh stryke.lock.");
        println!();
        println!("SPEC may be one of:");
        println!("  NAME[@VER]                                registry dep");
        println!("  github.com/OWNER/REPO[@TAG]               github-release dep (prebuilt tarball)");
        println!("  https://github.com/OWNER/REPO[.git][@TAG] github-release dep (full URL form)");
        println!("  ./PATH | ../PATH | /ABS/PATH | ~/PATH     local path dep");
        println!("  EXISTING_DIRECTORY                        local path dep (auto-detected)");
        println!();
        println!("`s install` downloads github-release deps as prebuilt tarballs from");
        println!("the repo's GitHub Releases (host-triple asset, SHA-256 verified). This");
        println!("is the path FFI cdylib packages (stryke-arrow, stryke-aws, ...) take.");
        println!("For source-only git deps (no [ffi], no release artifacts), write the");
        println!("`{{ git = \"...\" }}` form directly in stryke.toml.");
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
        println!("  stryke add github.com/MenkeTechnologies/stryke-parquet");
        println!("  stryke add github.com/MenkeTechnologies/stryke-aws@v0.2.0");
        println!("  stryke add ../sibling-pkg");
        println!("  stryke add /work/vendored/mylib");
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

    // github.com/OWNER/REPO[@TAG] shorthand → github-release dep. Writes
    // `github = "OWNER/REPO"` (plus `version = "TAG"` if `@TAG` given),
    // which routes through Resolver::install_github_dep at install time:
    // the prebuilt release tarball for the host triple gets downloaded,
    // SHA-256 verified, and extracted into the store. That's the path
    // FFI cdylib packages (stryke-arrow, stryke-aws, ...) need — they
    // can't be reproduced from a source clone without platform libs +
    // toolchain. For source-only git deps (no [ffi], no published
    // releases) the user writes `{ git = "..." }` directly in
    // stryke.toml; the `s add github.com/...` shorthand defaults to the
    // release path because that's what 99% of public GitHub-hosted
    // stryke packages actually want. `--path=` still wins (user
    // explicitly opted into a local copy of the source).
    if let Some(gh) = parse_github_shorthand(raw) {
        let spec = if let Some(p) = path_override {
            DepSpec::Detailed(DetailedDep {
                path: Some(p),
                version: gh.tag.clone(),
                features,
                default_features: true,
                ..DetailedDep::default()
            })
        } else {
            DepSpec::Detailed(DetailedDep {
                github: Some(gh.owner_repo),
                version: gh.tag,
                features,
                default_features: true,
                ..DetailedDep::default()
            })
        };
        return Ok(AddArgs {
            name: gh.name,
            spec,
            kind,
        });
    }

    // Bare path positional → path dep. Matches when the positional
    // either starts with a path sigil (`/`, `./`, `../`, `~/`) or
    // resolves to an existing directory. The dep's local name is its
    // own `[package].name` if a stryke.toml is present, otherwise the
    // last path component. Explicit `--path=DIR` still wins (it's the
    // documented override and may legitimately point at a different
    // directory than the positional argument).
    if path_override.is_none() {
        if let Some(local) = parse_local_path_arg(raw) {
            let spec = DepSpec::Detailed(DetailedDep {
                path: Some(local.path_for_manifest),
                features,
                default_features: true,
                ..DetailedDep::default()
            });
            return Ok(AddArgs {
                name: local.name,
                spec,
                kind,
            });
        }
    }

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

struct GithubShorthand {
    /// Local manifest key — the repo basename (last path component,
    /// minus any `.git` suffix). E.g. `stryke-parquet`.
    name: String,
    /// `OWNER/REPO` form that lands in the manifest's
    /// `github = "..."` field. E.g. `MenkeTechnologies/stryke-parquet`.
    owner_repo: String,
    /// Optional `@TAG` portion — pins which release to download. Without
    /// a tag the resolver fetches the latest release at install time.
    tag: Option<String>,
}

struct LocalPathArg {
    name: String,
    /// The exact string that lands in `stryke.toml`'s `path = "..."`
    /// field. Relative inputs (`../mylib`, `./vendored`) are preserved
    /// as-typed so the manifest stays portable across hosts; absolute
    /// inputs land as absolute. `~/` is expanded so the manifest never
    /// contains a literal tilde (which the resolver wouldn't expand).
    path_for_manifest: String,
}

/// Recognize a bare positional that names a local directory dependency
/// (`./mylib`, `../foo/bar`, `/abs/path`, `~/projects/mylib`, or any
/// existing-on-disk directory). The dep's local name comes from the
/// directory's `stryke.toml`'s `[package].name` when present, otherwise
/// the last path component. Returns `None` for anything that doesn't
/// look path-shaped AND isn't an existing directory — those fall
/// through to the registry / version-spec path so `s add http@1.0`
/// still works.
fn parse_local_path_arg(raw: &str) -> Option<LocalPathArg> {
    // Reject obvious non-paths early: a `@VER` suffix never appears on
    // path positionals (versions for path deps come from the dep's own
    // [package].version), and a `:` is a URL marker (file://, http://).
    if raw.contains('@') || raw.contains(':') {
        return None;
    }

    let sigil_path = raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with('/')
        || raw.starts_with("~/")
        || raw == "."
        || raw == "..";

    // Expand `~/` so on-disk lookups + the manifest entry both work.
    // We resolve `~` only when it's the first path component; embedded
    // `~` elsewhere is not a tilde (e.g. could be in a vendor dirname).
    let expanded: String = if raw == "~" {
        std::env::var("HOME").ok()?
    } else if let Some(rest) = raw.strip_prefix("~/") {
        let home = std::env::var("HOME").ok()?;
        format!("{}/{}", home.trim_end_matches('/'), rest)
    } else {
        raw.to_string()
    };

    let candidate = Path::new(&expanded);
    let exists_as_dir = candidate.is_dir();

    // Path-shaped sigils are accepted even when the directory doesn't
    // yet exist (helps with `s add ../about-to-create`); non-sigil
    // names only become path deps when they already exist on disk
    // (avoids treating `serde` as a path because there happens to be
    // a `./serde` symlink — well, that IS a path, but if there is a
    // local `./serde` dir the user almost certainly meant it).
    if !sigil_path && !exists_as_dir {
        return None;
    }

    // The name is the package's own [package].name when stryke.toml is
    // present and parseable; otherwise the last path component.
    let derived_name = candidate
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        // `..` and `.` have no file_name; fall back to the canonicalized
        // basename in those cases.
        .or_else(|| {
            candidate
                .canonicalize()
                .ok()
                .and_then(|c| c.file_name().map(|s| s.to_string_lossy().into_owned()))
        })?;

    let name = if exists_as_dir {
        let manifest_path = candidate.join(MANIFEST_FILE);
        if manifest_path.is_file() {
            Manifest::from_path(&manifest_path)
                .ok()
                .and_then(|m| m.package.map(|p| p.name))
                .unwrap_or(derived_name)
        } else {
            derived_name
        }
    } else {
        derived_name
    };

    // For the manifest path string: preserve relative form as typed
    // (skip tilde expansion when the input was relative — already not
    // tilde-prefixed), and emit the expanded form for `~/` inputs.
    let path_for_manifest = if raw.starts_with("~/") || raw == "~" {
        expanded
    } else {
        raw.to_string()
    };

    Some(LocalPathArg {
        name,
        path_for_manifest,
    })
}

/// Recognize `github.com/OWNER/REPO[.git][@TAG]` and `https://github.com/...`
/// forms. Returns `None` if the raw arg doesn't match (callers fall through
/// to the registry / version-spec path).
fn parse_github_shorthand(raw: &str) -> Option<GithubShorthand> {
    let stripped = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    let body = stripped.strip_prefix("github.com/")?;
    // Must be exactly OWNER/REPO[@TAG] — no extra path components, no
    // empty owner/repo. `body.splitn(2, '/')` separates owner from the
    // rest so we can detect a trailing `/` (sub-path) as invalid.
    let mut parts = body.splitn(2, '/');
    let owner = parts.next()?;
    let remainder = parts.next()?;
    if owner.is_empty() || remainder.is_empty() || owner.contains('@') {
        return None;
    }
    // The remainder is `REPO[.git][@TAG]`. Sub-paths beyond REPO aren't
    // a valid git source — refuse them rather than silently truncate.
    if remainder.contains('/') {
        return None;
    }
    let (repo_with_suffix, tag) = match remainder.split_once('@') {
        Some((r, t)) if !t.is_empty() => (r, Some(t.to_string())),
        Some((_, _)) => return None, // trailing `@` with empty tag is malformed
        None => (remainder, None),
    };
    let repo = repo_with_suffix.strip_suffix(".git").unwrap_or(repo_with_suffix);
    if repo.is_empty() {
        return None;
    }
    Some(GithubShorthand {
        name: repo.to_string(),
        owner_repo: format!("{}/{}", owner, repo),
        tag,
    })
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
            if let Some(gh) = &d.github {
                bits.push(format!("github = \"{}\"", gh));
            }
            if let Some(b) = &d.branch {
                bits.push(format!("branch = \"{}\"", b));
            }
            if let Some(t) = &d.tag {
                bits.push(format!("tag = \"{}\"", t));
            }
            if let Some(r) = &d.rev {
                bits.push(format!("rev = \"{}\"", r));
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

    // Pin every just-resolved package into ~/.stryke/installed.toml so
    // `use Foo` from outside the project (or before any `s install -g`)
    // still resolves via the global index. This matches the principle
    // "any install action should leave the global index reflecting the
    // packages present in the store"; users running `s install` from
    // a project expect downstream tooling (the linter, `stryke -e`,
    // standalone scripts) to see those packages too.
    sync_installed_index_from_resolution(&store, &outcome.installed);
    0
}

/// Pull each resolved package's canonical name + version + source +
/// namespace from its store-extracted `stryke.toml` and upsert into the
/// global `~/.stryke/installed.toml`. Silent best-effort: failures
/// surface as one stderr warning each and don't abort `s install`
/// (the lockfile is already on disk; the global pin is a convenience).
fn sync_installed_index_from_resolution(
    store: &Store,
    installed: &[(String, String, std::path::PathBuf)],
) {
    if installed.is_empty() {
        return;
    }
    let mut idx = match InstalledIndex::load_from(store) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s install: load installed.toml (skipping global pin): {}", e);
            return;
        }
    };
    let mut updated = 0usize;
    for (lock_name, version, store_path) in installed {
        // Try both candidate dirs (alias and prefixed) — same reason
        // as resolve_store_candidates: lockfiles record the alias while
        // store dirs use the canonical name.
        let candidates = resolve_store_candidates(store, lock_name, version);
        let real_store_dir = candidates
            .iter()
            .find(|p| p.is_dir())
            .cloned()
            .unwrap_or_else(|| store_path.clone());
        let manifest_path = real_store_dir.join(MANIFEST_FILE);
        let Ok(m) = Manifest::from_path(&manifest_path) else {
            continue;
        };
        let Some(pkg) = m.package.as_ref() else {
            continue;
        };
        let namespace = m
            .ffi
            .as_ref()
            .map(|f| f.namespace.clone())
            .unwrap_or_default();
        // Re-use the lockfile's source URL when present; for now derive
        // a placeholder from name@version. The pin is keyed on
        // (name, version), so the source string is informational.
        let source = format!("local-install:{}@{}", pkg.name, pkg.version);
        idx.upsert_with_namespace(&pkg.name, &pkg.version, &source, &namespace);
        updated += 1;
    }
    if updated > 0 {
        if let Err(e) = idx.save() {
            eprintln!("s install: write installed.toml: {}", e);
            return;
        }
        eprintln!(
            "\x1b[32m✓ pinned {} package{} in ~/.stryke/installed.toml\x1b[0m",
            updated,
            if updated == 1 { "" } else { "s" }
        );
    }
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
/// 3. The global `installed.toml` has an entry and the store dir exists.
///
/// `pin_version` (Perl-style `use Module VERSION` from the parser) is the
/// override: when `Some(v)`, the resolver looks up `<store>/<name>@<v>/`
/// directly and skips both the lockfile and the global index. An explicit
/// pin **always wins** — inside or outside a project. When `None`, the
/// lockfile-then-index precedence applies.
///
/// Returns `Ok(None)` if nothing resolved (caller falls through to `@INC`).
/// Returns `Err(...)` only when a pin can't be honored — either a use-site
/// `use Module VERSION` whose store dir is missing, or a lockfile entry
/// whose pinned version is missing from the store. In both cases falling
/// through silently would let a *different* version satisfy the load,
/// which is the "stryke use must respect package version" bug. Surfacing
/// the miss forces the user to run `s install` instead.
pub fn resolve_module(
    root: &Path,
    logical_name: &str,
    pin_version: Option<&str>,
) -> PkgResult<Option<PathBuf>> {
    let segments: Vec<&str> = logical_name.split("::").collect();
    if segments.is_empty() {
        return Ok(None);
    }

    // The `.stk` wrapper is the source-of-truth — if it exists at any tier,
    // resolution succeeds. The companion cdylib (when `[ffi]` is declared) is
    // best-effort: a missing or unloadable cdylib does NOT abort resolution
    // here, because that would silently send a real local hit down to @INC
    // (the caller in `vm_helper::try_resolve_via_lockfile` does
    // `resolve_module(...).unwrap_or_default()`). The first FFI call into an
    // unregistered export will surface the load failure at the actual call
    // site with a clear message, which is the right layer to report it.

    // 1. Project-local `lib/`. The use-site pin doesn't apply here —
    //    local lib/ is whatever's in the project's tree; pinning it to
    //    a different version is meaningless. Local hits always win
    //    (live edits should never be shadowed by a store version).
    let local = root.join("lib").join(segments_to_path(&segments));
    if local.is_file() {
        let _ = try_load_ffi_for(root);
        return Ok(Some(local));
    }

    // 1b. Flat-layout namespace bridge. Stryke-* packages ship
    //     `lib/<Sub>.stk` declaring `package <Ns>::<Sub>` with no
    //     `lib/<Ns>/` subdir. Bridge fires when segments[0] (case-
    //     insensitive) matches any of:
    //       * `[ffi].namespace` — FFI packages (stryke-arrow, stryke-aws).
    //       * `[package].name` — packages literally named after the
    //         namespace (rare; future-proofing).
    //       * `[package].name` minus the `stryke-` prefix — every
    //         stryke-* pure-stryke package (stryke-utils: name =
    //         "stryke-utils", umbrella = `Utils`).
    //     Mirrors the global-store branch (#3) and the
    //     `canonical_store_names_for_namespace` 3-arm logic so `s test`
    //     inside the package dir finds its own siblings without
    //     needing `s install -g .` after every edit.
    if segments.len() > 1 {
        if let Ok(manifest) = Manifest::from_path(&root.join(MANIFEST_FILE)) {
            let seg0_lc = segments[0].to_lowercase();
            let ns_match = manifest
                .ffi
                .as_ref()
                .map(|f| !f.namespace.is_empty() && f.namespace.eq_ignore_ascii_case(segments[0]))
                .unwrap_or(false);
            let pkg_name_match = manifest
                .package
                .as_ref()
                .map(|p| {
                    let n = p.name.to_lowercase();
                    n == seg0_lc || n == format!("stryke-{}", seg0_lc)
                })
                .unwrap_or(false);
            if ns_match || pkg_name_match {
                let flat = root.join("lib").join(segments_to_path(&segments[1..]));
                if flat.is_file() {
                    let _ = try_load_ffi_for(root);
                    return Ok(Some(flat));
                }
            }
        }
    }

    // 2a. Use-site pin (`use Module VERSION`) — direct store lookup,
    //     no lockfile / index consult. Inside or outside a project.
    //     Standalone scripts that want a specific version write
    //     `use Module 1.2` and land directly on `<store>/<name>@1.2/`.
    if let Some(v) = pin_version {
        let store = Store::user_default()?;
        let pkg_name_lower = segments[0].to_lowercase();
        let names_to_try = [pkg_name_lower.clone(), format!("stryke-{}", pkg_name_lower)];
        let mut tried: Vec<PathBuf> = Vec::with_capacity(names_to_try.len());
        for nm in names_to_try.iter() {
            let store_pkg = store.package_dir(nm, v);
            tried.push(store_pkg.clone());
            let nested_path = if segments.len() == 1 {
                store_pkg.join("lib").join(format!("{}.stk", segments[0]))
            } else {
                store_pkg.join("lib").join(segments_to_path(&segments[1..]))
            };
            if nested_path.is_file() {
                let _ = try_load_ffi_for(&store_pkg);
                return Ok(Some(nested_path));
            }
        }
        let probed = tried
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(PkgError::Other(format!(
            "use {} {}: pinned version not in store (tried: {})",
            logical_name, v, probed
        )));
    }

    // 2b. Lockfile-driven store lookup. Use the (lower-cased) first segment as
    //     the package name; remaining segments become the in-package path. Fall
    //     back to `stryke-<name>` so `use GUI` bridges to a `stryke-gui` entry
    //     even when the lockfile dep key is the prefixed canonical name.
    let lock_path = root.join(LOCKFILE_FILE);
    if lock_path.is_file() {
        let lock = Lockfile::from_path(&lock_path)?;
        let pkg_name = segments[0].to_lowercase();
        let entry = lock
            .find(&pkg_name)
            .or_else(|| lock.find(&format!("stryke-{}", pkg_name)));
        if let Some(entry) = entry {
            let store = Store::user_default()?;
            // Lockfiles often record the dep alias (`name = "postgres"`)
            // while the store directory uses the canonical package name
            // (`stryke-postgres@<ver>/`). Try the alias path first, then
            // fall back to the prefixed canonical form so deps installed
            // before the alias-vs-canonical convention shake-out still
            // resolve. Same path the analyzer uses (kept in sync).
            for store_pkg in resolve_store_candidates(&store, &entry.name, &entry.version) {
                let nested_path = if segments.len() == 1 {
                    store_pkg.join("lib").join(format!("{}.stk", segments[0]))
                } else {
                    store_pkg.join("lib").join(segments_to_path(&segments[1..]))
                };
                if nested_path.is_file() {
                    let _ = try_load_ffi_for(&store_pkg);
                    return Ok(Some(nested_path));
                }
            }
            // Lockfile pinned the version but the store has no
            // extraction at that version. Refuse to fall through to
            // the global index — silently picking a different version
            // would violate the lockfile pin. Surface the mismatch so
            // the user runs `s install`.
            return Err(PkgError::Other(format!(
                "use {}: stryke.lock pins {}@{} but store has no extraction at that version \
                 (run `s install`)",
                logical_name, entry.name, entry.version
            )));
        }
    }

    // 3. Outside-project / unpinned global lookup. The script runs
    //    outside any stryke project (no `stryke.toml` ancestor) or the
    //    project's lockfile doesn't pin this package. The contract here
    //    is "latest installed version" — scan the store for every
    //    `<canonical>@*/` extraction matching the namespace and pick
    //    the highest semver. This is robust against InstalledIndex
    //    drift (`upsert` records last-installed, not highest-installed),
    //    and matches the LSP's version-completion behavior.
    //
    // Canonical name discovery: same 3-arm logic as the LSP —
    // `[package].name` direct match, `[ffi].namespace` match, then
    // `stryke-<lowername>` prefix fallback. Each arm contributes
    // names to scan for; we union them and let the scan pick the
    // highest version across all matches.
    let store = Store::user_default()?;
    let canonical_names = canonical_store_names_for_namespace(&store, &segments[0].to_lowercase());
    if let Some((name, version)) = scan_store_for_highest_version(&store, &canonical_names) {
        let store_pkg = store.package_dir(&name, &version);
        let nested_path = if segments.len() == 1 {
            store_pkg.join("lib").join(format!("{}.stk", segments[0]))
        } else {
            store_pkg.join("lib").join(segments_to_path(&segments[1..]))
        };
        if nested_path.is_file() {
            let _ = try_load_ffi_for(&store_pkg);
            return Ok(Some(nested_path));
        }
    }

    Ok(None)
}

/// Resolve a lowercased `use Foo` first segment to every store-name it
/// might be extracted under. Three sources, unioned + deduped:
///
/// 1. `<ns>` itself — the bare namespace (e.g. `gui`).
/// 2. `stryke-<ns>` — the prefixed canonical name used by every
///    `stryke-*` ecosystem package.
/// 3. `installed.toml` entries whose `[package].name` matches the
///    namespace OR whose `[ffi].namespace` matches — bridges
///    `use GUI` to a `stryke-gui` extraction regardless of how the
///    name was recorded.
///
/// Returns the union so `scan_store_for_highest_version` can pick the
/// best version across all of them — important when both `gui@0.9` and
/// `stryke-gui@1.2` exist on disk from different install paths.
pub(crate) fn canonical_store_names_for_namespace(store: &Store, namespace_lc: &str) -> Vec<String> {
    let mut names = Vec::new();
    names.push(namespace_lc.to_string());
    names.push(format!("stryke-{}", namespace_lc));
    if let Ok(idx) = InstalledIndex::load_from(store) {
        for pkg in &idx.packages {
            let name_lc = pkg.name.to_lowercase();
            let pkg_ns_lc = pkg.namespace.to_lowercase();
            if name_lc == namespace_lc
                || pkg_ns_lc == namespace_lc
                || name_lc == format!("stryke-{}", namespace_lc)
            {
                names.push(pkg.name.clone());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Walk `<store>/` and pick the highest-version extraction whose
/// directory name is `<candidate>@<version>/` for any candidate in
/// `names`. Returns `(canonical_name, version_string)` for the
/// winner. Versions are compared as dotted-integer tuples so
/// `2.0` > `1.99` and `1.0` < `1.0.1` order the way humans expect.
///
/// Non-semver-looking versions (e.g. `dev`, `0.1-pre`) sort as zero
/// in the numeric comparison and lose to anything with real numbers,
/// matching the LSP's completion ranking.
pub(crate) fn scan_store_for_highest_version(
    store: &Store,
    names: &[String],
) -> Option<(String, String)> {
    let store_dir = store.store_dir();
    let entries = std::fs::read_dir(&store_dir).ok()?;
    let mut best: Option<(String, String, VersionRank)> = None;
    for ent in entries.flatten() {
        let dirname = ent.file_name().to_string_lossy().into_owned();
        let Some((pkg, ver)) = dirname.rsplit_once('@') else {
            continue;
        };
        if !names.iter().any(|n| n == pkg) {
            continue;
        }
        let rank = VersionRank::parse(ver);
        let take = match &best {
            None => true,
            Some((_, _, cur)) => rank > *cur,
        };
        if take {
            best = Some((pkg.to_string(), ver.to_string(), rank));
        }
    }
    best.map(|(n, v, _)| (n, v))
}

/// Sort key for ranking `<store>/<name>@<version>/` directories by
/// "newest wins" semantics. Total-ordered so the scan is deterministic
/// regardless of filesystem iteration order.
///
/// Ordering rules, in priority:
/// 1. Numeric tuple from the dotted prefix: `2.0 > 1.99`, `0.10 > 0.3`
///    (numeric compare, not lexicographic).
/// 2. Release > pre-release. A version with a `-suffix` (e.g.
///    `1.0.0-rc1`) ranks below the same numeric prefix without one
///    (`1.0.0`). Mirrors semver §11 — without this rule, `1.0.0` and
///    `1.0.0-rc1` mapped to the same key and the scan returned
///    whichever directory the filesystem happened to yield first.
/// 3. Within pre-releases, alphabetic prefix of the suffix
///    (`alpha < beta < rc`), then trailing number (`rc10 > rc2`).
///    Not a full semver §11 implementation — the resolver only needs
///    a stable total order; the package manager owns the strict
///    semver layer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VersionRank {
    /// Dotted numeric prefix as a Vec<u64>. `1.0.0` → `[1, 0, 0]`.
    nums: Vec<u64>,
    /// `1` for a release (no `-suffix`), `0` for any pre-release. Comes
    /// before the suffix fields so release beats every pre-release with
    /// the same numeric prefix.
    is_release: u8,
    /// Alphabetic head of the pre-release suffix (`rc` from `rc10`).
    /// Empty for releases; lexicographic compare gives the conventional
    /// `alpha < beta < rc` ordering.
    suffix_alpha: String,
    /// Trailing number in the pre-release suffix (`10` from `rc10`).
    /// Zero when the suffix has no number (`beta` → `0`).
    suffix_num: u64,
}

impl VersionRank {
    pub(crate) fn parse(ver: &str) -> Self {
        // Split numeric prefix from pre-release suffix at the first `-`.
        // Build-metadata (`+sha…`) is not part of precedence per semver
        // §10; strip it if present.
        let no_build = ver.split('+').next().unwrap_or(ver);
        let (prefix, suffix) = match no_build.split_once('-') {
            Some((p, s)) => (p, s),
            None => (no_build, ""),
        };
        let nums: Vec<u64> = prefix
            .split('.')
            .map(|p| {
                let head: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                head.parse::<u64>().unwrap_or(0)
            })
            .collect();
        let is_release = if suffix.is_empty() { 1 } else { 0 };
        // Split suffix into alpha-head + trailing number. `rc10` →
        // ("rc", 10); `beta` → ("beta", 0). Anything past the trailing
        // digits is discarded — the resolver doesn't need to round-trip
        // semver, only rank deterministically.
        let alpha_end = suffix
            .char_indices()
            .find(|(_, c)| c.is_ascii_digit())
            .map(|(i, _)| i)
            .unwrap_or(suffix.len());
        let suffix_alpha = suffix[..alpha_end].to_string();
        let suffix_num = suffix[alpha_end..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0);
        Self {
            nums,
            is_release,
            suffix_alpha,
            suffix_num,
        }
    }
}

/// Build the list of plausible `<store>/<name>@<version>/` paths for a
/// resolver entry. Tries the entry name as-is first, then `stryke-<name>`
/// when the entry isn't already prefixed. Covers the legacy split where
/// lockfile/installed.toml entries record the dep alias (`postgres`) but
/// the store directory is the canonical package name (`stryke-postgres`).
fn resolve_store_candidates(
    store: &Store,
    name: &str,
    version: &str,
) -> Vec<std::path::PathBuf> {
    let mut out = Vec::with_capacity(2);
    out.push(store.package_dir(name, version));
    if !name.starts_with("stryke-") {
        out.push(store.package_dir(&format!("stryke-{}", name), version));
    }
    out
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

/// `s upgrade [NAME]` — in a project: move deps to their latest upstream
/// versions, rewriting stryke.toml pins, then re-resolve via `s update`.
///
/// This is deliberately stronger than `s update`: `update` re-resolves
/// within the manifest's existing constraints; `upgrade` moves the
/// constraints themselves. Per dep kind:
///   - `{ github = "OWNER/REPO", version = "vX" }` — fetch the latest
///     release tag, rewrite `version` when it moved.
///   - `{ git = "https://github.com/..." , tag = "vX" }` — same, rewriting `tag`.
///   - unpinned github deps already float to latest on every re-resolve.
///   - path/workspace deps track their source dir — nothing to bump.
///   - registry deps can't be bumped until the registry endpoint is wired.
pub fn cmd_upgrade_project(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke upgrade [NAME]");
        println!();
        println!("Move deps to their latest upstream versions and re-resolve. Unlike");
        println!("`s update` (re-resolve within existing constraints), this rewrites the");
        println!("stryke.toml pins themselves:");
        println!("  github deps    pinned `version` bumped to the latest release tag");
        println!("  git deps       pinned `tag` bumped (github-hosted URLs only)");
        println!("  path deps      nothing to bump — they track their source dir");
        println!("  registry deps  skipped until the registry endpoint is wired");
        println!();
        println!("NAME: when given, only that dep's pin is bumped (others stay).");
        println!("Outside a project, use `s upgrade -g` for global packages.");
        return 0;
    }
    let filter = args.iter().find(|a| !a.starts_with('-')).cloned();
    let cwd = match std::env::current_dir() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("s upgrade: cwd: {}", e);
            return 1;
        }
    };
    let root = match find_project_root(&cwd) {
        Some(r) => r,
        None => {
            eprintln!("s upgrade: no stryke.toml found (use `s upgrade -g` for global packages)");
            return 1;
        }
    };
    let manifest_path = root.join(MANIFEST_FILE);
    let mut manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("s upgrade: {}", e);
            return 1;
        }
    };

    // Bump pins across [deps], [dev-deps], and every [groups.*] table.
    let mut bumped = 0u32;
    let mut failed = 0u32;
    let mut sections: Vec<&mut IndexMap<String, DepSpec>> =
        vec![&mut manifest.deps, &mut manifest.dev_deps];
    sections.extend(manifest.groups.values_mut());
    for section in sections {
        for (name, spec) in section.iter_mut() {
            if filter.as_deref().map_or(false, |f| name != f) {
                continue;
            }
            match bump_dep_pin(name, spec) {
                Ok(true) => bumped += 1,
                Ok(false) => {}
                Err(e) => {
                    eprintln!("  \x1b[31m✗ {}: {}\x1b[0m", name, e);
                    failed += 1;
                }
            }
        }
    }

    if bumped > 0 {
        let body = match manifest.to_toml_string() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("s upgrade: {}", e);
                return 1;
            }
        };
        if let Err(e) = std::fs::write(&manifest_path, body) {
            eprintln!("s upgrade: write {}: {}", manifest_path.display(), e);
            return 1;
        }
    }
    if failed > 0 {
        return 1;
    }
    // Re-resolve regardless: unpinned github deps float to latest here, and
    // path-dep integrity hashes re-pin against the current source dirs.
    cmd_update(&[])
}

/// Bump one dep's manifest pin to the latest upstream release. Returns
/// `Ok(true)` when the pin was rewritten, `Ok(false)` when there was nothing
/// to bump (already latest, unpinned, path/workspace, or registry dep).
fn bump_dep_pin(name: &str, spec: &mut DepSpec) -> Result<bool, String> {
    let DepSpec::Detailed(d) = spec else {
        // Bare `name = "1.0"` registry shorthand — registry not wired.
        eprintln!("  - {} registry dep — skipped (registry not wired)", name);
        return Ok(false);
    };
    // `{ github = "OWNER/REPO" }` — bump the `version` pin.
    if let Some(owner_repo) = d.github.clone() {
        let Some(pinned) = d.version.clone() else {
            eprintln!("  ✓ {} unpinned github dep — floats to latest on re-resolve", name);
            return Ok(false);
        };
        let (owner, repo) = parse_gh_owner_repo(&owner_repo)?;
        let latest = fetch_latest_release_tag(&owner, &repo)?;
        if same_version(&latest, &pinned) {
            eprintln!("  ✓ {}@{} up to date", name, pinned);
            return Ok(false);
        }
        eprintln!("  \x1b[33m{} {} → {}\x1b[0m", name, pinned, latest);
        d.version = Some(latest);
        return Ok(true);
    }
    // `{ git = "https://github.com/...", tag = "vX" }` — bump the `tag` pin.
    if let Some(url) = d.git.clone() {
        let Some((owner, repo)) = super::resolver::parse_github_url(&url) else {
            eprintln!("  - {} non-github git dep — skipped (no release API)", name);
            return Ok(false);
        };
        let Some(pinned) = d.tag.clone() else {
            eprintln!("  ✓ {} unpinned git dep — floats to latest on re-resolve", name);
            return Ok(false);
        };
        let latest = fetch_latest_release_tag(&owner, &repo)?;
        if same_version(&latest, &pinned) {
            eprintln!("  ✓ {}@{} up to date", name, pinned);
            return Ok(false);
        }
        eprintln!("  \x1b[33m{} {} → {}\x1b[0m", name, pinned, latest);
        d.tag = Some(latest);
        return Ok(true);
    }
    if d.path.is_some() || d.workspace {
        // Path/workspace deps track their source — re-resolve handles them.
        return Ok(false);
    }
    eprintln!("  - {} registry dep — skipped (registry not wired)", name);
    Ok(false)
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

    if let Err(msg) = finalize_global_install(&store, &manifest, &store_pkg_dir, &source) {
        eprintln!("s install -g: {}", msg);
        return 1;
    }
    0
}

/// Shared tail of every global install/upgrade: write the `[bin]` launchers,
/// pin the package in `~/.stryke/installed.toml`, and print the success line.
/// Called by `cmd_install_global` and per-package by `cmd_upgrade_global`.
fn finalize_global_install(
    store: &Store,
    manifest: &Manifest,
    store_pkg_dir: &Path,
    source: &str,
) -> Result<(), String> {
    // Launchers from [bin]. FFI-only packages may have empty [bin] — fine,
    // they're invoked via `use <namespace>` not via a CLI launcher.
    for (bin_name, entry) in &manifest.bin {
        let target = store_pkg_dir.join(entry);
        if !target.is_file() {
            return Err(format!(
                "bin `{}` -> {} does not exist",
                bin_name,
                target.display()
            ));
        }
        let launcher = store.bin_dir().join(bin_name);
        write_launcher(&launcher, &target)
            .map_err(|e| format!("write {}: {}", launcher.display(), e))?;
        eprintln!("  installed {} -> {}", launcher.display(), target.display());
    }

    // Pin the install in ~/.stryke/installed.toml so standalone scripts
    // (no project dir) can resolve `use <namespace>` to this store entry
    // — that's the resolution path resolve_module's third arm walks.
    if let Some(pkg) = manifest.package.as_ref() {
        let mut idx =
            InstalledIndex::load_or_default().map_err(|e| format!("load installed.toml: {}", e))?;
        // Warn loud when replacing a different pinned version so the user
        // notices the old store dir is now an orphan. Same-version reinstall
        // is silent (idempotent re-runs are normal).
        if let Some(prev) = idx.find(&pkg.name) {
            if prev.version != pkg.version {
                let old_dir = store.package_dir(&pkg.name, &prev.version);
                eprintln!(
                    "  \x1b[33mreplacing pinned {} {} → {}\x1b[0m  ({} kept on disk; run `s pkg gc -g` to free)",
                    pkg.name,
                    prev.version,
                    pkg.version,
                    old_dir.display()
                );
            }
        }
        let namespace = manifest
            .ffi
            .as_ref()
            .map(|f| f.namespace.clone())
            .unwrap_or_default();
        idx.upsert_with_namespace(&pkg.name, &pkg.version, source, &namespace);
        idx.save()
            .map_err(|e| format!("write installed.toml: {}", e))?;
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
    Ok(())
}

/// A pinned package's provenance, parsed back out of the `source` string
/// `s install -g` wrote into `~/.stryke/installed.toml`.
enum PinnedSource {
    /// `github:owner/repo@tag` — upgradable by asking the GitHub releases API
    /// for the latest tag.
    GitHub { owner: String, repo: String },
    /// `path+file:///abs/dir` — upgradable by re-reading the source dir's
    /// manifest and re-copying when its version moved.
    Path(PathBuf),
    /// `local-install:name@version` — pinned by `s install` inside a project;
    /// there is no upstream to poll, the owning project drives it.
    Local,
}

impl PinnedSource {
    fn parse(source: &str) -> Option<PinnedSource> {
        if let Some(rest) = source.strip_prefix("github:") {
            let head = rest.split('@').next().unwrap_or(rest);
            let (owner, repo) = parse_gh_owner_repo(head).ok()?;
            return Some(PinnedSource::GitHub { owner, repo });
        }
        if let Some(p) = source.strip_prefix("path+file://") {
            return Some(PinnedSource::Path(PathBuf::from(p)));
        }
        if source.starts_with("local-install:") {
            return Some(PinnedSource::Local);
        }
        None
    }
}

/// `v0.2.0` and `0.2.0` are the same version — release tags conventionally
/// carry the `v`, manifest `[package].version` never does.
fn same_version(a: &str, b: &str) -> bool {
    a.trim_start_matches('v') == b.trim_start_matches('v')
}

/// `s upgrade -g [NAME]` — re-pin every globally installed package at its
/// latest upstream version. GitHub pins poll the releases API and reinstall
/// when the tag moved; path pins re-read the source dir's manifest and
/// re-copy when its version moved; `local-install:` pins are skipped (the
/// owning project's `s install` drives those). With NAME, only that package.
pub fn cmd_upgrade_global(args: &[String]) -> i32 {
    if args.iter().any(|a| is_help_flag(a)) {
        println!("usage: stryke upgrade -g [NAME]");
        println!();
        println!("Upgrade globally installed packages (~/.stryke/installed.toml) to their");
        println!("latest upstream versions. Per pinned source:");
        println!("  github:owner/repo@tag    fetch latest release tag, reinstall if newer");
        println!("  path+file:///dir         re-copy when the source dir's version moved");
        println!("  local-install:...        skipped — re-run `s install` in that project");
        println!();
        println!("NAME: when given, only that package is upgraded.");
        return 0;
    }
    let filter = args.iter().find(|a| !a.starts_with('-')).cloned();
    let store = match Store::user_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("s upgrade -g: {}", e);
            return 1;
        }
    };
    let idx = match InstalledIndex::load_or_default() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("s upgrade -g: load installed.toml: {}", e);
            return 1;
        }
    };
    // Snapshot the entries up front: finalize_global_install rewrites
    // installed.toml after each successful upgrade.
    let entries: Vec<_> = idx
        .packages
        .iter()
        .filter(|p| filter.as_deref().map_or(true, |f| p.name == f))
        .cloned()
        .collect();
    if entries.is_empty() {
        match filter {
            Some(f) => {
                eprintln!("s upgrade -g: `{}` is not in installed.toml", f);
                return 1;
            }
            None => {
                eprintln!("s upgrade -g: no global packages installed");
                return 0;
            }
        }
    }

    let (mut upgraded, mut current, mut skipped, mut failed) = (0u32, 0u32, 0u32, 0u32);
    for entry in &entries {
        match PinnedSource::parse(&entry.source) {
            Some(PinnedSource::GitHub { owner, repo }) => {
                let latest = match fetch_latest_release_tag(&owner, &repo) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("  \x1b[31m✗ {}: {}\x1b[0m", entry.name, e);
                        failed += 1;
                        continue;
                    }
                };
                if same_version(&latest, &entry.version) {
                    eprintln!("  ✓ {}@{} up to date", entry.name, entry.version);
                    current += 1;
                    continue;
                }
                match install_global_from_github(&store, &owner, &repo, Some(&latest))
                    .and_then(|(m, dir, src)| finalize_global_install(&store, &m, &dir, &src))
                {
                    Ok(()) => upgraded += 1,
                    Err(e) => {
                        eprintln!("  \x1b[31m✗ {}: {}\x1b[0m", entry.name, e);
                        failed += 1;
                    }
                }
            }
            Some(PinnedSource::Path(dir)) => {
                let upstream = match Manifest::from_path(&dir.join(MANIFEST_FILE)) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!(
                            "  \x1b[31m✗ {}: source dir {}: {}\x1b[0m",
                            entry.name,
                            dir.display(),
                            e
                        );
                        failed += 1;
                        continue;
                    }
                };
                let upstream_version = upstream
                    .package
                    .as_ref()
                    .map(|p| p.version.clone())
                    .unwrap_or_default();
                if same_version(&upstream_version, &entry.version) {
                    eprintln!("  ✓ {}@{} up to date", entry.name, entry.version);
                    current += 1;
                    continue;
                }
                match install_global_from_path(&store, &dir)
                    .and_then(|(m, sdir, src)| finalize_global_install(&store, &m, &sdir, &src))
                {
                    Ok(()) => upgraded += 1,
                    Err(e) => {
                        eprintln!("  \x1b[31m✗ {}: {}\x1b[0m", entry.name, e);
                        failed += 1;
                    }
                }
            }
            Some(PinnedSource::Local) => {
                eprintln!(
                    "  - {}@{} pinned by a project install — re-run `s install` there",
                    entry.name, entry.version
                );
                skipped += 1;
            }
            None => {
                eprintln!(
                    "  - {}@{} unrecognized source `{}` — skipped",
                    entry.name, entry.version, entry.source
                );
                skipped += 1;
            }
        }
    }

    eprintln!(
        "{} upgraded, {} up to date, {} skipped, {} failed",
        upgraded, current, skipped, failed
    );
    if failed > 0 {
        1
    } else {
        0
    }
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

    // For `[ffi]` packages, the cdylib must end up at `lib/lib<name>.<ext>`
    // in the store (production layout — what GH-shipped tarballs always
    // contain). Source trees may or may not have it pre-built:
    //
    //   * `lib/lib<name>.<ext>`           — already in production layout; copied via the
    //                                       filtered subtree pass below.
    //   * `target/release/lib<name>.<ext>` — dev built; staged into dst/lib/ after copy.
    //   * neither                         — run `cargo build --release` here, then stage.
    let ffi_stage = ensure_ffi_cdylib(&abs, &manifest)?;

    let dst = store
        .install_path_dep(&pkg.name, &pkg.version, &abs, &manifest)
        .map_err(|e| e.to_string())?;

    // Stage the freshly-built (or target/-located) cdylib into dst/lib/ when
    // the source's lib/ didn't already have it. Mirrors the GH layout so
    // try_load_ffi_for finds the lib at candidate (1) — no fallback to
    // target/ needed once installed.
    if let Some((built_lib, lib_filename)) = ffi_stage {
        let dst_lib_dir = dst.join("lib");
        std::fs::create_dir_all(&dst_lib_dir)
            .map_err(|e| format!("create {}: {}", dst_lib_dir.display(), e))?;
        let dst_lib = dst_lib_dir.join(&lib_filename);
        if !dst_lib.is_file() {
            std::fs::copy(&built_lib, &dst_lib).map_err(|e| {
                format!(
                    "stage cdylib {} -> {}: {}",
                    built_lib.display(),
                    dst_lib.display(),
                    e
                )
            })?;
            eprintln!("  staged {} -> {}", built_lib.display(), dst_lib.display());
        }
    }

    let source = format!("path+file://{}", abs.display());
    Ok((manifest, dst, source))
}

/// For `[ffi]` packages, locate the cdylib in the source tree or build it
/// via `cargo build --release` if absent. Returns `Some((built_path,
/// dll_filename))` when a build was needed or the lib lives outside
/// `lib/` — the caller stages the file into `dst/lib/`. Returns `None`
/// when the package has no `[ffi]` section or when `lib/<filename>` is
/// already present in source (the filtered subtree copy handles it).
pub(crate) fn ensure_ffi_cdylib(
    src: &Path,
    manifest: &Manifest,
) -> Result<Option<(PathBuf, String)>, String> {
    let Some(ffi) = manifest.ffi.as_ref() else {
        return Ok(None);
    };
    if ffi.lib_name.is_empty() {
        return Ok(None);
    }
    let lib_filename = format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        ffi.lib_name,
        std::env::consts::DLL_SUFFIX
    );

    let in_lib = src.join("lib").join(&lib_filename);
    if in_lib.is_file() {
        // Already in production layout — install_path_dep's subtree pass
        // copies it as part of `lib/`.
        return Ok(None);
    }

    let release_built = src.join("target/release").join(&lib_filename);
    if release_built.is_file() {
        return Ok(Some((release_built, lib_filename)));
    }

    // Nothing built yet. Need a Cargo.toml at the source root to drive the
    // build — error clearly when absent so the user knows they're missing
    // the cdylib crate, not the toolchain.
    let cargo_toml = src.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return Err(format!(
            "[ffi] declared but cdylib `{}` not found and no Cargo.toml at {} \
             to build it (looked under lib/ and target/release/)",
            lib_filename,
            src.display()
        ));
    }

    eprintln!("  building cdylib: cargo build --release ({})", src.display());
    let status = std::process::Command::new("cargo")
        .arg("build")
        .arg("--release")
        .current_dir(src)
        .status()
        .map_err(|e| format!("spawn cargo: {}", e))?;
    if !status.success() {
        return Err(format!(
            "cargo build --release failed (exit {:?}) in {}",
            status.code(),
            src.display()
        ));
    }

    if !release_built.is_file() {
        return Err(format!(
            "cargo build --release succeeded but `{}` was not produced at {} \
             — check `[lib].name` in Cargo.toml matches `[ffi].lib-name` in stryke.toml",
            lib_filename,
            release_built.display()
        ));
    }
    Ok(Some((release_built, lib_filename)))
}

pub(crate) fn install_global_from_github(
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
                                // Per-launcher write to `removed_anything` was
                                // redundant — line 1830 unconditionally sets
                                // it to true once we reach this arm (the
                                // package IS in the index, so the unpin
                                // counts as success even if zero launchers
                                // existed). Keep the visible eprintln, drop
                                // the dead assignment that clippy flags.
                                eprintln!("  removed launcher {}", launcher.display());
                            }
                            Err(e) => {
                                eprintln!("s uninstall -g: remove {}: {}", launcher.display(), e);
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
        eprintln!(
            "  unpinned {} (store entry kept at {})",
            name,
            store_pkg.display()
        );
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
    let namespace = idx
        .find(&name)
        .map(|p| p.namespace.clone())
        .unwrap_or_default();
    idx.upsert_with_namespace(&name, &version, &source, &namespace);
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
            eprintln!("\x1b[32m✓ {} pin already at {}\x1b[0m", name, version);
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
        println!(
            "  list -g                   list global packages, launchers, and orphans (alias: ls)"
        );
        println!("  gc -g [--dry-run]         delete ~/.stryke/store/ entries no longer pinned");
        println!("  add NAME[@VER] [...]      add a dep to stryke.toml");
        println!("  remove NAME               drop a dep from stryke.toml");
        println!(
            "  update [NAME]             re-resolve within manifest constraints, rewrite stryke.lock (alias: up)"
        );
        println!("  upgrade [NAME]            bump stryke.toml pins to latest upstream, then re-resolve");
        println!("  upgrade -g [NAME]         upgrade global packages to latest upstream versions");
        println!("  outdated                  report deps drifted from their lock pin");
        println!("  audit                     check lockfile against advisory feed");
        println!("  tree                      print resolved dep graph");
        println!("  info NAME                 show lockfile entry for a dep");
        println!("  vendor                    snapshot store deps to ./vendor/");
        println!("  clean [--all]             wipe target/ (and optionally global caches)");
        println!("  search NAME               registry query (registry not deployed)");
        println!(
            "  publish [--dry-run]       publish to registry (registry not deployed) (alias: pub)"
        );
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
        "update" | "up" | "upgrade" => {
            if args.iter().skip(1).any(|a| a == "-g" || a == "--global") {
                let filtered: Vec<String> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.as_str(), "-g" | "--global"))
                    .cloned()
                    .collect();
                cmd_upgrade_global(&filtered)
            } else if args[0] == "upgrade" {
                cmd_upgrade_project(&args[1..])
            } else {
                cmd_update(&args[1..])
            }
        }
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
    fn pinned_source_parse_covers_all_install_writers() {
        // Every string shape a `let source = ...` writer produces must
        // round-trip through PinnedSource::parse.
        match PinnedSource::parse("github:MenkeTechnologies/stryke-gui@v0.3.0") {
            Some(PinnedSource::GitHub { owner, repo }) => {
                assert_eq!(owner, "MenkeTechnologies");
                assert_eq!(repo, "stryke-gui");
            }
            other => panic!("expected GitHub source, got {:?}", other.is_some()),
        }
        match PinnedSource::parse("path+file:///tmp/mytool") {
            Some(PinnedSource::Path(p)) => assert_eq!(p, PathBuf::from("/tmp/mytool")),
            other => panic!("expected Path source, got {:?}", other.is_some()),
        }
        assert!(matches!(
            PinnedSource::parse("local-install:foo@1.0.0"),
            Some(PinnedSource::Local)
        ));
        assert!(PinnedSource::parse("registry:foo@1.0.0").is_none());
        assert!(PinnedSource::parse("").is_none());
    }

    #[test]
    fn bump_dep_pin_no_network_paths() {
        // Every branch that must NOT hit the GitHub API returns Ok(false).
        let mut bare = DepSpec::Version("1.0".into());
        assert_eq!(bump_dep_pin("http", &mut bare), Ok(false));

        let mut path_dep = DepSpec::Detailed(DetailedDep {
            path: Some("../mylib".into()),
            ..Default::default()
        });
        assert_eq!(bump_dep_pin("mylib", &mut path_dep), Ok(false));

        // Unpinned github dep floats on re-resolve — no API call, no rewrite.
        let mut floating = DepSpec::Detailed(DetailedDep {
            github: Some("owner/repo".into()),
            ..Default::default()
        });
        assert_eq!(bump_dep_pin("float", &mut floating), Ok(false));

        // Non-github git URL has no releases API to poll.
        let mut gitlab = DepSpec::Detailed(DetailedDep {
            git: Some("https://gitlab.com/owner/repo.git".into()),
            tag: Some("v1.0".into()),
            ..Default::default()
        });
        assert_eq!(bump_dep_pin("gl", &mut gitlab), Ok(false));
    }

    #[test]
    fn same_version_normalizes_v_prefix() {
        assert!(same_version("v0.2.0", "0.2.0"));
        assert!(same_version("0.2.0", "v0.2.0"));
        assert!(same_version("v1.0", "v1.0"));
        assert!(!same_version("v0.2.0", "0.2.1"));
    }

    #[test]
    fn upgrade_global_unknown_name_errors() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("upgrade-unknown");
        std::env::set_var("STRYKE_HOME", &home);
        // Empty index + explicit NAME → exit 1 (typo protection).
        assert_eq!(cmd_upgrade_global(&["nosuchpkg".to_string()]), 1);
        // Empty index, no NAME → nothing to do, exit 0.
        assert_eq!(cmd_upgrade_global(&[]), 0);
        std::env::remove_var("STRYKE_HOME");
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
        let names: Vec<(String, String)> = orphans.into_iter().map(|(n, v, _)| (n, v)).collect();
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
        assert_eq!(
            reloaded.find("gui").unwrap().version,
            "0.1.0",
            "pin must be untouched"
        );
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
        let r = resolve_module(&root, "Foo::Bar", None).unwrap().unwrap();
        assert!(r.ends_with("lib/Foo/Bar.stk"), "got {:?}", r);
    }

    /// Flat-layout namespace bridge — every `stryke-*` connector ships
    /// `lib/<Sub>.stk` declaring `package <Ns>::<Sub>` with no
    /// `lib/<Ns>/` subdirectory (stryke-arrow, stryke-aws, stryke-gcp,
    /// …). `use <Ns>::<Sub>` from inside the project must chase
    /// `lib/<Sub>.stk` via the `[ffi].namespace` match, otherwise
    /// `stryke t` (and every reverse-dependency build) blows up with
    /// `Can't locate <Ns>/<Sub>.pm in @INC`.
    #[test]
    fn resolve_module_local_lib_flat_layout_via_ffi_namespace() {
        let root = tempdir("proj-flat");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"stryke-arrow\"\nversion=\"0.1.0\"\n\
             [ffi]\nlib-name=\"stryke_arrow\"\nnamespace=\"Arrow\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/Parquet.stk"), b"# parquet").unwrap();
        let r = resolve_module(&root, "Arrow::Parquet", None)
            .unwrap()
            .unwrap();
        assert!(r.ends_with("lib/Parquet.stk"), "got {:?}", r);
    }

    /// Namespace bridge must be case-insensitive — `[ffi].namespace =
    /// "AWS"` accepts `use aws::S3` and vice-versa, matching how the
    /// global-store branch lower-cases segments[0] for canonical_names.
    #[test]
    fn resolve_module_local_lib_flat_layout_case_insensitive() {
        let root = tempdir("proj-flat-case");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"stryke-aws\"\nversion=\"0.1.0\"\n\
             [ffi]\nlib-name=\"stryke_aws\"\nnamespace=\"AWS\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/S3.stk"), b"# s3").unwrap();
        let r = resolve_module(&root, "aws::S3", None).unwrap().unwrap();
        assert!(r.ends_with("lib/S3.stk"), "got {:?}", r);
    }

    /// Namespace bridge must NOT shortcut when the first segment doesn't
    /// match `[ffi].namespace` — otherwise `use Other::Foo` would
    /// silently bind to this project's `lib/Foo.stk`.
    #[test]
    fn resolve_module_local_lib_flat_layout_only_fires_on_namespace_match() {
        let root = tempdir("proj-flat-mismatch");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"stryke-arrow\"\nversion=\"0.1.0\"\n\
             [ffi]\nlib-name=\"stryke_arrow\"\nnamespace=\"Arrow\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/Parquet.stk"), b"# parquet").unwrap();
        let r = resolve_module(&root, "Other::Parquet", None).unwrap();
        assert!(r.is_none(), "must not bridge across namespaces: {:?}", r);
    }

    /// Pure-stryke packages with no `[ffi]` table still need the bridge.
    /// `stryke-utils` ships `lib/String.stk` declaring `package
    /// Utils::String` and a top-level `[package].name = "stryke-utils"`
    /// with no `[ffi]` block — `use Utils::String` from inside the
    /// project must chase `lib/String.stk` via the `stryke-<ns>` name
    /// arm. Without this, the umbrella `use Utils` triggers a chain of
    /// `Can't locate Utils/<Sub>.pm in @INC` errors and every assertion
    /// past `Utils::version()` fails.
    #[test]
    fn resolve_module_local_lib_flat_layout_via_stryke_prefix_pkg_name() {
        let root = tempdir("proj-flat-pure");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"stryke-utils\"\nversion=\"0.1.1\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/String.stk"), b"# string").unwrap();
        let r = resolve_module(&root, "Utils::String", None)
            .unwrap()
            .unwrap();
        assert!(r.ends_with("lib/String.stk"), "got {:?}", r);
    }

    /// Pure-stryke bridge stays scoped: an unrelated `use Foo::Bar` from
    /// inside `stryke-utils` must NOT bind to `lib/Bar.stk`.
    #[test]
    fn resolve_module_local_lib_flat_layout_pkg_name_bridge_scoped() {
        let root = tempdir("proj-flat-pure-mismatch");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"stryke-utils\"\nversion=\"0.1.1\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("lib/Bar.stk"), b"# bar").unwrap();
        let r = resolve_module(&root, "Foo::Bar", None).unwrap();
        assert!(r.is_none(), "must not bridge unrelated namespaces: {:?}", r);
    }

    #[test]
    fn resolve_module_falls_back_when_nothing_resolves() {
        let root = tempdir("proj");
        std::fs::write(
            root.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();
        let r = resolve_module(&root, "Foo::Bar", None).unwrap();
        assert!(r.is_none());
    }

    /// Legacy install (or first-install on a fresh machine) where the index
    /// entry has the prefixed name `stryke-gui` but no namespace recorded.
    /// `use GUI` must still resolve via the `stryke-<lowername>` fallback —
    /// otherwise older machines get a Perl @INC fallback and the user sees
    /// `Can't locate GUI.pm`.
    #[test]
    fn resolve_module_resolves_via_stryke_prefix_when_namespace_empty() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-prefix");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        let pkg_dir = store.package_dir("stryke-gui", "0.3.0");
        std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
        std::fs::write(pkg_dir.join("lib/GUI.stk"), b"sub g { 1 }").unwrap();

        let mut idx = InstalledIndex::new();
        // Empty namespace simulates a legacy entry written before the
        // install path populated the namespace field.
        idx.upsert("stryke-gui", "0.3.0", "github:MenkeTechnologies/stryke-gui");
        idx.save_to(&store).unwrap();

        let proj = tempdir("proj-no-local-gui");
        let r = resolve_module(&proj, "GUI", None).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("GUI should resolve to stryke-gui via prefix fallback");
        assert!(r.ends_with("store/stryke-gui@0.3.0/lib/GUI.stk"), "got {:?}", r);
    }

    /// "stryke use must respect package version" — use-site `use Foo 1.0`
    /// outside any project must land on `<store>/foo@1.0/` directly,
    /// even when the global installed.toml records a different version.
    #[test]
    fn resolve_module_pin_version_lands_on_exact_store_dir() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-pin-exact");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Two versions on disk; installed.toml pins the newer one.
        for v in ["1.0", "2.0"] {
            let pkg_dir = store.package_dir("foo", v);
            std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
            std::fs::write(
                pkg_dir.join("lib/Foo.stk"),
                format!("# foo {}\n", v).as_bytes(),
            )
            .unwrap();
        }
        let mut idx = InstalledIndex::new();
        idx.upsert("foo", "2.0", "test");
        idx.save_to(&store).unwrap();

        let proj = tempdir("proj-pin-exact");
        let r = resolve_module(&proj, "Foo", Some("1.0")).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("Foo 1.0 should resolve directly to foo@1.0");
        assert!(
            r.ends_with("store/foo@1.0/lib/Foo.stk"),
            "use-site pin must bypass installed.toml; got {:?}",
            r
        );
    }

    /// Pin requested but the version isn't in the store — resolver
    /// MUST refuse to fall through to a different version. The whole
    /// point of the pin is "this version or nothing".
    #[test]
    fn resolve_module_pin_version_missing_errors_not_substitutes() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-pin-missing");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Only 2.0 in store; user pins 1.0 which doesn't exist.
        let pkg_dir = store.package_dir("foo", "2.0");
        std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
        std::fs::write(pkg_dir.join("lib/Foo.stk"), b"# foo 2.0\n").unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("foo", "2.0", "test");
        idx.save_to(&store).unwrap();

        let proj = tempdir("proj-pin-missing");
        let r = resolve_module(&proj, "Foo", Some("1.0"));
        std::env::remove_var("STRYKE_HOME");

        assert!(
            r.is_err(),
            "missing pin must error, not silently substitute 2.0; got {:?}",
            r
        );
    }

    /// Outside-project resolution must pick the HIGHEST version of the
    /// package in the store — not whatever installed.toml last
    /// upserted, and not the lexicographically-largest dir name.
    /// `2.0` beats `1.99` (numeric tuple compare).
    #[test]
    fn resolve_module_outside_project_picks_highest_version() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-highest");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        for v in ["1.99", "2.0", "0.5"] {
            let pkg_dir = store.package_dir("foo", v);
            std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
            std::fs::write(
                pkg_dir.join("lib/Foo.stk"),
                format!("# foo {}\n", v).as_bytes(),
            )
            .unwrap();
        }
        // No installed.toml entry — proves the scan path is what
        // picks the version, not the index.

        let proj = tempdir("proj-highest");
        let r = resolve_module(&proj, "Foo", None).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("foo should resolve to highest version on disk");
        assert!(
            r.ends_with("store/foo@2.0/lib/Foo.stk"),
            "expected foo@2.0 (highest), got {:?}",
            r
        );
    }

    /// Highest-version scan must respect canonical-name mapping —
    /// `use GUI` should pick the highest `stryke-gui@*/` even though
    /// the bare `gui@*/` form doesn't exist on disk.
    #[test]
    fn resolve_module_highest_version_bridges_stryke_prefix() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-bridge-highest");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        for v in ["0.3.0", "0.10.0", "0.2.0"] {
            let pkg_dir = store.package_dir("stryke-gui", v);
            std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
            std::fs::write(
                pkg_dir.join("lib/GUI.stk"),
                format!("# gui {}\n", v).as_bytes(),
            )
            .unwrap();
        }

        let proj = tempdir("proj-bridge-highest");
        let r = resolve_module(&proj, "GUI", None).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("GUI should bridge to stryke-gui and pick highest");
        assert!(
            r.ends_with("store/stryke-gui@0.10.0/lib/GUI.stk"),
            "0.10.0 > 0.3.0 numerically — got {:?}",
            r
        );
    }

    /// Pre-release identifier loses to release (semver §11). `1.0.0-rc1`
    /// must rank below `1.0.0`, not tie with it. Without this rule the
    /// scan returns whichever extraction the filesystem yields first
    /// — flaky cross-platform, flaky across `s install` orderings.
    #[test]
    fn resolve_module_release_wins_over_pre_release() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-prerelease");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Both extracted side-by-side. Release MUST win.
        for v in ["1.0.0-rc1", "1.0.0", "1.0.0-beta"] {
            let pkg_dir = store.package_dir("foo", v);
            std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
            std::fs::write(
                pkg_dir.join("lib/Foo.stk"),
                format!("# foo {}\n", v).as_bytes(),
            )
            .unwrap();
        }

        let proj = tempdir("proj-prerelease");
        let r = resolve_module(&proj, "Foo", None).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("foo should resolve");
        assert!(
            r.ends_with("foo@1.0.0/lib/Foo.stk"),
            "release 1.0.0 must beat 1.0.0-rc1 / 1.0.0-beta; got {:?}",
            r
        );
    }

    /// Within pre-releases, the semver §11 ordering is `alpha < beta
    /// < rc`. Stryke's resolver scan doesn't need full semver — it
    /// only needs *deterministic* ranking — but the resolver should
    /// at least keep the higher-numbered pre-release suffix on top
    /// of the lower one (`rc2 > rc1`).
    #[test]
    fn resolve_module_pre_release_ordered_by_numeric_suffix() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-prerelease-suffix");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Only pre-releases on disk — release isn't out yet.
        for v in ["1.0.0-rc1", "1.0.0-rc2", "1.0.0-rc10"] {
            let pkg_dir = store.package_dir("foo", v);
            std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
            std::fs::write(
                pkg_dir.join("lib/Foo.stk"),
                format!("# foo {}\n", v).as_bytes(),
            )
            .unwrap();
        }

        let proj = tempdir("proj-prerelease-suffix");
        let r = resolve_module(&proj, "Foo", None).unwrap();
        std::env::remove_var("STRYKE_HOME");

        let r = r.expect("foo should resolve to highest pre-release");
        assert!(
            r.ends_with("foo@1.0.0-rc10/lib/Foo.stk"),
            "rc10 > rc2 > rc1 numerically; got {:?}",
            r
        );
    }

    /// Lockfile pins a version whose store dir is missing. Resolver MUST
    /// error rather than silently fall through to the global index —
    /// that fall-through was the version-disrespect bug.
    #[test]
    fn resolve_module_lockfile_pin_missing_errors_not_substitutes() {
        let _g = STRYKE_HOME_MUTEX.lock().unwrap();
        let home = tempdir("stryke-home-lock-missing");
        std::env::set_var("STRYKE_HOME", &home);

        let store = Store::user_default().unwrap();
        store.ensure_layout().unwrap();
        // Global has foo@2.0 (latest install). Project lockfile pins
        // foo@1.0 but the store doesn't have that extracted.
        let pkg_dir = store.package_dir("foo", "2.0");
        std::fs::create_dir_all(pkg_dir.join("lib")).unwrap();
        std::fs::write(pkg_dir.join("lib/Foo.stk"), b"# foo 2.0\n").unwrap();
        let mut idx = InstalledIndex::new();
        idx.upsert("foo", "2.0", "test");
        idx.save_to(&store).unwrap();

        let proj = tempdir("proj-lock-missing");
        std::fs::write(
            proj.join(MANIFEST_FILE),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\n[deps]\nfoo = \"1.0\"\n",
        )
        .unwrap();
        let mut lf = Lockfile::new();
        lf.packages.push(super::super::lockfile::LockedPackage {
            name: "foo".into(),
            version: "1.0".into(),
            source: "registry+test".into(),
            integrity: "sha256-deadbeef".into(),
            features: vec![],
            deps: vec![],
        });
        std::fs::write(
            proj.join(LOCKFILE_FILE),
            lf.to_toml_string().unwrap(),
        )
        .unwrap();

        let r = resolve_module(&proj, "Foo", None);
        std::env::remove_var("STRYKE_HOME");

        assert!(
            r.is_err(),
            "lockfile pin missing in store must error, not silently use foo@2.0; got {:?}",
            r
        );
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

        assert_eq!(
            resolved.as_deref(),
            Some(pkg_dir.join("lib/GUI.stk").as_path())
        );
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
        assert!(!crate::lsp::line_completion_is_use_context(
            line,
            line.len()
        ));
        let line = "use overload '+'";
        assert!(!crate::lsp::line_completion_is_use_context(
            line,
            line.len()
        ));
        let line = "GUI::mouse_pos";
        assert!(!crate::lsp::line_completion_is_use_context(
            line,
            line.len()
        ));
        let line = "p use_count";
        assert!(!crate::lsp::line_completion_is_use_context(
            line,
            line.len()
        ));
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

        let hover = crate::lsp::hover_markdown_for_word("GUI", text, script_path.to_str().unwrap());
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

    // === parse_github_shorthand =========================================
    //
    // `s add github.com/OWNER/REPO[@TAG]` must produce a git dep, not a
    // registry dep (which would fail `s install` with the RFC-phases-7-8
    // unimplemented error). These tests pin both directions: shorthands
    // that should be recognized, and forms that look github-ish but
    // shouldn't quietly convert (sub-paths, http://, malformed tags).

    #[test]
    fn github_shorthand_bare_form() {
        let gh = parse_github_shorthand("github.com/MenkeTechnologies/stryke-parquet").unwrap();
        assert_eq!(gh.name, "stryke-parquet");
        assert_eq!(gh.owner_repo, "MenkeTechnologies/stryke-parquet");
        assert!(gh.tag.is_none());
    }

    #[test]
    fn github_shorthand_https_form() {
        let gh =
            parse_github_shorthand("https://github.com/MenkeTechnologies/stryke-aws").unwrap();
        assert_eq!(gh.name, "stryke-aws");
        assert_eq!(gh.owner_repo, "MenkeTechnologies/stryke-aws");
        assert!(gh.tag.is_none());
    }

    #[test]
    fn github_shorthand_strips_dot_git() {
        let gh =
            parse_github_shorthand("https://github.com/MenkeTechnologies/stryke-aws.git").unwrap();
        assert_eq!(gh.name, "stryke-aws");
        assert_eq!(gh.owner_repo, "MenkeTechnologies/stryke-aws");
    }

    #[test]
    fn github_shorthand_with_tag() {
        let gh =
            parse_github_shorthand("github.com/MenkeTechnologies/stryke-aws@v0.2.0").unwrap();
        assert_eq!(gh.name, "stryke-aws");
        assert_eq!(gh.owner_repo, "MenkeTechnologies/stryke-aws");
        assert_eq!(gh.tag.as_deref(), Some("v0.2.0"));
    }

    #[test]
    fn github_shorthand_dot_git_with_tag() {
        let gh = parse_github_shorthand(
            "https://github.com/MenkeTechnologies/stryke-aws.git@v0.2.0",
        )
        .unwrap();
        assert_eq!(gh.name, "stryke-aws");
        assert_eq!(gh.tag.as_deref(), Some("v0.2.0"));
    }

    #[test]
    fn github_shorthand_rejects_non_github() {
        // Random crate name — must fall through to registry path.
        assert!(parse_github_shorthand("serde").is_none());
        assert!(parse_github_shorthand("http@1.0").is_none());
        // GitLab / other hosts — not in scope for this shorthand.
        assert!(parse_github_shorthand("gitlab.com/foo/bar").is_none());
        assert!(parse_github_shorthand("https://gitlab.com/foo/bar").is_none());
    }

    #[test]
    fn github_shorthand_rejects_subpath() {
        // `github.com/owner/repo/subdir` is not a valid git source URL;
        // the parser must NOT silently truncate to `repo`.
        assert!(parse_github_shorthand("github.com/owner/repo/subdir").is_none());
        assert!(parse_github_shorthand("github.com/owner/repo/tree/main").is_none());
    }

    #[test]
    fn github_shorthand_rejects_empty_owner_or_repo() {
        assert!(parse_github_shorthand("github.com/").is_none());
        assert!(parse_github_shorthand("github.com//repo").is_none());
        assert!(parse_github_shorthand("github.com/owner/").is_none());
        assert!(parse_github_shorthand("github.com/owner/.git").is_none());
    }

    #[test]
    fn github_shorthand_rejects_empty_tag() {
        // Trailing `@` with no tag is malformed (user-typo of `@<TAB>`).
        assert!(parse_github_shorthand("github.com/owner/repo@").is_none());
    }

    // === cmd_add integration ============================================

    #[test]
    fn parse_add_args_github_shorthand_produces_github_release_dep() {
        let args = vec!["github.com/MenkeTechnologies/stryke-parquet".to_string()];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "stryke-parquet");
        match parsed.spec {
            DepSpec::Detailed(d) => {
                assert_eq!(
                    d.github.as_deref(),
                    Some("MenkeTechnologies/stryke-parquet")
                );
                // No `version` → resolver fetches the latest release.
                assert!(d.version.is_none());
                // Important: must NOT write a `git` URL. That would route
                // through install_git_dep (source clone), which is wrong
                // for FFI packages — they need the release tarball.
                assert!(d.git.is_none(), "github shorthand must not also set git");
                assert!(d.branch.is_none());
                assert!(d.path.is_none());
            }
            other => panic!("expected DepSpec::Detailed github dep, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_args_github_shorthand_with_tag_pins_version() {
        let args = vec!["github.com/MenkeTechnologies/stryke-aws@v0.2.0".to_string()];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "stryke-aws");
        match parsed.spec {
            DepSpec::Detailed(d) => {
                assert_eq!(
                    d.github.as_deref(),
                    Some("MenkeTechnologies/stryke-aws")
                );
                // `@TAG` lands in the `version` field — that's what
                // install_global_from_github uses to construct the
                // release-tarball URL.
                assert_eq!(d.version.as_deref(), Some("v0.2.0"));
                assert!(d.git.is_none());
            }
            other => panic!("expected DepSpec::Detailed github dep, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_args_path_flag_wins_over_github_shorthand() {
        // Explicit `--path=` must still take precedence — the user opted
        // into a local checkout even though the SPEC looks github-shaped.
        let args = vec![
            "github.com/MenkeTechnologies/stryke-aws".to_string(),
            "--path=../local-aws".to_string(),
        ];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "stryke-aws");
        match parsed.spec {
            DepSpec::Detailed(d) => {
                assert_eq!(d.path.as_deref(), Some("../local-aws"));
                assert!(d.git.is_none(), "git must be None when --path overrides");
            }
            other => panic!("expected DepSpec::Detailed path dep, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_args_registry_form_still_returns_version_spec() {
        // Backstop the existing behavior for plain registry deps.
        let args = vec!["http@1.0".to_string()];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "http");
        match parsed.spec {
            DepSpec::Version(v) => assert_eq!(v, "1.0"),
            other => panic!("expected DepSpec::Version, got {:?}", other),
        }
    }

    // === parse_local_path_arg ===========================================
    //
    // `s add ./mylib` and friends should write a path dep, not a registry
    // dep. These pin both the sigil-driven cases (where the directory
    // doesn't have to exist — useful for "about to create") and the
    // exists-on-disk auto-detection (where bare names like `mylib`
    // become path deps when there's a `./mylib` next to the cwd).

    #[test]
    fn local_path_arg_relative_dot_slash() {
        let lp =
            parse_local_path_arg("./mylib").expect("./mylib should parse as path");
        assert_eq!(lp.name, "mylib");
        assert_eq!(lp.path_for_manifest, "./mylib");
    }

    #[test]
    fn local_path_arg_relative_parent() {
        let lp =
            parse_local_path_arg("../sibling").expect("../sibling should parse as path");
        assert_eq!(lp.name, "sibling");
        assert_eq!(lp.path_for_manifest, "../sibling");
    }

    #[test]
    fn local_path_arg_absolute_nonexistent_still_accepted() {
        // Absolute sigil is enough to disambiguate — the directory
        // doesn't need to exist yet at the moment of `s add`.
        let lp = parse_local_path_arg("/tmp/will-create-later")
            .expect("absolute sigil should parse as path");
        assert_eq!(lp.name, "will-create-later");
        assert_eq!(lp.path_for_manifest, "/tmp/will-create-later");
    }

    #[test]
    fn local_path_arg_tilde_expands_in_manifest() {
        let home = std::env::var("HOME").expect("HOME set in test env");
        let lp = parse_local_path_arg("~/projects/mylib")
            .expect("~/PATH should parse as path");
        assert_eq!(lp.name, "mylib");
        assert_eq!(
            lp.path_for_manifest,
            format!("{}/projects/mylib", home.trim_end_matches('/'))
        );
    }

    #[test]
    fn local_path_arg_reads_pkg_name_when_manifest_present() {
        // When the targeted directory has a stryke.toml with
        // [package].name, that name takes precedence over the
        // directory basename. Common when the dep dir is `crates/foo`
        // but the package is named `foo-lib`.
        let d = tempdir("path-arg-name");
        let pkg_dir = d.join("crates").join("foo");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(MANIFEST_FILE),
            "[package]\nname=\"foo-lib\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();

        let raw = pkg_dir.to_str().unwrap();
        let lp = parse_local_path_arg(raw).expect("existing dir should parse as path");
        assert_eq!(lp.name, "foo-lib");
        assert_eq!(lp.path_for_manifest, raw);
    }

    #[test]
    fn local_path_arg_existing_dir_without_sigil_auto_detected() {
        // No sigil, but the directory exists — auto-detect as path.
        let d = tempdir("path-arg-bare-existing");
        let pkg_dir = d.join("mylib");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join(MANIFEST_FILE),
            "[package]\nname=\"mylib\"\nversion=\"0.1.0\"\n",
        )
        .unwrap();

        let lp =
            parse_local_path_arg(pkg_dir.to_str().unwrap()).expect("existing dir → path dep");
        assert_eq!(lp.name, "mylib");
    }

    #[test]
    fn local_path_arg_rejects_registry_names() {
        // Plain crate-style names without a sigil and without an
        // existing on-disk dir must NOT be parsed as paths (the user
        // wants the registry path → `DepSpec::Version("*")`).
        assert!(parse_local_path_arg("serde").is_none());
        assert!(parse_local_path_arg("stryke-arrow").is_none());
    }

    #[test]
    fn local_path_arg_rejects_version_suffixed_names() {
        // `http@1.0` is a registry version, not a path.
        assert!(parse_local_path_arg("http@1.0").is_none());
        assert!(parse_local_path_arg("./mylib@1.0").is_none());
    }

    #[test]
    fn local_path_arg_rejects_url_like_shapes() {
        // URLs contain `:` — those go to git/github paths, not local.
        assert!(parse_local_path_arg("file:///tmp/mylib").is_none());
        assert!(parse_local_path_arg("https://example.com/mylib").is_none());
    }

    #[test]
    fn parse_add_args_relative_path_becomes_path_dep() {
        let args = vec!["./mylib".to_string()];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "mylib");
        match parsed.spec {
            DepSpec::Detailed(d) => {
                assert_eq!(d.path.as_deref(), Some("./mylib"));
                assert!(d.git.is_none());
                assert!(d.version.is_none());
            }
            other => panic!("expected DepSpec::Detailed path dep, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_args_absolute_path_becomes_path_dep() {
        let args = vec!["/work/vendored/mylib".to_string()];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        assert_eq!(parsed.name, "mylib");
        match parsed.spec {
            DepSpec::Detailed(d) => {
                assert_eq!(d.path.as_deref(), Some("/work/vendored/mylib"));
            }
            other => panic!("expected DepSpec::Detailed path dep, got {:?}", other),
        }
    }

    #[test]
    fn parse_add_args_path_override_flag_wins_over_positional_path() {
        // User passes BOTH a path-shaped positional AND --path= — the
        // explicit flag wins, but the positional's NAME is still
        // extracted from itself (since it was being treated as a
        // local path sigil even before the override). With both
        // path-shapes, the flag's directory is the source of truth.
        let args = vec![
            "./aaa".to_string(),
            "--path=../bbb".to_string(),
        ];
        let parsed = parse_add_args(&args).expect("parse should succeed");
        // `./aaa` is detected as a github-shorthand miss → falls through
        // to the standard path/features/version branches below. With
        // --path=../bbb set, the branch becomes "path dep with version
        // = None, path = ../bbb". The name comes from the @-split of
        // `./aaa`, which yields `./aaa` as the manifest key.
        // (The path-positional auto-detection above is gated by
        // `path_override.is_none()`, so --path takes priority.)
        match parsed.spec {
            DepSpec::Detailed(d) => assert_eq!(d.path.as_deref(), Some("../bbb")),
            other => panic!("expected DepSpec::Detailed path dep, got {:?}", other),
        }
    }
}
