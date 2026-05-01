//! Templates baked into the binary at compile time. Each function returns
//! the rendered text for a generated file. The templates use `{{ }}`
//! placeholder substitution because stryke's own `#{}` interpolation would
//! be ambiguous when the OUTPUT is stryke source — we don't want the
//! template's own `#{}` to expand at render time.
//!
//! Substitution is a single-pass straight-string replace on each
//! placeholder name supplied by the caller. No conditionals, no loops in
//! the templating layer — anything dynamic is done by the caller in Rust
//! before passing the substitution map.

use std::collections::BTreeMap;

/// Replace every `{{key}}` occurrence in `template` with `vars[key]`.
/// Missing keys are left as `{{key}}` so callers can spot un-substituted
/// placeholders during development. Whitespace inside the braces is
/// tolerated: `{{ key }}` and `{{key}}` both substitute.
pub fn render(template: &str, vars: &BTreeMap<&str, &str>) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end) = find_close(template, i + 2) {
                let key = template[i + 2..end].trim();
                if let Some(v) = vars.get(key) {
                    out.push_str(v);
                    i = end + 2;
                    continue;
                }
            }
        }
        // Walk one UTF-8 codepoint at a time. Single-byte indexing here
        // would split multi-byte sequences and double-encode them when
        // pushed via `as char` — bug seen in m-dashes corrupting to `â`.
        let ch_len = utf8_char_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        out.push_str(&template[i..end]);
        i = end;
    }
    out
}

#[inline]
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte — skip defensively; real lead found earlier
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

fn find_close(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = start;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `config/routes.stk` — empty starter route table for `s_web new`. Rails'
/// equivalent is `config/routes.rb` with a `Rails.application.routes.draw`
/// block; ours uses the bare `route ...` DSL.
pub const ROUTES_STK: &str = include_str!("../templates/app/config/routes.stk.tmpl");

/// `config/application.stk` — wires the framework, sets app-wide config
/// (default locale, time zone, autoload paths). Loaded at boot by `s_web s`.
pub const APPLICATION_STK: &str = include_str!("../templates/app/config/application.stk.tmpl");

/// `config/database.toml` — DB connection per environment.
pub const DATABASE_TOML: &str = include_str!("../templates/app/config/database.toml.tmpl");

/// `app/controllers/application_controller.stk` — base controller every
/// generated controller extends. Holds shared `before_action` filters and
/// helpers.
pub const APPLICATION_CONTROLLER_STK: &str =
    include_str!("../templates/app/controllers/application_controller.stk.tmpl");

/// `app/models/application_record.stk` — base model. Sets up shared `class`
/// inheritance for every generated model.
pub const APPLICATION_RECORD_STK: &str =
    include_str!("../templates/app/models/application_record.stk.tmpl");

/// `app/views/layouts/application.html.erb` — top-level page chrome that
/// `<%= yield %>`s the action's view.
pub const APPLICATION_LAYOUT_ERB: &str =
    include_str!("../templates/app/views/layouts/application.html.erb.tmpl");

/// `app/helpers/application_helper.stk` — view helpers shared across the app.
pub const APPLICATION_HELPER_STK: &str =
    include_str!("../templates/app/helpers/application_helper.stk.tmpl");

/// `bin/server` — single-line bootstrap that starts the dev server.
pub const BIN_SERVER: &str = include_str!("../templates/app/bin/server.tmpl");

/// `db/seeds.stk` — placeholder seeds file.
pub const DB_SEEDS_STK: &str = include_str!("../templates/app/db/seeds.stk.tmpl");

/// `Gemfile-equivalent` — `stryke.toml` declares stryke version + framework.
pub const STRYKE_TOML: &str = include_str!("../templates/app/stryke.toml.tmpl");

/// `README.md` for the generated app.
pub const APP_README: &str = include_str!("../templates/app/README.md.tmpl");

/// `.gitignore` — log/, tmp/, db/*.sqlite3, etc.
pub const GITIGNORE: &str = include_str!("../templates/app/gitignore.tmpl");

// ── Per-resource templates (controller / model / view / migration) ──

/// Controller class file template.
pub const CONTROLLER_STK: &str = include_str!("../templates/controller.stk.tmpl");

/// Model class file template.
pub const MODEL_STK: &str = include_str!("../templates/model.stk.tmpl");

/// Migration file template.
pub const MIGRATION_STK: &str = include_str!("../templates/migration.stk.tmpl");

/// Per-action view template (one per action: index/show/new/edit by default).
pub const VIEW_INDEX_ERB: &str = include_str!("../templates/views/index.html.erb.tmpl");
pub const VIEW_SHOW_ERB: &str = include_str!("../templates/views/show.html.erb.tmpl");
pub const VIEW_NEW_ERB: &str = include_str!("../templates/views/new.html.erb.tmpl");
pub const VIEW_EDIT_ERB: &str = include_str!("../templates/views/edit.html.erb.tmpl");
pub const VIEW_FORM_ERB: &str = include_str!("../templates/views/_form.html.erb.tmpl");
