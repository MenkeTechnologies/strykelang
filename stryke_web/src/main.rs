//! `s_web` — CLI entry point. Dispatches to subcommands; each subcommand
//! lives in its own `cmd_*` module so the dispatcher stays readable as the
//! generator surface grows toward Rails parity.
//!
//! Subcommands (current + planned):
//!   s_web new APP                 — scaffold a new app at ./APP/
//!   s_web g controller NAME ACT.. — generate a controller + actions + views
//!   s_web g model NAME field:type.. — generate a model + migration
//!   s_web g migration NAME field:type.. — generate a standalone migration
//!   s_web g scaffold NAME field:type.. — model+controller+views+migration
//!   s_web s [PORT]                — run the dev server (delegates to stryke)
//!   s_web routes                  — list registered routes
//!   s_web db migrate              — apply pending migrations
//!   s_web db rollback             — roll back the last migration
//!   s_web db seed                 — load db/seeds.stk
//!   s_web db reset                — drop, create, migrate, seed
//!   s_web console                 — REPL with the app loaded

use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "s_web", version, about = "Stryke web — Rails-shaped framework CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scaffold a new stryke web app at ./APP/
    New {
        /// Name of the application directory to create.
        name: String,
        /// Skip the initial `git init`.
        #[arg(long)]
        skip_git: bool,
        /// Database adapter to wire up (`sqlite` only for v0).
        #[arg(long, default_value = "sqlite")]
        database: String,
        /// Bake an entire app into the new directory in one shot.
        #[arg(long)]
        app: Option<String>,
        /// After scaffolding, run `s_web db migrate` automatically.
        #[arg(long)]
        migrate: bool,
        /// API-only mode — skip view templates, default controllers
        /// emit JSON via `web_json`, layout dropped. Equivalent to
        /// `rails new --api`.
        #[arg(long)]
        api: bool,
        /// Wire User auth (User model + sessions + signup/login/logout
        /// pages). Same as running `s_web g auth` after scaffold.
        #[arg(long)]
        auth: bool,
        /// Add an admin panel at `/admin` for every generated model.
        /// Same as running `s_web g admin` after scaffold.
        #[arg(long)]
        admin: bool,
        /// CSS theme preset baked into `app/views/layouts/application.html.erb`
        /// and `public/assets/application.css`. One of: `pico`,
        /// `bootstrap`, `tailwind`, `simple`, `dark`. Default: `simple`.
        #[arg(long, default_value = "simple")]
        theme: String,
    },

    /// Generate scaffolding inside an existing app (alias: `g`)
    #[command(alias = "g")]
    Generate {
        #[command(subcommand)]
        what: GenerateCmd,
    },

    /// Run the dev server (alias: `s`)
    #[command(alias = "s")]
    Server {
        /// Port to bind (defaults to 3000, matching Rails).
        #[arg(short, long, default_value = "3000")]
        port: u16,
    },

    /// List the route table (compiled from config/routes.stk)
    Routes,

    /// Database tasks
    Db {
        #[command(subcommand)]
        task: DbCmd,
    },

    /// Open a REPL with the app loaded
    Console,
}

#[derive(Subcommand)]
enum GenerateCmd {
    /// Generate a controller with the given action methods
    Controller {
        name: String,
        #[arg(num_args = 0..)]
        actions: Vec<String>,
    },
    /// Generate a model + migration from `field:type` specs
    Model {
        name: String,
        #[arg(num_args = 0..)]
        fields: Vec<String>,
    },
    /// Generate a standalone migration
    Migration {
        name: String,
        #[arg(num_args = 0..)]
        fields: Vec<String>,
    },
    /// Full Rails scaffold: model + migration + controller + views
    Scaffold {
        name: String,
        #[arg(num_args = 0..)]
        fields: Vec<String>,
    },
    /// Mega scaffold: a whole app from a preset (blog, ecommerce, saas,
    /// social, cms, forum, crm, helpdesk, everything) or a list of
    /// `Name:field:type,...` specs. Auto-wires `web_resources` lines
    /// into config/routes.stk.
    App {
        #[arg(num_args = 1..)]
        spec: Vec<String>,
    },
    /// User auth scaffold — User model, signup/login/logout pages,
    /// session-backed sign-in. One command, full auth flow.
    Auth,
    /// Admin panel — auto-generated CRUD UI for every model under
    /// `app/models/`. Mounted at `/admin`.
    Admin,
    /// JSON API controller for an existing model. Each action returns
    /// `web_json` instead of HTML.
    Api {
        name: String,
    },
    /// Mailer scaffold — `app/mailers/NAME_mailer.stk` with action stubs.
    Mailer {
        name: String,
        #[arg(num_args = 0..)]
        actions: Vec<String>,
    },
    /// Background job scaffold — `app/jobs/NAME_job.stk`.
    Job {
        name: String,
    },
    /// WebSocket / SSE channel scaffold — `app/channels/NAME_channel.stk`.
    Channel {
        name: String,
    },
}

#[derive(Subcommand)]
enum DbCmd {
    /// Apply all pending migrations from db/migrate/
    Migrate,
    /// Roll back the most recent migration
    Rollback,
    /// Load db/seeds.stk
    Seed,
    /// Drop, create, migrate, seed (destructive)
    Reset,
}

/// One-liner mode. Lay out the new tree, optionally apply theme +
/// auth + admin + api, optionally bulk-scaffold a preset, optionally
/// run `db migrate`. Each step is a no-op unless its flag is set.
#[allow(clippy::too_many_arguments)]
fn one_shot_new(
    name: &str,
    skip_git: bool,
    database: &str,
    app: Option<&str>,
    migrate: bool,
    api: bool,
    auth: bool,
    admin: bool,
    theme: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    stryke_web::cmd_new::run(name, skip_git, database)?;

    let prev = std::env::current_dir()?;
    std::env::set_current_dir(name)?;
    let result: Result<(), Box<dyn std::error::Error>> = (|| {
        // Theme is applied first so subsequent generated views can
        // reference any classes / partials the theme installed.
        if theme != "simple" || api {
            stryke_web::cmd_extras::apply_theme(theme, api)?;
        }
        if api {
            stryke_web::cmd_extras::convert_to_api()?;
        }
        if let Some(spec) = app {
            let words: Vec<String> =
                spec.split_whitespace().map(|s| s.to_string()).collect();
            stryke_web::cmd_app::run(&words)?;
        }
        if auth {
            stryke_web::cmd_extras::auth()?;
        }
        if admin {
            stryke_web::cmd_extras::admin()?;
        }
        if migrate {
            println!();
            println!("Running migrations…");
            stryke_web::cmd_db::migrate()?;
        }
        Ok(())
    })();
    let _ = std::env::set_current_dir(&prev);
    result?;

    if app.is_some() || migrate || auth || admin || api {
        println!();
        println!("Done. Boot the app:");
        println!("  cd {}", name);
        println!("  bin/server");
    }
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::New {
            name,
            skip_git,
            database,
            app,
            migrate,
            api,
            auth,
            admin,
            theme,
        } => one_shot_new(
            &name,
            skip_git,
            &database,
            app.as_deref(),
            migrate,
            api,
            auth,
            admin,
            &theme,
        ),
        Cmd::Generate { what } => match what {
            GenerateCmd::Controller { name, actions } => {
                stryke_web::cmd_generate::controller(&name, &actions)
            }
            GenerateCmd::Model { name, fields } => {
                stryke_web::cmd_generate::model(&name, &fields)
            }
            GenerateCmd::Migration { name, fields } => {
                stryke_web::cmd_generate::migration(&name, &fields)
            }
            GenerateCmd::Scaffold { name, fields } => {
                stryke_web::cmd_generate::scaffold(&name, &fields)
            }
            GenerateCmd::App { spec } => stryke_web::cmd_app::run(&spec),
            GenerateCmd::Auth => stryke_web::cmd_extras::auth(),
            GenerateCmd::Admin => stryke_web::cmd_extras::admin(),
            GenerateCmd::Api { name } => stryke_web::cmd_extras::api(&name),
            GenerateCmd::Mailer { name, actions } => {
                stryke_web::cmd_extras::mailer(&name, &actions)
            }
            GenerateCmd::Job { name } => stryke_web::cmd_extras::job(&name),
            GenerateCmd::Channel { name } => stryke_web::cmd_extras::channel(&name),
        },
        Cmd::Server { port } => stryke_web::cmd_server::run(port),
        Cmd::Routes => stryke_web::cmd_routes::run(),
        Cmd::Db { task } => match task {
            DbCmd::Migrate => stryke_web::cmd_db::migrate(),
            DbCmd::Rollback => stryke_web::cmd_db::rollback(),
            DbCmd::Seed => stryke_web::cmd_db::seed(),
            DbCmd::Reset => stryke_web::cmd_db::reset(),
        },
        Cmd::Console => stryke_web::cmd_server::console(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("s_web: {}", e);
            ExitCode::FAILURE
        }
    }
}
