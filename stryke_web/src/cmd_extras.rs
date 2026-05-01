//! Extra generators: auth, admin, api, mailer, job, channel, theme.
//!
//! These complement `cmd_generate` (the per-resource scaffolder) and
//! `cmd_app` (the bulk preset runner) — together they make `s_web new`
//! a one-line full-stack app generator that JHipster-people can use
//! without writing any framework code.

use crate::util::{file_stem, force_write, plural_snake, write_file, Result};
use heck::ToPascalCase;
use std::path::{Path, PathBuf};

const APPLICATION_CSS_SIMPLE: &str = include_str!("../templates/themes/simple.css");
const APPLICATION_CSS_PICO: &str = include_str!("../templates/themes/pico.css");
const APPLICATION_CSS_BOOTSTRAP: &str = include_str!("../templates/themes/bootstrap.css");
const APPLICATION_CSS_TAILWIND: &str = include_str!("../templates/themes/tailwind.css");
const APPLICATION_CSS_DARK: &str = include_str!("../templates/themes/dark.css");
const APPLICATION_CSS_CYBERPUNK: &str = include_str!("../templates/themes/cyberpunk.css");
const APPLICATION_CSS_SYNTHWAVE: &str = include_str!("../templates/themes/synthwave.css");
const APPLICATION_CSS_TERMINAL: &str = include_str!("../templates/themes/terminal.css");
const APPLICATION_CSS_MATRIX: &str = include_str!("../templates/themes/matrix.css");
const LAYOUT_THEMED: &str = include_str!("../templates/themes/layout.html.erb");
const LAYOUT_CYBER: &str = include_str!("../templates/themes/layout_cyber.html.erb");

// ── Theme ──────────────────────────────────────────────────────────────

pub fn apply_theme(theme: &str, api: bool) -> Result<()> {
    if api {
        return Ok(());
    }
    let (css, layout_kind) = match theme {
        "simple" => (APPLICATION_CSS_SIMPLE, "plain"),
        "pico" => (APPLICATION_CSS_PICO, "plain"),
        "bootstrap" => (APPLICATION_CSS_BOOTSTRAP, "plain"),
        "tailwind" => (APPLICATION_CSS_TAILWIND, "plain"),
        "dark" => (APPLICATION_CSS_DARK, "plain"),
        "cyberpunk" => (APPLICATION_CSS_CYBERPUNK, "cyber"),
        "synthwave" => (APPLICATION_CSS_SYNTHWAVE, "cyber"),
        "terminal" => (APPLICATION_CSS_TERMINAL, "cyber"),
        "matrix" => (APPLICATION_CSS_MATRIX, "cyber"),
        other => {
            return Err(format!(
                "unknown theme `{}` — pick one of: simple, dark, pico, bootstrap, tailwind, cyberpunk, synthwave, terminal, matrix",
                other
            )
            .into());
        }
    };
    force_write(&PathBuf::from("public/assets/application.css"), css)?;
    let layout_src = if layout_kind == "cyber" {
        LAYOUT_CYBER
    } else {
        LAYOUT_THEMED
    };
    let layout = layout_src.replace("{{theme_name}}", theme);
    force_write(
        &PathBuf::from("app/views/layouts/application.html.erb"),
        &layout,
    )?;
    println!("Theme: {}", theme);
    Ok(())
}

// ── Auth scaffold ──────────────────────────────────────────────────────

const AUTH_USER_MODEL: &str = include_str!("../templates/auth/user.stk");
const AUTH_USER_MIGRATION: &str = include_str!("../templates/auth/create_users.stk");
const AUTH_SESSIONS_CONTROLLER: &str =
    include_str!("../templates/auth/sessions_controller.stk");
const AUTH_USERS_CONTROLLER: &str =
    include_str!("../templates/auth/users_controller.stk");
const AUTH_SIGNUP_VIEW: &str = include_str!("../templates/auth/signup.html.erb");
const AUTH_LOGIN_VIEW: &str = include_str!("../templates/auth/login.html.erb");

pub fn auth() -> Result<()> {
    ensure_app_root()?;

    // The auth versions of User model + UsersController + UsersController#new
    // view are richer than what `s_web g scaffold User …` writes, so
    // force-overwrite when a preset already created them. Sessions
    // controller / login view are auth-only so a plain create is fine.
    force_write(&PathBuf::from("app/models/user.stk"), AUTH_USER_MODEL)?;

    let ts = crate::util::migration_timestamp();
    let mig_name = format!("db/migrate/{}_create_users_with_auth.stk", ts);
    if !user_already_has_password_digest()? {
        write_file(&PathBuf::from(&mig_name), AUTH_USER_MIGRATION)?;
    } else {
        println!("note: existing users migration already has password_digest — skipping");
    }

    write_file(
        &PathBuf::from("app/controllers/sessions_controller.stk"),
        AUTH_SESSIONS_CONTROLLER,
    )?;
    force_write(
        &PathBuf::from("app/controllers/users_controller.stk"),
        AUTH_USERS_CONTROLLER,
    )?;
    force_write(
        &PathBuf::from("app/views/users/new.html.erb"),
        AUTH_SIGNUP_VIEW,
    )?;
    write_file(
        &PathBuf::from("app/views/sessions/new.html.erb"),
        AUTH_LOGIN_VIEW,
    )?;

    // Wire auth routes.
    append_routes(&[
        "web_route \"GET /signup\", \"users#new\"",
        "web_route \"POST /signup\", \"users#create\"",
        "web_route \"GET /login\", \"sessions#new\"",
        "web_route \"POST /login\", \"sessions#create\"",
        "web_route \"DELETE /logout\", \"sessions#destroy\"",
        "web_route \"POST /logout\", \"sessions#destroy\"",
    ])?;

    println!("Auth scaffolded. Endpoints:");
    println!("  GET  /signup   POST /signup");
    println!("  GET  /login    POST /login    POST /logout");
    Ok(())
}

fn user_already_has_password_digest() -> Result<bool> {
    let dir = Path::new("db/migrate");
    if !dir.exists() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.extension().and_then(|s| s.to_str()).unwrap_or("").eq("stk") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        if content.contains("password_digest") && content.contains("\"users\"") {
            return Ok(true);
        }
    }
    Ok(false)
}

// ── Admin panel ────────────────────────────────────────────────────────

const ADMIN_CONTROLLER: &str =
    include_str!("../templates/admin/admin_controller.stk");
const ADMIN_INDEX: &str = include_str!("../templates/admin/index.html.erb");
const ADMIN_TABLE: &str = include_str!("../templates/admin/table.html.erb");
const ADMIN_CSS: &str = include_str!("../templates/admin/admin.css");

pub fn admin() -> Result<()> {
    ensure_app_root()?;

    // Discover models so the admin can list every table.
    let models = scan_models()?;
    let table_names: Vec<String> = models.iter().map(|n| plural_snake(n)).collect();
    let table_list_lit = table_names
        .iter()
        .map(|t| format!("\"{}\"", t))
        .collect::<Vec<_>>()
        .join(", ");

    let controller = ADMIN_CONTROLLER.replace("{{table_list}}", &table_list_lit);
    write_file(
        &PathBuf::from("app/controllers/admin_controller.stk"),
        &controller,
    )?;
    write_file(&PathBuf::from("app/views/admin/index.html.erb"), ADMIN_INDEX)?;
    write_file(&PathBuf::from("app/views/admin/table.html.erb"), ADMIN_TABLE)?;
    write_file(&PathBuf::from("public/assets/admin.css"), ADMIN_CSS)?;

    append_routes(&[
        "web_route \"GET /admin\", \"admin#index\"",
        "web_route \"GET /admin/:table\", \"admin#table\"",
    ])?;

    println!("Admin panel scaffolded:");
    println!("  GET /admin       — list of tables");
    println!("  GET /admin/<t>   — paginated rows for table <t>");
    println!("  Tables wired: {}", table_names.join(", "));
    Ok(())
}

// ── API controller (for an existing model) ─────────────────────────────

const API_CONTROLLER: &str = include_str!("../templates/api/controller.stk");

pub fn api(name: &str) -> Result<()> {
    ensure_app_root()?;
    let model_cn = name.to_pascal_case();
    let plural = plural_snake(name);
    let model_singular = file_stem(name);
    let controller_cn = format!("Api{}", plural.to_pascal_case());
    let controller_fs = format!("api_{}", plural);

    let body = API_CONTROLLER
        .replace("{{class_name}}", &controller_cn)
        .replace("{{file_stem}}", &controller_fs)
        .replace("{{plural}}", &plural)
        .replace("{{singular}}", &model_singular)
        .replace("{{model}}", &model_cn);

    write_file(
        &PathBuf::from(format!("app/controllers/{}_controller.stk", controller_fs)),
        &body,
    )?;

    let routes: Vec<String> = vec![
        format!(
            "web_route \"GET    /api/{p}\", \"api_{p}#index\"",
            p = plural
        ),
        format!(
            "web_route \"POST   /api/{p}\", \"api_{p}#create\"",
            p = plural
        ),
        format!(
            "web_route \"GET    /api/{p}/:id\", \"api_{p}#show\"",
            p = plural
        ),
        format!(
            "web_route \"PATCH  /api/{p}/:id\", \"api_{p}#update\"",
            p = plural
        ),
        format!(
            "web_route \"PUT    /api/{p}/:id\", \"api_{p}#update\"",
            p = plural
        ),
        format!(
            "web_route \"DELETE /api/{p}/:id\", \"api_{p}#destroy\"",
            p = plural
        ),
    ];
    let route_strs: Vec<&str> = routes.iter().map(|s| s.as_str()).collect();
    append_routes(&route_strs)?;
    println!("API controller scaffolded for {} at /api/{}", model_cn, plural);
    Ok(())
}

// ── Mailer / Job / Channel ─────────────────────────────────────────────

const MAILER_TMPL: &str = include_str!("../templates/extras/mailer.stk");
const JOB_TMPL: &str = include_str!("../templates/extras/job.stk");
const CHANNEL_TMPL: &str = include_str!("../templates/extras/channel.stk");

pub fn mailer(name: &str, actions: &[String]) -> Result<()> {
    ensure_app_root()?;
    let cn = format!("{}Mailer", name.to_pascal_case());
    let fs = file_stem(name);
    let actions: Vec<String> = if actions.is_empty() {
        vec!["welcome".into()]
    } else {
        actions.to_vec()
    };
    let mut actions_block = String::new();
    for a in &actions {
        actions_block.push_str(&format!(
            "    fn {a} {{\n        # TODO: deliver `{a}` mail. Use `web_send_mail`\n        # when the SMTP runtime ships in PASS 9.\n    }}\n\n",
            a = a
        ));
    }
    let body = MAILER_TMPL
        .replace("{{class_name}}", &cn)
        .replace("{{file_stem}}", &fs)
        .replace("{{actions_block}}", actions_block.trim_end_matches('\n'));
    write_file(
        &PathBuf::from(format!("app/mailers/{}_mailer.stk", fs)),
        &body,
    )?;
    println!("Mailer scaffolded: {}Mailer with {} action(s)", name.to_pascal_case(), actions.len());
    Ok(())
}

pub fn job(name: &str) -> Result<()> {
    ensure_app_root()?;
    let cn = format!("{}Job", name.to_pascal_case());
    let fs = file_stem(name);
    let body = JOB_TMPL
        .replace("{{class_name}}", &cn)
        .replace("{{file_stem}}", &fs);
    write_file(&PathBuf::from(format!("app/jobs/{}_job.stk", fs)), &body)?;
    println!("Job scaffolded: {}Job", name.to_pascal_case());
    Ok(())
}

pub fn channel(name: &str) -> Result<()> {
    ensure_app_root()?;
    let cn = format!("{}Channel", name.to_pascal_case());
    let fs = file_stem(name);
    let body = CHANNEL_TMPL
        .replace("{{class_name}}", &cn)
        .replace("{{file_stem}}", &fs);
    write_file(
        &PathBuf::from(format!("app/channels/{}_channel.stk", fs)),
        &body,
    )?;
    println!("Channel scaffolded: {}Channel", name.to_pascal_case());
    Ok(())
}

// ── API mode conversion ────────────────────────────────────────────────

pub fn convert_to_api() -> Result<()> {
    // Drop the layout — API responses don't need HTML chrome.
    let layout = PathBuf::from("app/views/layouts/application.html.erb");
    if layout.exists() {
        let _ = std::fs::remove_file(&layout);
    }
    // Mark application.stk so the user sees this is API mode.
    let app_path = PathBuf::from("config/application.stk");
    let mut content = std::fs::read_to_string(&app_path)
        .map_err(|e| format!("read application.stk: {}", e))?;
    if !content.contains("# api-only mode") {
        content.insert_str(0, "# api-only mode — `s_web new --api` skips views/helpers/layout.\n");
        std::fs::write(&app_path, content)?;
    }
    println!("API-only mode applied (no views, no layout, controllers emit JSON).");
    Ok(())
}

// ── Internal utilities ─────────────────────────────────────────────────

fn ensure_app_root() -> Result<()> {
    if !Path::new("config/application.stk").exists() {
        return Err(
            "config/application.stk not found — run from an app directory.".into(),
        );
    }
    Ok(())
}

fn scan_models() -> Result<Vec<String>> {
    let mut out = Vec::new();
    let dir = Path::new("app/models");
    if !dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("stk") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem == "application_record" || stem.is_empty() {
            continue;
        }
        out.push(stem.to_pascal_case());
    }
    out.sort();
    Ok(out)
}

fn append_routes(lines: &[&str]) -> Result<()> {
    let path = Path::new("config/routes.stk");
    let mut content = std::fs::read_to_string(path)
        .map_err(|e| format!("read routes.stk: {}", e))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }
    let mut added = 0usize;
    for line in lines {
        if content.contains(line.trim()) {
            continue;
        }
        content.push_str(line);
        content.push('\n');
        added += 1;
    }
    if added > 0 {
        std::fs::write(path, content)?;
    }
    Ok(())
}
