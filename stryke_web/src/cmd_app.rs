//! `s_web g app PRESET_OR_RESOURCES` — the mega-scaffold generator.
//!
//! Where `s_web g scaffold` writes one resource, `s_web g app` writes
//! an entire domain in one shot:
//!
//!   s_web g app blog            # 8 resources from the blog preset
//!   s_web g app saas            # 12 resources for an org/billing app
//!   s_web g app everything      # ~80 resources, every preset combined
//!   s_web g app User Post:title:string,body:text Comment:body:text
//!                               # bulk inline mode — each token is
//!                               # NAME[:field:type[,field:type...]]
//!
//! After scaffolding every resource it auto-edits `config/routes.stk`
//! to insert a `web_resources "..."` line per resource, and prints the
//! "now run db migrate + bin/server" hint exactly once.

use crate::cmd_generate;
use crate::presets;
use crate::util::{plural_snake, Result};
use std::path::Path;
use std::time::Instant;

/// `args` is whatever the user typed after `s_web g app`. First token
/// is treated as a preset name if it matches the registry; otherwise
/// every token is parsed as an inline resource spec.
pub fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Err(usage().into());
    }

    if !Path::new("config/application.stk").exists() {
        return Err("config/application.stk not found — run from an app directory.".into());
    }

    let resources = if args.len() == 1 {
        if args[0] == "everything" {
            let all = presets::everything_resources();
            println!(
                "Mega-scaffold mode: `everything` — {} resources from {} presets",
                all.len(),
                presets::PRESETS.len()
            );
            all.iter()
                .map(|r| (r.name.to_string(), spec_field_list(r.fields)))
                .collect()
        } else if let Some(preset) = presets::lookup(&args[0]) {
            println!(
                "Preset `{}` ({}): {} resources",
                preset.name,
                preset.description,
                preset.resources.len()
            );
            preset
                .resources
                .iter()
                .map(|r| (r.name.to_string(), spec_field_list(r.fields)))
                .collect()
        } else {
            // Single inline spec.
            vec![parse_inline_spec(&args[0])?]
        }
    } else {
        // Multiple inline specs.
        args.iter()
            .map(|s| parse_inline_spec(s))
            .collect::<Result<Vec<_>>>()?
    };

    if resources.is_empty() {
        return Err("nothing to scaffold".into());
    }

    let started = Instant::now();
    let total = resources.len();
    let mut plurals: Vec<String> = Vec::with_capacity(total);
    for (i, (name, fields)) in resources.iter().enumerate() {
        println!(
            "\n[{}/{}] scaffolding {}{}",
            i + 1,
            total,
            name,
            if fields.is_empty() {
                String::new()
            } else {
                format!(" ({} fields)", fields.len())
            }
        );
        cmd_generate::scaffold(name, fields)?;
        plurals.push(plural_snake(name));
    }

    auto_wire_routes(&plurals)?;

    let elapsed = started.elapsed();
    println!();
    println!(
        "Done — {} resources scaffolded in {:.2}s.",
        total,
        elapsed.as_secs_f32()
    );
    println!();
    println!("Next:");
    println!("  s_web db migrate");
    println!("  bin/server");
    println!();
    println!("Routes wired into config/routes.stk:");
    for plural in &plurals {
        println!("  web_resources \"{}\"", plural);
    }
    Ok(())
}

fn usage() -> &'static str {
    "usage: s_web g app PRESET | RESOURCE_SPEC...\n\
     \n\
     Presets:\n\
       blog ecommerce saas social cms forum crm helpdesk everything\n\
     \n\
     Inline specs (one per resource):\n\
       Name                            (no fields)\n\
       Name:field:type                 (one field)\n\
       Name:field:type,field:type      (multiple fields)"
}

/// Convert a Resource's `&[&str]` field list into the `Vec<String>`
/// shape `cmd_generate::scaffold` wants.
fn spec_field_list(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|s| s.to_string()).collect()
}

/// Parse `Name:field:type,field:type` — colon separates name from
/// fields, comma separates fields. The bare form `Name` is also valid
/// and produces zero fields.
fn parse_inline_spec(spec: &str) -> Result<(String, Vec<String>)> {
    let (name, fields_part) = match spec.split_once(':') {
        Some((n, f)) => (n.to_string(), f),
        None => return Ok((spec.to_string(), Vec::new())),
    };
    let fields = fields_part
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    Ok((name, fields))
}

/// Append `web_resources "<plural>"` to `config/routes.stk` for every
/// new resource that isn't already wired. Scans non-comment lines so
/// the same string occurring inside a `# ...` example doesn't fool us
/// into thinking the resource is registered. Idempotent: re-running
/// `s_web g app blog` adds nothing the second time.
fn auto_wire_routes(plurals: &[String]) -> Result<()> {
    let path = Path::new("config/routes.stk");
    let mut content =
        std::fs::read_to_string(path).map_err(|e| format!("read routes.stk: {}", e))?;
    if !content.ends_with('\n') {
        content.push('\n');
    }

    let live: std::collections::HashSet<String> = content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                return None;
            }
            // Match `web_resources "name"` even if there's trailing
            // whitespace, args, or comments after.
            let key = trimmed.strip_prefix("web_resources")?.trim_start();
            let key = key.strip_prefix('"')?;
            let end = key.find('"')?;
            Some(key[..end].to_string())
        })
        .collect();

    let mut added = 0usize;
    for plural in plurals {
        if live.contains(plural) {
            continue;
        }
        content.push_str(&format!("web_resources \"{}\"\n", plural));
        added += 1;
    }
    if added > 0 {
        std::fs::write(path, content).map_err(|e| format!("write routes.stk: {}", e))?;
    }
    Ok(())
}
