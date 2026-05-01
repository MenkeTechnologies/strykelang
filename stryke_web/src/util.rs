//! Shared helpers — file writing with friendly create/skip output, name-case
//! conversions tuned for the Rails-shaped naming convention (singular vs
//! plural, snake_case vs PascalCase), and migration timestamping.

use heck::{ToPascalCase, ToSnakeCase};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Result alias used across the generator commands. `Box<dyn Error>` keeps
/// each subcommand free to surface filesystem / parse / template errors
/// without an enum bottleneck — the binary just prints them.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Write `contents` to `path`. Creates parent directories. Skips when the
/// destination already exists with the same content; errors when content
/// differs (Rails' safe default).
pub fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        let existing = fs::read_to_string(path).unwrap_or_default();
        if existing == contents {
            println!("    identical  {}", path.display());
            return Ok(());
        }
        return Err(format!(
            "{}: file exists with different content (refusing to overwrite)",
            path.display()
        )
        .into());
    }
    let mut f = fs::File::create(path)?;
    f.write_all(contents.as_bytes())?;
    println!("      create  {}", path.display());
    Ok(())
}

/// Overwrite-allowed variant for theme/asset files that legitimately
/// replace whatever was scaffolded by `s_web new`.
pub fn force_write(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(path)?;
    f.write_all(contents.as_bytes())?;
    println!("       force  {}", path.display());
    Ok(())
}

/// Print `mkdir` line + create the directory (idempotent).
pub fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(path)?;
    println!("      create  {}/", path.display());
    Ok(())
}

/// Convert a model-style name to its database table name. Rails does
/// `Post → posts`, `Person → people`, `Mouse → mice`. We start with the
/// trivial `+s` rule and special-case a few common irregulars; the full
/// inflector lives in `inflections.rs` (TODO).
pub fn pluralize(name: &str) -> String {
    let snake = name.to_snake_case();
    match snake.as_str() {
        "person" => "people".into(),
        "child" => "children".into(),
        "mouse" => "mice".into(),
        "goose" => "geese".into(),
        s if s.ends_with("ss") => format!("{}es", s),
        s if s.ends_with("ch") || s.ends_with("sh") || s.ends_with('x') || s.ends_with('z') => {
            format!("{}es", s)
        }
        s if s.ends_with('y') && !ends_with_vowel_y(s) => {
            format!("{}ies", &s[..s.len() - 1])
        }
        s => format!("{}s", s),
    }
}

/// `categories → category`, mirroring `pluralize` for ORM lookups.
pub fn singularize(name: &str) -> String {
    let snake = name.to_snake_case();
    match snake.as_str() {
        "people" => "person".into(),
        "children" => "child".into(),
        "mice" => "mouse".into(),
        "geese" => "goose".into(),
        s if s.ends_with("ies") => format!("{}y", &s[..s.len() - 3]),
        s if s.ends_with("sses") || s.ends_with("ches") || s.ends_with("shes") => {
            s[..s.len() - 2].to_string()
        }
        s if s.ends_with('s') && !s.ends_with("ss") => s[..s.len() - 1].to_string(),
        s => s.to_string(),
    }
}

fn ends_with_vowel_y(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes[bytes.len() - 1] != b'y' {
        return false;
    }
    matches!(bytes[bytes.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Generate a Rails-shaped migration timestamp: `20260430153012`
/// (UTC, year-month-day-hour-minute-second). Uses the system clock; the
/// granularity (1 sec) matches Rails' format exactly, so files sort
/// chronologically in `db/migrate/`.
pub fn migration_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Convert epoch secs → "YYYYMMDDHHMMSS" (UTC).
    let s = secs as i64;
    let days = s / 86_400;
    let secs_today = s % 86_400;
    let h = secs_today / 3600;
    let m = (secs_today % 3600) / 60;
    let sec = secs_today % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}",
        y, mo, d, h, m, sec
    )
}

/// Convert days-since-1970 to (year, month, day) in the Gregorian calendar.
/// Compact algorithm — handles leap years correctly through year 9999, which
/// is the upper bound migration timestamps will ever realistically need.
fn days_to_ymd(mut days: i64) -> (i64, i64, i64) {
    let mut y = 1970;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if days < yd {
            break;
        }
        days -= yd;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut mo = 1;
    for d in dim {
        if days < d {
            break;
        }
        days -= d;
        mo += 1;
    }
    (y, mo, days + 1)
}

/// Parse a `field:type` spec into `(name, type)`. Accepts bare `field` (=> `string`).
/// Recognized types: `string`, `text`, `int`, `bigint`, `float`, `bool`,
/// `date`, `datetime`, `decimal`, `references`. Mirrors Rails' `db:migrate`
/// vocabulary so generators can scaffold a migration directly.
pub fn parse_field(spec: &str) -> (String, String) {
    let mut parts = spec.splitn(2, ':');
    let name = parts.next().unwrap_or("").to_snake_case();
    let ty = parts.next().unwrap_or("string").to_string();
    (name, ty)
}

/// Map our field-type vocabulary to SQLite column types.
pub fn sql_type_for(ty: &str) -> &'static str {
    match ty {
        "string" | "text" => "TEXT",
        "int" | "integer" | "bigint" | "references" => "INTEGER",
        "float" | "decimal" => "REAL",
        "bool" | "boolean" => "INTEGER",
        "date" | "datetime" | "timestamp" => "TEXT",
        "blob" | "binary" => "BLOB",
        _ => "TEXT",
    }
}

/// Convert a model class name to the Pascal-cased identifier used in stryke
/// code (`post` / `Post` / `posts` all → `Post`).
pub fn class_name(name: &str) -> String {
    singularize(name).to_pascal_case()
}

/// Snake-case singular for filenames (`Post` → `post`).
pub fn file_stem(name: &str) -> String {
    singularize(name).to_snake_case()
}

/// Snake-case plural for table names and route prefixes.
pub fn plural_snake(name: &str) -> String {
    pluralize(name).to_snake_case()
}
