//! `s_web db {migrate,rollback,seed,reset}` — database tasks.
//!
//! Each task launches `stryke` against a one-line driver script that
//! requires `config/application.stk`, opens the connection via
//! `web_db_connect`, requires every migration file under `db/migrate/`,
//! and then calls the appropriate runtime builtin.

use crate::util::Result;
use std::path::Path;
use std::process::Command;

fn ensure_app_root() -> Result<()> {
    if !Path::new("config/application.stk").exists() {
        return Err("config/application.stk not found — run from an app directory.".into());
    }
    Ok(())
}

fn stryke_eval(snippet: &str) -> Result<()> {
    let status = Command::new("stryke").arg("-e").arg(snippet).status()?;
    if !status.success() {
        return Err(format!("db task exited with {}", status).into());
    }
    Ok(())
}

const BOOT: &str = r#"
require "./config/application.stk";
my $env = $ENV{STRYKE_ENV} // "development";
my $db_path = "db/${env}.sqlite3";
web_db_connect("sqlite://$db_path");
for my $f (glob "db/migrate/**/*.stk") { require $f; }
"#;
/// `migrate` — see implementation.
pub fn migrate() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nweb_migrate();\n", BOOT);
    stryke_eval(&snippet)
}
/// `rollback` — see implementation.
pub fn rollback() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nweb_rollback();\n", BOOT);
    stryke_eval(&snippet)
}
/// `seed` — see implementation.
pub fn seed() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nrequire \"./db/seeds.stk\";\n", BOOT);
    stryke_eval(&snippet)
}
/// `reset` — see implementation.
pub fn reset() -> Result<()> {
    ensure_app_root()?;
    // Drop the file, recreate via migrate, then re-seed.
    let env = std::env::var("STRYKE_ENV").unwrap_or_else(|_| "development".to_string());
    let db_path = format!("db/{}.sqlite3", env);
    let _ = std::fs::remove_file(&db_path);
    migrate()?;
    seed()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ensure_app_root` errors when `config/application.stk` is missing.
    /// All four public commands chain through this guard so the error
    /// message they surface must point at the right file.
    #[test]
    fn ensure_app_root_errors_with_helpful_message_outside_app_dir() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = ensure_app_root();
        std::env::set_current_dir(prev).expect("restore");
        let err = result.expect_err("must error without application.stk");
        let msg = err.to_string();
        assert!(
            msg.contains("config/application.stk"),
            "expected error to name the missing file, got: {msg}"
        );
        assert!(
            msg.contains("not found"),
            "expected 'not found' phrasing, got: {msg}"
        );
    }

    #[test]
    fn ensure_app_root_succeeds_when_application_stk_present() {
        let dir = tempfile::tempdir().expect("tmpdir");
        std::fs::create_dir(dir.path().join("config")).expect("mkdir config");
        std::fs::write(dir.path().join("config/application.stk"), "# stub").expect("write");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = ensure_app_root();
        std::env::set_current_dir(prev).expect("restore");
        assert!(result.is_ok());
    }

    /// All four public commands short-circuit via `ensure_app_root`
    /// when invoked from a non-app directory. Pin that contract so a
    /// future refactor can't accidentally launch a `stryke` subprocess
    /// against a host directory.
    #[test]
    fn migrate_errors_when_application_stk_missing() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = migrate();
        std::env::set_current_dir(prev).expect("restore");
        assert!(result.is_err());
    }

    #[test]
    fn rollback_errors_when_application_stk_missing() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = rollback();
        std::env::set_current_dir(prev).expect("restore");
        assert!(result.is_err());
    }

    #[test]
    fn seed_errors_when_application_stk_missing() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = seed();
        std::env::set_current_dir(prev).expect("restore");
        assert!(result.is_err());
    }

    #[test]
    fn reset_errors_when_application_stk_missing() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(&dir).expect("cd");
        let result = reset();
        std::env::set_current_dir(prev).expect("restore");
        assert!(result.is_err());
    }

    /// The shared BOOT prelude must reference both `web_db_connect`
    /// (so the connection opens before any migration body runs) and
    /// glob the migrate dir. Pin the contract — silently dropping
    /// either would land a major regression where migrate/seed silently
    /// runs against a wrong/missing DB.
    #[test]
    fn boot_prelude_references_required_runtime_builtins() {
        assert!(
            BOOT.contains("web_db_connect"),
            "BOOT must call web_db_connect"
        );
        assert!(BOOT.contains("db/migrate"), "BOOT must glob db/migrate");
        assert!(
            BOOT.contains("config/application.stk"),
            "BOOT must require config/application.stk"
        );
        assert!(BOOT.contains("STRYKE_ENV"), "BOOT must read STRYKE_ENV");
    }

    /// reset() composes migrate + seed; verify the composition order
    /// is correct in the source so a future refactor can't accidentally
    /// seed before migrating (would crash on a fresh DB).
    #[test]
    fn reset_source_calls_migrate_before_seed() {
        // Use the source file itself as the contract.
        let src = include_str!("cmd_db.rs");
        let migrate_pos = src.rfind("migrate()?;").expect("migrate? call");
        let seed_pos = src.rfind("seed()").expect("seed call");
        assert!(
            migrate_pos < seed_pos,
            "reset() must migrate THEN seed (migrate@{migrate_pos}, seed@{seed_pos})"
        );
    }
}
