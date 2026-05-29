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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pluralize_simple_s() {
        assert_eq!(pluralize("post"), "posts");
        assert_eq!(pluralize("book"), "books");
        assert_eq!(pluralize("tag"), "tags");
    }

    #[test]
    fn pluralize_irregulars() {
        assert_eq!(pluralize("person"), "people");
        assert_eq!(pluralize("child"), "children");
        assert_eq!(pluralize("mouse"), "mice");
        assert_eq!(pluralize("goose"), "geese");
    }

    #[test]
    fn pluralize_y_to_ies_consonant() {
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("policy"), "policies");
        assert_eq!(pluralize("city"), "cities");
    }

    #[test]
    fn pluralize_y_to_s_vowel() {
        assert_eq!(pluralize("day"), "days");
        assert_eq!(pluralize("key"), "keys");
        assert_eq!(pluralize("toy"), "toys");
    }

    #[test]
    fn pluralize_ss_ch_sh_x_z_to_es() {
        assert_eq!(pluralize("class"), "classes");
        assert_eq!(pluralize("watch"), "watches");
        assert_eq!(pluralize("dish"), "dishes");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("buzz"), "buzzes");
    }

    #[test]
    fn pluralize_accepts_pascal_case_input() {
        // pluralize first snake-cases the input.
        assert_eq!(pluralize("BlogPost"), "blog_posts");
        assert_eq!(pluralize("CartItem"), "cart_items");
    }

    #[test]
    fn singularize_irregulars() {
        assert_eq!(singularize("people"), "person");
        assert_eq!(singularize("children"), "child");
        assert_eq!(singularize("mice"), "mouse");
        assert_eq!(singularize("geese"), "goose");
    }

    #[test]
    fn singularize_ies_to_y() {
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("policies"), "policy");
        assert_eq!(singularize("cities"), "city");
    }

    #[test]
    fn singularize_strip_trailing_s() {
        assert_eq!(singularize("posts"), "post");
        assert_eq!(singularize("books"), "book");
        assert_eq!(singularize("tags"), "tag");
    }

    #[test]
    fn singularize_keeps_ss_words() {
        // "class" ends in "ss" so it shouldn't be stripped to "cla".
        assert_eq!(singularize("class"), "class");
        assert_eq!(singularize("dress"), "dress");
    }

    #[test]
    fn singularize_pluralize_roundtrip() {
        for word in &["post", "book", "tag", "category", "user", "comment"] {
            let round = singularize(&pluralize(word));
            assert_eq!(round, *word, "roundtrip broke for {}", word);
        }
    }

    #[test]
    fn parse_field_with_explicit_type() {
        assert_eq!(parse_field("title:string"), ("title".into(), "string".into()));
        assert_eq!(parse_field("body:text"), ("body".into(), "text".into()));
        assert_eq!(
            parse_field("user_id:references"),
            ("user_id".into(), "references".into())
        );
    }

    #[test]
    fn parse_field_default_type_is_string() {
        assert_eq!(parse_field("name"), ("name".into(), "string".into()));
    }

    #[test]
    fn parse_field_snake_cases_name() {
        assert_eq!(parse_field("PostTitle:string"), ("post_title".into(), "string".into()));
    }

    #[test]
    fn sql_type_for_known_types() {
        assert_eq!(sql_type_for("string"), "TEXT");
        assert_eq!(sql_type_for("text"), "TEXT");
        assert_eq!(sql_type_for("int"), "INTEGER");
        assert_eq!(sql_type_for("integer"), "INTEGER");
        assert_eq!(sql_type_for("bigint"), "INTEGER");
        assert_eq!(sql_type_for("references"), "INTEGER");
        assert_eq!(sql_type_for("float"), "REAL");
        assert_eq!(sql_type_for("decimal"), "REAL");
        assert_eq!(sql_type_for("bool"), "INTEGER");
        assert_eq!(sql_type_for("boolean"), "INTEGER");
        assert_eq!(sql_type_for("date"), "TEXT");
        assert_eq!(sql_type_for("datetime"), "TEXT");
        assert_eq!(sql_type_for("timestamp"), "TEXT");
        assert_eq!(sql_type_for("blob"), "BLOB");
        assert_eq!(sql_type_for("binary"), "BLOB");
    }

    #[test]
    fn sql_type_for_unknown_falls_back_to_text() {
        assert_eq!(sql_type_for("frobnicate"), "TEXT");
        assert_eq!(sql_type_for(""), "TEXT");
    }

    #[test]
    fn class_name_pascalizes_singular() {
        assert_eq!(class_name("post"), "Post");
        assert_eq!(class_name("posts"), "Post");
        assert_eq!(class_name("blog_post"), "BlogPost");
        assert_eq!(class_name("BlogPosts"), "BlogPost");
    }

    #[test]
    fn file_stem_is_snake_singular() {
        assert_eq!(file_stem("Post"), "post");
        assert_eq!(file_stem("posts"), "post");
        assert_eq!(file_stem("BlogPost"), "blog_post");
    }

    #[test]
    fn plural_snake_for_table_names() {
        assert_eq!(plural_snake("Post"), "posts");
        assert_eq!(plural_snake("BlogPost"), "blog_posts");
        assert_eq!(plural_snake("Category"), "categories");
        assert_eq!(plural_snake("Person"), "people");
    }

    #[test]
    fn ends_with_vowel_y_basic() {
        assert!(ends_with_vowel_y("day"));
        assert!(ends_with_vowel_y("key"));
        assert!(ends_with_vowel_y("boy"));
        assert!(!ends_with_vowel_y("city"));
        assert!(!ends_with_vowel_y("policy"));
        assert!(!ends_with_vowel_y("post"));
        assert!(!ends_with_vowel_y(""));
        assert!(!ends_with_vowel_y("a"));
    }

    #[test]
    fn migration_timestamp_is_14_digits() {
        let ts = migration_timestamp();
        assert_eq!(ts.len(), 14, "timestamp = {ts:?}");
        assert!(ts.chars().all(|c| c.is_ascii_digit()), "timestamp = {ts:?}");
    }

    #[test]
    fn migration_timestamp_is_monotonic_within_a_second() {
        // Even at the same instant the output must be sortable and parseable.
        let a = migration_timestamp();
        let b = migration_timestamp();
        assert!(b >= a, "{b:?} < {a:?}");
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_anchors() {
        // 2000-01-01 is day 10957 since epoch.
        assert_eq!(days_to_ymd(10957), (2000, 1, 1));
        // 2026-01-01 is day 20454 since epoch.
        assert_eq!(days_to_ymd(20454), (2026, 1, 1));
    }

    #[test]
    fn days_to_ymd_handles_leap_years() {
        // 2000 is a leap year (div by 400). Feb has 29 days. 2000-02-29
        // is day 10957 + 31 + 28 = 11016.
        assert_eq!(days_to_ymd(10957 + 31 + 28), (2000, 2, 29));
        // 2100 is NOT a leap year (div by 100 but not 400) — Feb has 28
        // days. 2100-03-01 = 2100-01-01 + 31 + 28 = day 47482 + 59 = 47541.
        // 2100-01-01 day-number: 130 years from 1970, 32 of which are
        // leap (1972..=2096 step 4 = 32, minus century 2000 still counts,
        // minus century 2100 not yet reached) = 32 leaps. 130*365 + 32 = 47482.
        assert_eq!(days_to_ymd(47482 + 31 + 28), (2100, 3, 1));
    }

    #[test]
    fn write_file_creates_and_skips_identical() {
        let tmp = std::env::temp_dir().join(format!("stryke_web_util_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let path = tmp.join("nested/dir/file.txt");
        write_file(&path, "hello").expect("first write");
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
        // Identical re-write is a no-op (no error).
        write_file(&path, "hello").expect("identical re-write");
        // Different content is rejected.
        assert!(write_file(&path, "different").is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn force_write_overwrites() {
        let tmp = std::env::temp_dir().join(format!("stryke_web_util_force_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let path = tmp.join("file.txt");
        force_write(&path, "first").expect("first");
        force_write(&path, "second").expect("overwrite");
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ensure_dir_idempotent() {
        let tmp = std::env::temp_dir().join(format!("stryke_web_util_dir_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        ensure_dir(&tmp).expect("create");
        ensure_dir(&tmp).expect("idempotent");
        assert!(tmp.is_dir());
        let _ = fs::remove_dir_all(&tmp);
    }
}
