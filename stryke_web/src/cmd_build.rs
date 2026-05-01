//! `s_web build [--out DIR]` — emit a self-contained Rust wrapper
//! crate that include_str!s every `.stk` file from the user's app +
//! the framework runtime. After this writes the wrapper, the user
//! runs `cargo build --release` inside it to get a single binary that
//! ships the whole app.
//!
//! No deps beyond libc + libssl. SQLite is statically linked via the
//! `rusqlite` "bundled" feature already in strykelang's Cargo.toml.

use crate::util::Result;
use std::path::{Path, PathBuf};

const WRAPPER_MAIN: &str = include_str!("../templates/devops/wrapper_main.rs");
const WRAPPER_CARGO: &str = include_str!("../templates/devops/wrapper_cargo.toml");

pub fn run(out_dir: Option<&str>, app_name_override: Option<&str>) -> Result<()> {
    if !Path::new("config/application.stk").exists() {
        return Err(
            "config/application.stk not found — run from an app directory.".into(),
        );
    }

    let cwd = std::env::current_dir()?;
    let app_basename = cwd
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("app")
        .to_string();
    let app_name = app_name_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| sanitize_crate_name(&app_basename));

    let out = out_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("dist"));
    std::fs::create_dir_all(&out)?;
    std::fs::create_dir_all(out.join("src"))?;

    // Walk the app dir and collect every file we want embedded.
    let mut entries: Vec<(String, String)> = Vec::new();
    for top in [
        "config",
        "app",
        "db/migrate",
        "db/seeds.stk",
        "public",
    ] {
        collect_files(&cwd, &cwd.join(top), &mut entries)?;
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut embedded_lit = String::new();
    for (rel, body) in &entries {
        embedded_lit.push_str(&format!(
            "    ({:?}, include_str!({:?})),\n",
            rel,
            cwd.join(rel).display().to_string()
        ));
    }

    // Path to strykelang. Prefer the one next to the s_web binary's
    // crate (most reliable — written into the binary at compile time).
    // Fall back to walking up from cwd for users who reorganized the
    // tree.
    let strykelang_path = compiled_in_strykelang_path()
        .or_else(|| locate_strykelang_path(&cwd))
        .unwrap_or_else(|| PathBuf::from("../../strykelang"));

    let cargo_toml = WRAPPER_CARGO
        .replace("{{app_name}}", &app_name)
        .replace(
            "{{strykelang_path}}",
            &strykelang_path.display().to_string(),
        );
    let main_rs = WRAPPER_MAIN.replace("{{embedded_entries}}", &embedded_lit);

    std::fs::write(out.join("Cargo.toml"), cargo_toml)?;
    std::fs::write(out.join("src/main.rs"), main_rs)?;

    println!();
    println!("Wrote self-contained wrapper crate at:");
    println!("  {}", out.display());
    println!();
    println!("Embedded {} files. Build the fat binary:", entries.len());
    println!("  cd {}", out.display());
    println!("  cargo build --release");
    println!();
    println!("Resulting binary: {}/target/release/{}", out.display(), app_name);
    println!("Run with: ./{} (no other deps required)", app_name);
    Ok(())
}

fn sanitize_crate_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else if c == '-' || c == ' ' {
            out.push('_');
        }
    }
    if out.is_empty() {
        return "app".to_string();
    }
    if out.chars().next().unwrap().is_ascii_digit() {
        out.insert(0, '_');
    }
    out
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    if dir.is_file() {
        let rel = dir.strip_prefix(root).unwrap().to_string_lossy().to_string();
        let body = std::fs::read_to_string(dir).unwrap_or_default();
        out.push((rel, body));
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip caches and lock files.
        if name == ".keep" || name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            // Skip user-data SQLite files — the binary should ship a
            // schema, not your dev rows.
            if name.ends_with(".sqlite3") || name.contains(".sqlite3-") {
                continue;
            }
            // Cap embedded size — 5 MB per file is more than any
            // reasonable template should need.
            let meta = std::fs::metadata(&path)?;
            if meta.len() > 5 * 1024 * 1024 {
                eprintln!(
                    "  warn: skipping {} ({} bytes; over 5 MB cap)",
                    path.display(),
                    meta.len()
                );
                continue;
            }
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().to_string();
            let body = std::fs::read_to_string(&path).unwrap_or_default();
            out.push((rel, body));
        }
    }
    Ok(())
}

fn locate_strykelang_path(cwd: &Path) -> Option<PathBuf> {
    // Walk up looking for a `strykelang/` sibling — covers the dev
    // workflow where the user is hacking inside the strykelang repo.
    let mut p: PathBuf = cwd.to_path_buf();
    for _ in 0..6 {
        let candidate = p.join("strykelang");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !p.pop() {
            break;
        }
    }
    None
}

/// Path to the strykelang crate root. In this repo the `strykelang`
/// package is defined in the workspace root's Cargo.toml (with
/// `lib.path = strykelang/lib.rs`), so we point at the parent of
/// `stryke_web/`, not at `stryke_web/../strykelang/`.
fn compiled_in_strykelang_path() -> Option<PathBuf> {
    let stryke_web_dir = env!("CARGO_MANIFEST_DIR");
    let candidate = Path::new(stryke_web_dir).parent()?;
    if candidate.join("Cargo.toml").is_file() {
        candidate.canonicalize().ok()
    } else {
        None
    }
}
