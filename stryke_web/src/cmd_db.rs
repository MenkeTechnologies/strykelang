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

pub fn migrate() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nweb_migrate();\n", BOOT);
    stryke_eval(&snippet)
}

pub fn rollback() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nweb_rollback();\n", BOOT);
    stryke_eval(&snippet)
}

pub fn seed() -> Result<()> {
    ensure_app_root()?;
    let snippet = format!("{}\nrequire \"./db/seeds.stk\";\n", BOOT);
    stryke_eval(&snippet)
}

pub fn reset() -> Result<()> {
    ensure_app_root()?;
    // Drop the file, recreate via migrate, then re-seed.
    let env = std::env::var("STRYKE_ENV").unwrap_or_else(|_| "development".to_string());
    let db_path = format!("db/{}.sqlite3", env);
    let _ = std::fs::remove_file(&db_path);
    migrate()?;
    seed()
}
