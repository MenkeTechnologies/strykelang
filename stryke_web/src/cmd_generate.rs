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

pub fn controller(name: &str, actions: &[String]) -> Result<()> {
    let cn = controller_class(name);
    let fs = controller_file_stem(name);
    let plural = plural_snake(name);

    let actions: Vec<String> = if actions.is_empty() {
        vec!["index".into()]
    } else {
        actions.iter().cloned().collect()
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

pub fn migration(name: &str, fields: &[String]) -> Result<()> {
    // Standalone migration: name is e.g. "AddPublishedToPosts" — no model,
    // no scaffold. Body is empty up/down for the user to fill in unless
    // fields were given (in which case we add column statements).
    write_migration_file(name, name, fields, false)?;
    Ok(())
}

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
                table, fname, sql_type_for(fty)
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
