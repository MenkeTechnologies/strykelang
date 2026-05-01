//! `s_web new APPNAME` — scaffold a brand-new stryke web app.
//!
//! Mirrors `rails new` exactly: lays out the directory tree, writes every
//! required file (config, controllers, models, views, db, public, bin),
//! optionally `git init`s, and prints the standard "now run …" hint.

use crate::templates::{self, render};
use crate::util::{ensure_dir, write_file, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(name: &str, skip_git: bool, database: &str) -> Result<()> {
    let root = PathBuf::from(name);
    if root.exists() {
        return Err(format!("`{}` already exists — pick a different name", name).into());
    }
    println!("Creating stryke web app at ./{}/", name);

    // Templates use `{{app_name}}` for the page <title> etc. Strip any
    // leading directories the user passed (e.g. `s_web new /tmp/foo`) so
    // the displayed name is just the basename.
    let display_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name);

    let mut vars = BTreeMap::new();
    vars.insert("app_name", display_name);
    vars.insert("database", database);

    // Top-level directories — same layout Rails ships.
    for d in &[
        "app/controllers",
        "app/models",
        "app/views/layouts",
        "app/views/admin",
        "app/views/sessions",
        "app/views/users",
        "app/helpers",
        "app/jobs",
        "app/mailers",
        "app/mailers/views",
        "app/channels",
        "bin",
        "config",
        "config/initializers",
        "config/environments",
        "db/migrate",
        "lib",
        "log",
        "public",
        "public/assets",
        "test/controllers",
        "test/models",
        "test/fixtures",
        "tmp",
        "vendor",
    ] {
        ensure_dir(&root.join(d))?;
    }

    // Static-ish files (each rendered through the template engine so
    // `{{app_name}}` / `{{database}}` substitutions land).
    write_file(&root.join("README.md"), &render(templates::APP_README, &vars))?;
    write_file(&root.join(".gitignore"), &render(templates::GITIGNORE, &vars))?;
    write_file(&root.join("stryke.toml"), &render(templates::STRYKE_TOML, &vars))?;

    write_file(
        &root.join("config/routes.stk"),
        &render(templates::ROUTES_STK, &vars),
    )?;
    write_file(
        &root.join("config/application.stk"),
        &render(templates::APPLICATION_STK, &vars),
    )?;
    write_file(
        &root.join("config/database.toml"),
        &render(templates::DATABASE_TOML, &vars),
    )?;

    write_file(
        &root.join("app/controllers/application_controller.stk"),
        &render(templates::APPLICATION_CONTROLLER_STK, &vars),
    )?;
    write_file(
        &root.join("app/models/application_record.stk"),
        &render(templates::APPLICATION_RECORD_STK, &vars),
    )?;
    write_file(
        &root.join("app/views/layouts/application.html.erb"),
        &render(templates::APPLICATION_LAYOUT_ERB, &vars),
    )?;
    write_file(
        &root.join("app/helpers/application_helper.stk"),
        &render(templates::APPLICATION_HELPER_STK, &vars),
    )?;

    write_file(
        &root.join("bin/server"),
        &render(templates::BIN_SERVER, &vars),
    )?;
    write_file(
        &root.join("db/seeds.stk"),
        &render(templates::DB_SEEDS_STK, &vars),
    )?;

    // chmod +x on bin/server.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let server = root.join("bin/server");
        let mut perms = std::fs::metadata(&server)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&server, perms)?;
    }

    // Empty placeholder so Git tracks the directories.
    for keep in &["log", "tmp", "vendor", "test/fixtures"] {
        write_file(&root.join(keep).join(".keep"), "")?;
    }

    if !skip_git {
        run_git_init(&root);
    }

    println!();
    println!("Done. Next:");
    println!("  cd {}", name);
    println!("  s_web g scaffold Post title:string body:text");
    println!("  s_web db migrate");
    println!("  bin/server");
    Ok(())
}

fn run_git_init(root: &Path) {
    let _ = Command::new("git").arg("init").arg("-q").current_dir(root).status();
    let _ = Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status();
}
