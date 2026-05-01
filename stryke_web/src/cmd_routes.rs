//! `s_web routes` — print the route table for the current app.
//!
//! Shells out to stryke with a snippet that loads `config/application.stk`
//! and `config/routes.stk` and prints the table via `web_routes_table`.

use crate::util::Result;
use std::path::Path;
use std::process::Command;

pub fn run() -> Result<()> {
    if !Path::new("config/routes.stk").exists() {
        return Err(
            "config/routes.stk not found — run from an app directory.".into(),
        );
    }
    let snippet = r#"
require "./config/application.stk";
require "./config/routes.stk";
print web_routes_table();
"#;
    let status = Command::new("stryke").arg("-e").arg(snippet).status()?;
    if !status.success() {
        return Err(format!("routes exited with {}", status).into());
    }
    Ok(())
}
