//! `s_web g {controller,model,migration,scaffold}` — write resource files
//! into an existing app. The renderers below take user-supplied names and
//! field specs, build the substitution map, and emit `.stk` files via
//! `templates::render`. The aim is byte-for-byte parity with Rails'
//! `rails generate` flow — same files, same naming conventions, same
//! per-action view scaffolding — adapted to stryke syntax.

use crate::templates::{self, render};
use crate::util::{
    class_name, file_stem, migration_timestamp, parse_field, plural_snake, sql_type_for,
    write_file, Result,
};
use heck::{ToPascalCase, ToTitleCase};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Controller naming follows Rails: model is singular (`Post`), controller is
/// plural (`PostsController` in `posts_controller.stk`). `name` may come in
/// either form — singular `Post`, plural `Posts`, snake `posts`, etc. —
/// and we re-pluralize / Pascal-case the controller artifacts unambiguously.
/// Returns just the bare plural Pascal-cased name (`Posts`); the controller
/// template appends the `Controller` suffix so the bare form is reusable in
/// other places that need the resource name (route helpers, comments).
fn controller_class(name: &str) -> String {
    plural_snake(name).to_pascal_case()
}

fn controller_file_stem(name: &str) -> String {
    plural_snake(name)
}
/// `controller` — see implementation.
pub fn controller(name: &str, actions: &[String]) -> Result<()> {
    let cn = controller_class(name);
    let fs = controller_file_stem(name);
    let plural = plural_snake(name);

    let actions: Vec<String> = if actions.is_empty() {
        vec!["index".into()]
    } else {
        actions.to_vec()
    };

    // Build the action stubs for the controller body. Each stub renders a
    // placeholder HTML page so the generated app responds to its routes
    // out of the box — replace the `web_render` line with template lookup
    // once the ERB pass lands.
    let mut actions_block = String::new();
    for a in &actions {
        actions_block.push_str(&format!(
            "    fn {a} {{\n        web_render(html => \"<h1>{cn}#{a}</h1><p>Find me in app/controllers/{fs}_controller.stk</p>\")\n    }}\n\n",
            a = a,
            cn = cn,
            fs = fs
        ));
    }

    let mut vars = BTreeMap::new();
    let actions_block = actions_block.trim_end_matches('\n').to_string();
    vars.insert("class_name", cn.as_str());
    vars.insert("file_stem", fs.as_str());
    vars.insert("actions_block", actions_block.as_str());

    let path = PathBuf::from(format!("app/controllers/{}_controller.stk", fs));
    write_file(&path, &render(templates::CONTROLLER_STK, &vars))?;

    // One placeholder view per action so the routes resolve out of the box.
    for a in &actions {
        let view_path = PathBuf::from(format!("app/views/{}/{}.html.erb", plural, a));
        let title = a.to_title_case();
        let body = format!(
            "<h1>{}#{}</h1>\n<p>Find me in app/views/{}/{}.html.erb</p>\n",
            cn, a, plural, a
        );
        let _ = title;
        write_file(&view_path, &body)?;
    }

    println!("\nAdd routes to config/routes.stk:");
    for a in &actions {
        println!(
            "  web_route \"GET /{}/{}\", \"{}#{}\"",
            plural, a, plural, a
        );
    }
    Ok(())
}
/// `model` — see implementation.
pub fn model(name: &str, fields: &[String]) -> Result<()> {
    write_model_file(name, fields)?;
    // Rails: migration class for `Post` model is `CreatePosts` (plural).
    write_migration_file(
        &format!("Create{}", plural_snake(name).to_pascal_case()),
        name,
        fields,
        true,
    )?;
    Ok(())
}
/// `migration` — see implementation.
pub fn migration(name: &str, fields: &[String]) -> Result<()> {
    // Standalone migration: name is e.g. "AddPublishedToPosts" — no model,
    // no scaffold. Body is empty up/down for the user to fill in unless
    // fields were given (in which case we add column statements).
    write_migration_file(name, name, fields, false)?;
    Ok(())
}
/// `scaffold` — see implementation.
pub fn scaffold(name: &str, fields: &[String]) -> Result<()> {
    // Full Rails scaffold = model (singular) + migration (plural class
    // name) + controller (plural class name) + views (plural directory).
    let model_cn = class_name(name);
    let model_fs = file_stem(name);
    let plural = plural_snake(name);
    let controller_cn = controller_class(name);
    let controller_fs = controller_file_stem(name);

    write_model_file(name, fields)?;
    write_migration_file(
        &format!("Create{}", plural.to_pascal_case()),
        name,
        fields,
        true,
    )?;

    // Controller with the seven REST actions, wired to the model class's
    // static methods (which themselves delegate to `web_model_*`).
    let plural_for_actions = plural_snake(name);
    let singular_for_actions = file_stem(name);
    let actions_block = format!(
        r#"    fn index {{
        my ${plural} = {model}::all()
        web_render(template => "{plural}/index", locals => +{{ {plural} => ${plural} }})
    }}

    fn show {{
        my ${singular} = {model}::find(web_params()->{{id}})
        web_render(template => "{plural}/show", locals => +{{ {singular} => ${singular} }})
    }}

    fn new {{
        web_render(template => "{plural}/new", locals => +{{ {singular} => +{{}} }})
    }}

    fn create {{
        {model}::create(web_params())
        web_redirect("/{plural}")
    }}

    fn edit {{
        my ${singular} = {model}::find(web_params()->{{id}})
        web_render(template => "{plural}/edit", locals => +{{ {singular} => ${singular} }})
    }}

    fn update {{
        my $id = web_params()->{{id}}
        {model}::update($id, web_params())
        web_redirect("/{plural}/$id")
    }}

    fn destroy {{
        {model}::destroy(web_params()->{{id}})
        web_redirect("/{plural}")
    }}
"#,
        model = model_cn,
        plural = plural_for_actions,
        singular = singular_for_actions,
    );
    let actions_block = actions_block.trim_end_matches('\n').to_string();
    let mut vars = BTreeMap::new();
    vars.insert("class_name", controller_cn.as_str());
    vars.insert("file_stem", controller_fs.as_str());
    vars.insert("actions_block", actions_block.as_str());
    write_file(
        &PathBuf::from(format!("app/controllers/{}_controller.stk", controller_fs)),
        &render(templates::CONTROLLER_STK, &vars),
    )?;

    // Views — index, show, new, edit, _form. Use the model (singular)
    // class name for titles and the model file stem for the local var.
    let parsed: Vec<(String, String)> = fields.iter().map(|s| parse_field(s)).collect();
    let plural_title = plural.to_title_case();
    let singular_title = model_cn.to_title_case();
    let singular_snake = model_fs.clone();

    let mut th_cells = String::new();
    let mut td_cells = String::new();
    let mut dl_pairs = String::new();
    let mut form_fields = String::new();
    for (fname, fty) in &parsed {
        th_cells.push_str(&format!("            <th>{}</th>\n", fname));
        // Auto-escape with `web_h` so HTML in fields can't break the
        // page or open XSS holes.
        td_cells.push_str(&format!(
            "                <td><%= web_h($r->{{{}}}) %></td>\n",
            fname
        ));
        dl_pairs.push_str(&format!(
            "    <dt>{}</dt><dd><%= web_h(${}->{{{}}}) %></dd>\n",
            fname, singular_snake, fname
        ));
        // Pick the form input type based on the migration field type.
        // text → textarea, bool → checkbox, anything else → text input.
        let input_helper = match fty.as_str() {
            "text" => format!(
                "<%= web_text_area(\"{fname}\", ${snake}->{{{fname}}} // \"\") %>",
                fname = fname,
                snake = singular_snake
            ),
            "bool" | "boolean" => format!(
                "<%= web_check_box(\"{fname}\", ${snake}->{{{fname}}}) %>",
                fname = fname,
                snake = singular_snake
            ),
            _ => format!(
                "<%= web_text_field(\"{fname}\", ${snake}->{{{fname}}} // \"\") %>",
                fname = fname,
                snake = singular_snake
            ),
        };
        form_fields.push_str(&format!(
            "    <div><label>{label}</label>{input}</div>\n",
            label = fname,
            input = input_helper,
        ));
    }
    let th_cells = th_cells.trim_end_matches('\n').to_string();
    let td_cells = td_cells.trim_end_matches('\n').to_string();
    let dl_pairs = dl_pairs.trim_end_matches('\n').to_string();
    let form_fields = form_fields.trim_end_matches('\n').to_string();

    let mut view_vars = BTreeMap::new();
    view_vars.insert("plural_title", plural_title.as_str());
    view_vars.insert("singular_title", singular_title.as_str());
    view_vars.insert("plural_snake", plural.as_str());
    view_vars.insert("singular_snake", singular_snake.as_str());
    view_vars.insert("th_cells", th_cells.as_str());
    view_vars.insert("td_cells", td_cells.as_str());
    view_vars.insert("dl_pairs", dl_pairs.as_str());
    view_vars.insert("form_fields", form_fields.as_str());

    let view_dir = format!("app/views/{}", plural);
    write_file(
        &PathBuf::from(format!("{}/index.html.erb", view_dir)),
        &render(templates::VIEW_INDEX_ERB, &view_vars),
    )?;
    write_file(
        &PathBuf::from(format!("{}/show.html.erb", view_dir)),
        &render(templates::VIEW_SHOW_ERB, &view_vars),
    )?;
    write_file(
        &PathBuf::from(format!("{}/new.html.erb", view_dir)),
        &render(templates::VIEW_NEW_ERB, &view_vars),
    )?;
    write_file(
        &PathBuf::from(format!("{}/edit.html.erb", view_dir)),
        &render(templates::VIEW_EDIT_ERB, &view_vars),
    )?;
    write_file(
        &PathBuf::from(format!("{}/_form.html.erb", view_dir)),
        &render(templates::VIEW_FORM_ERB, &view_vars),
    )?;

    println!(
        "\nAdd `web_resources \"{}\"` to config/routes.stk to wire up CRUD.",
        plural
    );
    Ok(())
}

fn write_model_file(name: &str, fields: &[String]) -> Result<()> {
    let cn = class_name(name);
    let fs = file_stem(name);
    let table = plural_snake(name);
    let parsed: Vec<(String, String)> = fields.iter().map(|s| parse_field(s)).collect();
    let mut fields_block = String::new();
    for (fname, fty) in &parsed {
        fields_block.push_str(&format!("    {}: {}\n", fname, stryke_type_for(fty)));
    }
    let fields_block = fields_block.trim_end_matches('\n').to_string();
    let mut vars = BTreeMap::new();
    vars.insert("class_name", cn.as_str());
    vars.insert("file_stem", fs.as_str());
    vars.insert("table_name", table.as_str());
    vars.insert("fields_block", fields_block.as_str());
    write_file(
        &PathBuf::from(format!("app/models/{}.stk", fs)),
        &render(templates::MODEL_STK, &vars),
    )
}

fn write_migration_file(
    class: &str,
    target: &str,
    fields: &[String],
    create_table: bool,
) -> Result<()> {
    let ts = migration_timestamp();
    let snake_class = heck::ToSnakeCase::to_snake_case(class);
    let filename = format!("db/migrate/{}_{}.stk", ts, snake_class);
    let table = plural_snake(target);

    let parsed: Vec<(String, String)> = fields.iter().map(|s| parse_field(s)).collect();
    let (up_block, down_block) = if create_table {
        // PASS 4 schema DSL: hashref of column → stryke type. `id`,
        // `created_at`, `updated_at` are added by web_create_table.
        let mut up = format!("        web_create_table(\"{}\", +{{\n", table);
        for (fname, fty) in &parsed {
            up.push_str(&format!("            {} => \"{}\",\n", fname, fty));
        }
        up.push_str("        })");
        let down = format!("        web_drop_table(\"{}\")", table);
        (up, down)
    } else if !fields.is_empty() {
        let mut up = String::new();
        for (fname, fty) in &parsed {
            up.push_str(&format!(
                "        web_add_column(\"{}\", \"{}\", \"{}\")\n",
                table,
                fname,
                sql_type_for(fty)
            ));
        }
        let mut down = String::new();
        for (fname, _) in &parsed {
            down.push_str(&format!(
                "        web_remove_column(\"{}\", \"{}\")\n",
                table, fname
            ));
        }
        (
            up.trim_end_matches('\n').to_string(),
            down.trim_end_matches('\n').to_string(),
        )
    } else {
        (
            "        # TODO: schema changes go here.".to_string(),
            "        # TODO: undo the schema changes from `up`.".to_string(),
        )
    };

    let mut vars = BTreeMap::new();
    vars.insert("class_name", class);
    vars.insert("filename", filename.as_str());
    vars.insert("up_block", up_block.as_str());
    vars.insert("down_block", down_block.as_str());
    write_file(
        &PathBuf::from(filename.clone()),
        &render(templates::MIGRATION_STK, &vars),
    )
}

/// Map a migration field type to the type annotation we use in the model
/// class declaration. Stryke's class syntax accepts `Str`, `Int`, `Float`,
/// `Bool` (matching the language's typed-field convention).
fn stryke_type_for(ty: &str) -> &'static str {
    match ty {
        "string" | "text" | "date" | "datetime" | "timestamp" => "Str",
        "int" | "integer" | "bigint" | "references" => "Int",
        "float" | "decimal" => "Float",
        "bool" | "boolean" => "Bool",
        _ => "Any",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Mutex;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    struct CwdGuard(std::path::PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    fn with_tmp_cwd<F, R>(f: F) -> R
    where
        F: FnOnce(&Path) -> R,
    {
        let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tmpdir");
        let prev = std::env::current_dir().expect("cwd");
        let _restore = CwdGuard(prev);
        std::env::set_current_dir(&dir).expect("cd");
        f(dir.path())
    }

    // ─── controller_class / controller_file_stem ────────────────────────

    #[test]
    fn controller_class_pluralizes_and_pascal_cases() {
        // Rails convention: model `Post` (singular) → controller class
        // `Posts`. Names are assumed to be the singular model form;
        // passing a plural in double-pluralizes (caller's fault).
        assert_eq!(controller_class("Post"), "Posts");
        assert_eq!(controller_class("post"), "Posts");
        assert_eq!(controller_class("BlogPost"), "BlogPosts");
        assert_eq!(controller_class("blog_post"), "BlogPosts");
        // Irregular plurals (pluralize special-cases).
        assert_eq!(controller_class("Person"), "People");
        assert_eq!(controller_class("mouse"), "Mice");
    }

    #[test]
    fn controller_file_stem_is_snake_plural() {
        // Files on disk go in snake_case_plural: posts_controller.stk.
        assert_eq!(controller_file_stem("Post"), "posts");
        assert_eq!(controller_file_stem("BlogPost"), "blog_posts");
        assert_eq!(controller_file_stem("Person"), "people");
    }

    // ─── stryke_type_for ────────────────────────────────────────────────

    #[test]
    fn stryke_type_for_string_family() {
        for ty in ["string", "text", "date", "datetime", "timestamp"] {
            assert_eq!(stryke_type_for(ty), "Str", "{ty} should map to Str");
        }
    }

    #[test]
    fn stryke_type_for_int_family_collapses_references() {
        for ty in ["int", "integer", "bigint", "references"] {
            assert_eq!(stryke_type_for(ty), "Int", "{ty} should map to Int");
        }
    }

    #[test]
    fn stryke_type_for_float_decimal_to_float() {
        assert_eq!(stryke_type_for("float"), "Float");
        assert_eq!(stryke_type_for("decimal"), "Float");
    }

    #[test]
    fn stryke_type_for_bool_aliases() {
        assert_eq!(stryke_type_for("bool"), "Bool");
        assert_eq!(stryke_type_for("boolean"), "Bool");
    }

    #[test]
    fn stryke_type_for_unknown_falls_back_to_any() {
        // Unknown types must not crash the generator — fall back to `Any`
        // so a typo in a `s_web g model` invocation still produces a
        // syntactically-valid model file.
        assert_eq!(stryke_type_for("frobnicate"), "Any");
        assert_eq!(stryke_type_for(""), "Any");
        assert_eq!(stryke_type_for("blob"), "Any");
    }

    // ─── stryke_type_for must agree with sql_type_for on every keyword ──

    #[test]
    fn stryke_type_for_and_sql_type_for_cover_same_int_family() {
        // Migration writes the SQL type; the model writes the stryke type.
        // The two functions are populated from the same `parse_field`
        // vocabulary, so the int-mapping families must align — otherwise
        // a column gets a wrong type-annotation in the generated model.
        for ty in ["int", "integer", "bigint", "references"] {
            assert_eq!(sql_type_for(ty), "INTEGER", "sql_type_for({ty})");
            assert_eq!(stryke_type_for(ty), "Int", "stryke_type_for({ty})");
        }
    }

    #[test]
    fn stryke_type_for_and_sql_type_for_cover_same_string_family() {
        for ty in ["string", "text"] {
            assert_eq!(sql_type_for(ty), "TEXT", "sql_type_for({ty})");
            assert_eq!(stryke_type_for(ty), "Str", "stryke_type_for({ty})");
        }
    }

    // ─── write_model_file / write_migration_file ────────────────────────

    #[test]
    fn write_model_file_creates_expected_path() {
        with_tmp_cwd(|root| {
            write_model_file(
                "Post",
                &["title:string".to_string(), "body:text".to_string()],
            )
            .expect("write model");
            let p = root.join("app/models/post.stk");
            assert!(p.exists(), "must write app/models/post.stk");
            let body = std::fs::read_to_string(&p).expect("read");
            // Stryke model class body must reference the model name.
            assert!(body.contains("Post"), "model body must reference Post");
        });
    }

    #[test]
    fn write_model_file_pluralizes_path_irregulars() {
        with_tmp_cwd(|root| {
            // Even for irregular plurals the file path uses the *singular*
            // snake form — `people` → file `person.stk` (model files are
            // always singular per Rails convention).
            write_model_file("Person", &[]).expect("write model");
            assert!(root.join("app/models/person.stk").exists());
            assert!(!root.join("app/models/people.stk").exists());
        });
    }

    #[test]
    fn write_migration_file_uses_timestamp_prefix_and_snake_table() {
        with_tmp_cwd(|root| {
            write_migration_file("CreatePosts", "posts", &[], true).expect("write migration");
            let entries: Vec<_> = std::fs::read_dir(root.join("db/migrate"))
                .expect("read migrate dir")
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            assert_eq!(
                entries.len(),
                1,
                "expected exactly one migration file, got {entries:?}"
            );
            let name = &entries[0];
            // Rails-style: 14-digit UTC timestamp prefix + _ + snake name + .stk.
            assert!(name.ends_with(".stk"), "missing .stk extension: {name}");
            assert!(name.contains("create_posts"), "missing snake name: {name}");
            let prefix: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
            assert_eq!(
                prefix.len(),
                14,
                "expected 14-digit timestamp prefix, got {prefix}"
            );
        });
    }

    #[test]
    fn migration_command_writes_file_with_fields_in_body() {
        with_tmp_cwd(|root| {
            migration("AddEmailToUsers", &["email:string".to_string()])
                .expect("write migration via public api");
            let entries: Vec<_> = std::fs::read_dir(root.join("db/migrate"))
                .expect("read")
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();
            assert_eq!(entries.len(), 1);
            let body = std::fs::read_to_string(&entries[0]).expect("read body");
            assert!(
                body.contains("email"),
                "migration body must reference declared field name"
            );
        });
    }

    // ─── model command ───────────────────────────────────────────────────

    #[test]
    fn model_command_writes_both_model_and_migration() {
        with_tmp_cwd(|root| {
            model(
                "Widget",
                &["sku:string".to_string(), "price:int".to_string()],
            )
            .expect("model");
            assert!(
                root.join("app/models/widget.stk").exists(),
                "model file must exist"
            );
            let migrations: Vec<_> = std::fs::read_dir(root.join("db/migrate"))
                .expect("read")
                .filter_map(|e| e.ok())
                .collect();
            assert_eq!(
                migrations.len(),
                1,
                "model command must also create the create_<plural> migration"
            );
        });
    }
}
