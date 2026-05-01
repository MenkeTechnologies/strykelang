//! `s_web s [--port N]` — boot the dev server.
//!
//! Today: shells out to `stryke bin/server` with the requested port. Once
//! the framework runtime ships as builtins this can be replaced with a
//! direct `boot_application(port)` call (skipping the subprocess).
//!
//! `s_web console` — boots a stryke REPL with the app's config + autoload
//! paths preloaded so models and helpers are available at the prompt.

use crate::util::Result;
use std::path::Path;
use std::process::Command;

pub fn run(port: u16) -> Result<()> {
    if !Path::new("bin/server").exists() {
        return Err(
            "bin/server not found — run this from inside an app directory \
             (or run `s_web new APP` first)."
                .into(),
        );
    }
    let status = Command::new("stryke")
        .arg("bin/server")
        .env("PORT", port.to_string())
        .status()?;
    if !status.success() {
        return Err(format!("server exited with {}", status).into());
    }
    Ok(())
}

pub fn console() -> Result<()> {
    if !Path::new("config/application.stk").exists() {
        return Err(
            "config/application.stk not found — run from an app directory.".into(),
        );
    }
    let status = Command::new("stryke")
        .args(["-r", "config/application.stk", "--repl"])
        .status()?;
    if !status.success() {
        return Err(format!("console exited with {}", status).into());
    }
    Ok(())
}
