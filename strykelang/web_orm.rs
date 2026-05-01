//! Web framework ORM + migrator runtime — PASS 3 + PASS 4.
//!
//! This module provides:
//!
//!   * `web_db_connect("sqlite://path")` — opens (and caches) one SQLite
//!     connection in the global `DB` slot. All other ORM/migrator builtins
//!     operate against it.
//!   * `web_db_execute(sql, [bindings])` / `web_db_query(sql, [bindings])`
//!     — raw SQL escape hatch.
//!   * `web_model_all` / `_find` / `_where` / `_create` / `_update`
//!     / `_destroy("posts", …)` — Active-Record-shaped CRUD that takes
//!     the table name as its first argument. Returns hashrefs; no Model
//!     base class needed.
//!   * `web_create_table` / `_drop_table` / `_add_column` / `_remove_column`
//!     — schema DSL the migration `up`/`down` blocks call.
//!   * `web_migrate` / `web_rollback` — tracks applied migrations in a
//!     `schema_migrations` table, runs each loaded `Migration` subclass's
//!     `up` (or `down`) method in order.

use crate::error::PerlError;
use crate::interpreter::{FlowOrError, Interpreter};
use crate::native_data::{exec_sql, perl_to_sql_value, query_sql};
use crate::value::PerlValue;
use indexmap::IndexMap;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

type Result<T> = std::result::Result<T, PerlError>;

// ── Global connection slot ──────────────────────────────────────────────
//
// Held inside Mutex<Option<Connection>> because rusqlite::Connection is
// Send but not Sync; holding the lock around every operation is fine
// because each request handler is short and SQLite itself is the
// concurrency control.

static DB: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

fn db_slot() -> &'static Mutex<Option<Connection>> {
    DB.get_or_init(|| Mutex::new(None))
}

fn with_db<F, R>(f: F, line: usize) -> Result<R>
where
    F: FnOnce(&Connection) -> Result<R>,
{
    let guard = db_slot().lock();
    match guard.as_ref() {
        Some(c) => f(c),
        None => Err(PerlError::runtime(
            "web orm: no database connection — call web_db_connect first",
            line,
        )),
    }
}

fn parse_db_url(url: &str) -> Result<String> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        return Ok(path.to_string());
    }
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        return Err(PerlError::runtime(
            "web orm: postgres adapter not implemented (PASS 5)",
            0,
        ));
    }
    // Bare path → treat as sqlite file.
    Ok(url.to_string())
}

pub(crate) fn web_db_connect(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let url = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "db/development.sqlite3".to_string());
    let path = parse_db_url(&url)?;
    if let Some(parent) = Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let conn = Connection::open(&path)
        .map_err(|e| PerlError::runtime(format!("web_db_connect: open {}: {}", path, e), line))?;
    // Sensible defaults for SQLite — same set Rails ships in dev.
    let _ = conn.execute_batch(
        "PRAGMA journal_mode = WAL;\n\
         PRAGMA foreign_keys = ON;\n\
         PRAGMA synchronous = NORMAL;\n",
    );
    *db_slot().lock() = Some(conn);
    Ok(PerlValue::UNDEF)
}

// ── Raw SQL escape hatch ────────────────────────────────────────────────

fn perl_args_as_sql(values: &[PerlValue]) -> Vec<rusqlite::types::Value> {
    values.iter().map(perl_to_sql_value).collect()
}

pub(crate) fn web_db_execute(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let sql = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_db_execute: sql required", line))?;
    let bindings = bindings_from_arg(args.get(1));
    let bound = perl_args_as_sql(&bindings);
    let n = with_db(|c| exec_sql(c, &sql, &bound), line)?;
    Ok(PerlValue::integer(n as i64))
}

pub(crate) fn web_db_query(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let sql = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_db_query: sql required", line))?;
    let bindings = bindings_from_arg(args.get(1));
    let bound = perl_args_as_sql(&bindings);
    let result = with_db(|c| query_sql(c, &sql, &bound, line), line)?;
    Ok(wrap_array_as_ref(result))
}

fn bindings_from_arg(v: Option<&PerlValue>) -> Vec<PerlValue> {
    match v {
        Some(arg) => arg
            .as_array_ref()
            .map(|a| a.read().clone())
            .unwrap_or_else(|| arg.clone().to_list()),
        None => Vec::new(),
    }
}

fn wrap_array_as_ref(v: PerlValue) -> PerlValue {
    if v.as_array_ref().is_some() {
        return v;
    }
    let list = v.to_list();
    PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(list)))
}

// ── Active-Record-shaped CRUD ───────────────────────────────────────────

pub(crate) fn web_model_all(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_all", line)?;
    let sql = format!("SELECT * FROM {} ORDER BY id ASC", quote_ident(&table));
    let result = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    Ok(wrap_array_as_ref(result))
}

pub(crate) fn web_model_find(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_find", line)?;
    let id = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("web_model_find: id required", line))?;
    let sql = format!(
        "SELECT * FROM {} WHERE id = ?1 LIMIT 1",
        quote_ident(&table)
    );
    let bound = perl_args_as_sql(std::slice::from_ref(id));
    let rows = with_db(|c| query_sql(c, &sql, &bound, line), line)?;
    Ok(first_row_or_undef(rows))
}

pub(crate) fn web_model_where(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_where", line)?;
    let cond = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| PerlError::runtime("web_model_where: second arg must be a hashref", line))?;
    let mut sql = format!("SELECT * FROM {}", quote_ident(&table));
    let mut bindings: Vec<rusqlite::types::Value> = Vec::new();
    if !cond.is_empty() {
        sql.push_str(" WHERE ");
        let mut parts = Vec::with_capacity(cond.len());
        for (i, (k, v)) in cond.iter().enumerate() {
            parts.push(format!("{} = ?{}", quote_ident(k), i + 1));
            bindings.push(perl_to_sql_value(v));
        }
        sql.push_str(&parts.join(" AND "));
    }
    sql.push_str(" ORDER BY id ASC");
    let result = with_db(|c| query_sql(c, &sql, &bindings, line), line)?;
    Ok(wrap_array_as_ref(result))
}

pub(crate) fn web_model_create(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_create", line)?;
    let attrs = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| {
            PerlError::runtime(
                "web_model_create: second arg must be a hashref of attrs",
                line,
            )
        })?;
    if attrs.is_empty() {
        return Err(PerlError::runtime(
            "web_model_create: attrs hashref must not be empty",
            line,
        ));
    }
    // Drop reserved auto-managed columns from the INSERT — we set them
    // on the server side. `id` left in if user supplied it explicitly.
    let now = current_timestamp();
    let mut working = attrs.clone();
    working.insert("created_at".into(), PerlValue::string(now.clone()));
    working.insert("updated_at".into(), PerlValue::string(now));

    // Filter to columns that actually exist on the table — silently drops
    // unknowns so callers can pass `web_params()` without sanitising.
    let cols = table_columns(&table, line)?;
    working.retain(|k, _| cols.iter().any(|c| c == k));

    if working.is_empty() {
        return Err(PerlError::runtime(
            format!(
                "web_model_create: no matching columns on {} (cols: {})",
                table,
                cols.join(", ")
            ),
            line,
        ));
    }

    let mut col_list = Vec::new();
    let mut placeholders = Vec::new();
    let mut bindings = Vec::new();
    for (i, (k, v)) in working.iter().enumerate() {
        col_list.push(quote_ident(k));
        placeholders.push(format!("?{}", i + 1));
        bindings.push(perl_to_sql_value(v));
    }
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        quote_ident(&table),
        col_list.join(", "),
        placeholders.join(", ")
    );
    let new_id = with_db(
        |c| {
            exec_sql(c, &sql, &bindings)?;
            Ok(c.last_insert_rowid())
        },
        line,
    )?;
    let find_sql = format!(
        "SELECT * FROM {} WHERE id = ?1 LIMIT 1",
        quote_ident(&table)
    );
    let bound = vec![rusqlite::types::Value::Integer(new_id)];
    let row = with_db(|c| query_sql(c, &find_sql, &bound, line), line)?;
    Ok(first_row_or_undef(row))
}

pub(crate) fn web_model_update(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_update", line)?;
    let id = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("web_model_update: id required", line))?;
    let attrs = args
        .get(2)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| {
            PerlError::runtime(
                "web_model_update: third arg must be a hashref of attrs",
                line,
            )
        })?;
    let cols = table_columns(&table, line)?;
    let mut working = attrs.clone();
    working.insert("updated_at".into(), PerlValue::string(current_timestamp()));
    working.retain(|k, _| cols.iter().any(|c| c == k) && k != "id");
    if working.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    let mut sets = Vec::new();
    let mut bindings = Vec::new();
    for (i, (k, v)) in working.iter().enumerate() {
        sets.push(format!("{} = ?{}", quote_ident(k), i + 1));
        bindings.push(perl_to_sql_value(v));
    }
    bindings.push(perl_to_sql_value(id));
    let sql = format!(
        "UPDATE {} SET {} WHERE id = ?{}",
        quote_ident(&table),
        sets.join(", "),
        bindings.len()
    );
    let n = with_db(|c| exec_sql(c, &sql, &bindings), line)?;
    Ok(PerlValue::integer(n as i64))
}

pub(crate) fn web_model_destroy(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_destroy", line)?;
    let id = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("web_model_destroy: id required", line))?;
    let sql = format!("DELETE FROM {} WHERE id = ?1", quote_ident(&table));
    let bound = perl_args_as_sql(std::slice::from_ref(id));
    let n = with_db(|c| exec_sql(c, &sql, &bound), line)?;
    Ok(PerlValue::integer(n as i64))
}

/// Soft delete — sets `deleted_at` to the current timestamp instead of
/// removing the row. Pair with `web_model_visible` to filter them out
/// of subsequent queries.
pub(crate) fn web_model_soft_destroy(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_soft_destroy", line)?;
    let id = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("web_model_soft_destroy: id required", line))?;
    let cols = table_columns(&table, line)?;
    if !cols.iter().any(|c| c == "deleted_at") {
        // Add the column on the fly so soft-delete works on tables
        // created before this builtin landed.
        let alter = format!(
            "ALTER TABLE {} ADD COLUMN deleted_at TEXT",
            quote_ident(&table)
        );
        with_db(|c| exec_sql(c, &alter, &[]), line)?;
    }
    let sql = format!(
        "UPDATE {} SET deleted_at = ?1 WHERE id = ?2",
        quote_ident(&table)
    );
    let bound = vec![
        rusqlite::types::Value::Text(current_timestamp()),
        perl_to_sql_value(id),
    ];
    let n = with_db(|c| exec_sql(c, &sql, &bound), line)?;
    Ok(PerlValue::integer(n as i64))
}

/// Paginated SELECT: returns a hashref `{rows => [...], total => N,
/// page => P, per_page => K, total_pages => …}`.
pub(crate) fn web_model_paginate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_paginate", line)?;
    let opts = parse_kv(&args[1.min(args.len())..]);
    let page = opts.get("page").map(|v| v.to_int().max(1)).unwrap_or(1);
    let per_page = opts
        .get("per_page")
        .map(|v| v.to_int().clamp(1, 1000))
        .unwrap_or(25);
    let order = opts
        .get("order")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "id DESC".to_string());

    let count_sql = format!("SELECT count(*) AS c FROM {}", quote_ident(&table));
    let count_rows = with_db(|c| query_sql(c, &count_sql, &[], line), line)?;
    let total: i64 = count_rows
        .to_list()
        .first()
        .and_then(|r| r.as_hash_ref())
        .and_then(|h| h.read().get("c").map(|v| v.to_int()))
        .unwrap_or(0);

    let offset = (page - 1) * per_page;
    let sql = format!(
        "SELECT * FROM {} ORDER BY {} LIMIT {} OFFSET {}",
        quote_ident(&table),
        sanitize_order(&order),
        per_page,
        offset
    );
    let rows = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    let rows_ref = wrap_array_as_ref(rows);
    let total_pages = if total <= 0 {
        0
    } else {
        ((total - 1) / per_page.max(1)) + 1
    };

    let mut out = IndexMap::new();
    out.insert("rows".to_string(), rows_ref);
    out.insert("total".to_string(), PerlValue::integer(total));
    out.insert("page".to_string(), PerlValue::integer(page));
    out.insert("per_page".to_string(), PerlValue::integer(per_page));
    out.insert("total_pages".to_string(), PerlValue::integer(total_pages));
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(out))))
}

/// LIKE-based search across one or more columns. `web_model_search("posts",
/// "stryke", cols => ["title", "body"])` returns matching rows.
pub(crate) fn web_model_search(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_search", line)?;
    let query = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let opts = parse_kv(&args[2.min(args.len())..]);
    let cols: Vec<String> = if let Some(v) = opts.get("cols") {
        if let Some(arr) = v.as_array_ref() {
            arr.read().iter().map(|x| x.to_string()).collect()
        } else {
            v.clone().to_list().iter().map(|x| x.to_string()).collect()
        }
    } else {
        return Err(PerlError::runtime(
            "web_model_search: pass cols => [\"col1\", \"col2\"]",
            line,
        ));
    };
    if cols.is_empty() {
        return Ok(wrap_array_as_ref(PerlValue::array(Vec::new())));
    }
    let where_clause = cols
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} LIKE ?{}", quote_ident(c), i + 1))
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "SELECT * FROM {} WHERE {} ORDER BY id DESC LIMIT 200",
        quote_ident(&table),
        where_clause
    );
    let pattern = format!("%{}%", query);
    let bindings: Vec<rusqlite::types::Value> = (0..cols.len())
        .map(|_| rusqlite::types::Value::Text(pattern.clone()))
        .collect();
    let rows = with_db(|c| query_sql(c, &sql, &bindings, line), line)?;
    Ok(wrap_array_as_ref(rows))
}

/// `web_model_count("posts")` → row count.
pub(crate) fn web_model_count(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_count", line)?;
    let sql = format!("SELECT count(*) AS c FROM {}", quote_ident(&table));
    let rows = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    let n = rows
        .to_list()
        .first()
        .and_then(|r| r.as_hash_ref())
        .and_then(|h| h.read().get("c").map(|v| v.to_int()))
        .unwrap_or(0);
    Ok(PerlValue::integer(n))
}

/// `web_model_first("posts")` / `web_model_last("posts")` — single-row helpers.
pub(crate) fn web_model_first(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_first", line)?;
    let sql = format!(
        "SELECT * FROM {} ORDER BY id ASC LIMIT 1",
        quote_ident(&table)
    );
    let rows = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    Ok(first_row_or_undef(rows))
}

pub(crate) fn web_model_last(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_last", line)?;
    let sql = format!(
        "SELECT * FROM {} ORDER BY id DESC LIMIT 1",
        quote_ident(&table)
    );
    let rows = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    Ok(first_row_or_undef(rows))
}

/// `web_model_increment("posts", id, "comments_count", 1)` — atomic
/// `UPDATE … SET col = col + delta` for counter caches.
pub(crate) fn web_model_increment(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_increment", line)?;
    let id = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("web_model_increment: id required", line))?;
    let col = args
        .get(2)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_model_increment: column required", line))?;
    let by = args.get(3).map(|v| v.to_int()).unwrap_or(1);
    let sql = format!(
        "UPDATE {} SET {} = COALESCE({},0) + ?1 WHERE id = ?2",
        quote_ident(&table),
        quote_ident(&col),
        quote_ident(&col)
    );
    let bound = vec![rusqlite::types::Value::Integer(by), perl_to_sql_value(id)];
    let n = with_db(|c| exec_sql(c, &sql, &bound), line)?;
    Ok(PerlValue::integer(n as i64))
}

/// `web_model_with("posts", "user")` — preload one belongs_to relation.
/// Returns posts with `_user => +{...}` attached. Uses a single IN
/// query to dodge n+1.
pub(crate) fn web_model_with(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_model_with", line)?;
    let assoc = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_model_with: assoc name required", line))?;
    let foreign_key = format!("{}_id", assoc);
    let assoc_table = pluralize_simple(&assoc);
    let sql = format!("SELECT * FROM {} ORDER BY id ASC", quote_ident(&table));
    let parents = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    let parent_list = parents.to_list();
    let ids: Vec<i64> = parent_list
        .iter()
        .filter_map(|r| {
            r.as_hash_ref()
                .and_then(|h| h.read().get(&foreign_key).map(|v| v.to_int()))
        })
        .collect();
    let mut by_id: IndexMap<i64, PerlValue> = IndexMap::new();
    if !ids.is_empty() {
        let placeholders = (1..=ids.len())
            .map(|i| format!("?{}", i))
            .collect::<Vec<_>>()
            .join(",");
        let bind: Vec<rusqlite::types::Value> = ids
            .iter()
            .map(|i| rusqlite::types::Value::Integer(*i))
            .collect();
        let assoc_sql = format!(
            "SELECT * FROM {} WHERE id IN ({})",
            quote_ident(&assoc_table),
            placeholders
        );
        let assoc_rows = with_db(|c| query_sql(c, &assoc_sql, &bind, line), line)?;
        for row in assoc_rows.to_list() {
            if let Some(h) = row.as_hash_ref() {
                if let Some(id) = h.read().get("id").map(|v| v.to_int()) {
                    by_id.insert(id, row.clone());
                }
            }
        }
    }
    let mut out = Vec::new();
    for parent in parent_list {
        if let Some(h) = parent.as_hash_ref() {
            let mut new_map = h.read().clone();
            if let Some(fk) = new_map.get(&foreign_key).cloned() {
                let id = fk.to_int();
                if let Some(child) = by_id.get(&id) {
                    new_map.insert(format!("_{}", assoc), child.clone());
                }
            }
            out.push(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
                new_map,
            ))));
        }
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn pluralize_simple(s: &str) -> String {
    if s.ends_with('y')
        && !s.ends_with("ay")
        && !s.ends_with("ey")
        && !s.ends_with("oy")
        && !s.ends_with("uy")
    {
        format!("{}ies", &s[..s.len() - 1])
    } else if s.ends_with('s')
        || s.ends_with('x')
        || s.ends_with('z')
        || s.ends_with("sh")
        || s.ends_with("ch")
    {
        format!("{}es", s)
    } else {
        format!("{}s", s)
    }
}

/// `web_db_transaction` — opens a transaction, runs the BEGIN/COMMIT
/// pair around the SQL string the caller passes, returning rollback on
/// error. For multi-step txn use `web_db_execute("BEGIN")`/COMMIT.
pub(crate) fn web_db_begin(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    with_db(|c| exec_sql(c, "BEGIN", &[]), line)?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_db_commit(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    with_db(|c| exec_sql(c, "COMMIT", &[]), line)?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_db_rollback(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    with_db(|c| exec_sql(c, "ROLLBACK", &[]), line)?;
    Ok(PerlValue::UNDEF)
}

// ── Validations ────────────────────────────────────────────────────────
//
// `web_validate($attrs, +{ field => "presence,length:1..100,format:^\\w+$" })`
// returns `+{ ok => 1 }` on success, `+{ ok => 0, errors => +{...} }`
// otherwise. Validators: `presence`, `length:MIN..MAX`, `format:REGEX`,
// `numericality`, `inclusion:a|b|c`, `confirmation:other_field`.

pub(crate) fn web_validate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let attrs = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| PerlError::runtime("web_validate: first arg must be a hashref", line))?;
    let rules = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| PerlError::runtime("web_validate: second arg must be a hashref", line))?;

    let mut errors: IndexMap<String, PerlValue> = IndexMap::new();
    for (field, spec_v) in &rules {
        let spec = spec_v.to_string();
        let value = attrs.get(field).cloned().unwrap_or(PerlValue::UNDEF);
        let s = if value.is_undef() {
            String::new()
        } else {
            value.to_string()
        };
        for raw in spec.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let (kind, arg) = raw.split_once(':').unwrap_or((raw, ""));
            let err = check_one_validator(field, &s, &value, &attrs, kind, arg);
            if let Some(msg) = err {
                errors.entry(field.clone()).or_insert_with(|| {
                    PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(Vec::new())))
                });
                if let Some(arr) = errors.get(field).and_then(|v| v.as_array_ref()) {
                    arr.write().push(PerlValue::string(msg));
                }
            }
        }
    }

    let mut out = IndexMap::new();
    out.insert(
        "ok".to_string(),
        PerlValue::integer(if errors.is_empty() { 1 } else { 0 }),
    );
    out.insert(
        "errors".to_string(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(errors))),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(out))))
}

fn check_one_validator(
    field: &str,
    s: &str,
    raw: &PerlValue,
    attrs: &IndexMap<String, PerlValue>,
    kind: &str,
    arg: &str,
) -> Option<String> {
    match kind {
        "presence" => {
            if s.trim().is_empty() {
                return Some(format!("{} can't be blank", field));
            }
            None
        }
        "length" => {
            let (min, max) = parse_range(arg).unwrap_or((0, i64::MAX));
            let n = s.chars().count() as i64;
            if n < min {
                return Some(format!("{} too short (minimum {} characters)", field, min));
            }
            if n > max {
                return Some(format!("{} too long (maximum {} characters)", field, max));
            }
            None
        }
        "format" => {
            if let Ok(re) = regex::Regex::new(arg) {
                if !re.is_match(s) {
                    return Some(format!("{} format is invalid", field));
                }
            }
            None
        }
        "numericality" => {
            if raw.is_undef() {
                return Some(format!("{} is not a number", field));
            }
            if s.parse::<f64>().is_err() {
                return Some(format!("{} is not a number", field));
            }
            None
        }
        "inclusion" => {
            let allowed: Vec<&str> = arg.split('|').collect();
            if !allowed.iter().any(|a| *a == s) {
                return Some(format!("{} is not in the list", field));
            }
            None
        }
        "confirmation" => {
            let other = attrs.get(arg).map(|v| v.to_string()).unwrap_or_default();
            if other != s {
                return Some(format!("{} doesn't match {}", field, arg));
            }
            None
        }
        _ => None,
    }
}

fn parse_range(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.splitn(2, "..").collect();
    if parts.len() != 2 {
        return s.parse::<i64>().ok().map(|n| (0, n));
    }
    let a = parts[0].trim().parse::<i64>().ok()?;
    let b = parts[1].trim().parse::<i64>().ok()?;
    Some((a, b))
}

fn parse_kv(args: &[PerlValue]) -> IndexMap<String, PerlValue> {
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].clone());
        i += 2;
    }
    out
}

fn sanitize_order(s: &str) -> String {
    // Allow `col`, `col DESC`, `col1 ASC, col2 DESC`. Reject anything
    // with semicolons / parens / quotes — we're putting this directly in
    // SQL because rusqlite param binding doesn't apply to ORDER BY.
    let bad = s.chars().any(|c| matches!(c, ';' | '(' | ')' | '"' | '\''));
    if bad {
        "id DESC".to_string()
    } else {
        s.to_string()
    }
}

// ── Schema DSL (used inside migration up/down blocks) ───────────────────

pub(crate) fn web_create_table(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = require_table(args.first(), "web_create_table", line)?;
    let cols = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .unwrap_or_default();
    let mut col_defs: Vec<String> = vec!["id INTEGER PRIMARY KEY AUTOINCREMENT".to_string()];
    for (cname, ty) in &cols {
        col_defs.push(format!(
            "{} {}",
            quote_ident(cname),
            stryke_type_to_sql(&ty.to_string())
        ));
    }
    col_defs.push("created_at TEXT".to_string());
    col_defs.push("updated_at TEXT".to_string());
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS {} ({})",
        quote_ident(&name),
        col_defs.join(", ")
    );
    with_db(|c| exec_sql(c, &sql, &[]), line)?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_drop_table(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = require_table(args.first(), "web_drop_table", line)?;
    let sql = format!("DROP TABLE IF EXISTS {}", quote_ident(&name));
    with_db(|c| exec_sql(c, &sql, &[]), line)?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_add_column(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_add_column", line)?;
    let col = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_add_column: column name required", line))?;
    let ty = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "TEXT".to_string());
    let sql = format!(
        "ALTER TABLE {} ADD COLUMN {} {}",
        quote_ident(&table),
        quote_ident(&col),
        stryke_type_to_sql(&ty)
    );
    with_db(|c| exec_sql(c, &sql, &[]), line)?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_remove_column(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let table = require_table(args.first(), "web_remove_column", line)?;
    let col = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_remove_column: column name required", line))?;
    // SQLite 3.35+ supports `DROP COLUMN`.
    let sql = format!(
        "ALTER TABLE {} DROP COLUMN {}",
        quote_ident(&table),
        quote_ident(&col)
    );
    with_db(|c| exec_sql(c, &sql, &[]), line)?;
    Ok(PerlValue::UNDEF)
}

// ── Migrator ───────────────────────────────────────────────────────────
//
// The user's `class CreatePosts { fn up { ... } fn down { ... } }`
// definitions land in `interp.class_defs` when the migration files are
// `require`d. The migrator picks them up by name pattern and invokes
// their `up` / `down` blocks in deterministic order.

impl Interpreter {
    pub(crate) fn web_migrate(&mut self, _args: &[PerlValue], line: usize) -> Result<PerlValue> {
        with_db(
            |c| {
                exec_sql(
                    c,
                    "CREATE TABLE IF NOT EXISTS schema_migrations (\
                     version TEXT PRIMARY KEY, applied_at TEXT)",
                    &[],
                )
            },
            line,
        )?;
        let applied = applied_versions(line)?;
        let migrations = self.collect_migration_classes();
        let mut applied_now: Vec<String> = Vec::new();
        for (version, class_name) in &migrations {
            if applied.contains(version) {
                continue;
            }
            self.invoke_migration_block(class_name, "up", line)?;
            with_db(
                |c| {
                    exec_sql(
                        c,
                        "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                        &[
                            rusqlite::types::Value::Text(version.clone()),
                            rusqlite::types::Value::Text(current_timestamp()),
                        ],
                    )
                },
                line,
            )?;
            applied_now.push(version.clone());
            eprintln!("== {}: migrated", class_name);
        }
        Ok(PerlValue::integer(applied_now.len() as i64))
    }

    pub(crate) fn web_rollback(&mut self, _args: &[PerlValue], line: usize) -> Result<PerlValue> {
        let applied = applied_versions(line)?;
        let mut migrations = self.collect_migration_classes();
        migrations.sort_by(|a, b| b.0.cmp(&a.0)); // descending
        for (version, class_name) in &migrations {
            if !applied.contains(version) {
                continue;
            }
            self.invoke_migration_block(class_name, "down", line)?;
            with_db(
                |c| {
                    exec_sql(
                        c,
                        "DELETE FROM schema_migrations WHERE version = ?1",
                        &[rusqlite::types::Value::Text(version.clone())],
                    )
                },
                line,
            )?;
            eprintln!("== {}: rolled back", class_name);
            return Ok(PerlValue::integer(1));
        }
        Ok(PerlValue::integer(0))
    }

    /// Walk `class_defs` looking for classes whose name matches a known
    /// migration pattern. A migration class is one whose body contains
    /// both `up` and `down` methods. The `version` is the timestamp the
    /// generator stamped in the *file path* — but at runtime we don't
    /// have file paths anymore, so we sort by class name lexicographic
    /// order. Generators emit `Create${Plural}` / `Add${X}To${Y}` style
    /// and tag each with a deterministic prefix in PASS 4.5; for now,
    /// stable sort by class name suffices for create-then-alter chains.
    fn collect_migration_classes(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = self
            .class_defs
            .iter()
            .filter(|(_, def)| {
                let has_up = def.methods.iter().any(|m| m.name == "up");
                let has_down = def.methods.iter().any(|m| m.name == "down");
                has_up && has_down
            })
            .map(|(name, _)| (name.clone(), name.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn invoke_migration_block(
        &mut self,
        class_name: &str,
        method: &str,
        line: usize,
    ) -> Result<()> {
        let class_def = self.class_defs.get(class_name).cloned().ok_or_else(|| {
            PerlError::runtime(format!("migrator: class not found: {}", class_name), line)
        })?;
        let m = class_def
            .methods
            .iter()
            .find(|m| m.name == method)
            .cloned()
            .ok_or_else(|| {
                PerlError::runtime(
                    format!("migrator: {}::{} not defined", class_name, method),
                    line,
                )
            })?;
        let body = m.body.ok_or_else(|| {
            PerlError::runtime(
                format!("migrator: {}::{} has no body", class_name, method),
                line,
            )
        })?;
        match self.call_static_class_method(&body, &m.params, vec![], line) {
            Ok(_) | Err(FlowOrError::Flow(_)) => Ok(()),
            Err(FlowOrError::Error(e)) => Err(e),
        }
    }
}

fn applied_versions(line: usize) -> Result<Vec<String>> {
    let rows = with_db(
        |c| query_sql(c, "SELECT version FROM schema_migrations", &[], line),
        line,
    )?;
    let list = rows.to_list();
    Ok(list
        .iter()
        .filter_map(|r| {
            r.as_hash_ref()
                .map(|h| h.read().get("version").cloned())
                .flatten()
                .map(|v| v.to_string())
        })
        .collect())
}

// ── Helpers ────────────────────────────────────────────────────────────

fn require_table(arg: Option<&PerlValue>, what: &str, line: usize) -> Result<String> {
    let table = arg
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime(format!("{}: table name required", what), line))?;
    if table.is_empty() {
        return Err(PerlError::runtime(
            format!("{}: table name must not be empty", what),
            line,
        ));
    }
    Ok(table)
}

fn quote_ident(name: &str) -> String {
    // Defensive: only allow identifiers SQLite will accept without
    // quoting (alpha + digit + underscore). Reject anything else by
    // double-quoting and escaping internal quotes — same shape Rails
    // uses for `quote_column_name`.
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn current_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // ISO-8601 minus tz. PASS 5 wires real `chrono` formatting.
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    let s = secs % 60;
    let m = mins % 60;
    let h = hours % 24;
    // Days since 1970-01-01 → Y/M/D via simple Howard Hinnant algorithm.
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_num = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m_num <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m_num, d, h, m, s)
}

fn stryke_type_to_sql(ty: &str) -> &'static str {
    match ty.to_ascii_lowercase().as_str() {
        "string" | "str" | "varchar" => "TEXT",
        "text" => "TEXT",
        "int" | "integer" | "bigint" => "INTEGER",
        "float" | "decimal" | "real" | "double" => "REAL",
        "bool" | "boolean" => "INTEGER",
        "date" | "datetime" | "timestamp" => "TEXT",
        "blob" | "bytes" => "BLOB",
        "references" => "INTEGER",
        _ => "TEXT",
    }
}

fn table_columns(table: &str, line: usize) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", quote_ident(table));
    let rows = with_db(|c| query_sql(c, &sql, &[], line), line)?;
    let list = rows.to_list();
    let mut out = Vec::new();
    for row in list {
        if let Some(h) = row.as_hash_ref() {
            if let Some(n) = h.read().get("name") {
                out.push(n.to_string());
            }
        }
    }
    Ok(out)
}

fn first_row_or_undef(rows: PerlValue) -> PerlValue {
    let list = rows.to_list();
    list.into_iter().next().unwrap_or(PerlValue::UNDEF)
}

// ── Background job queue ─────────────────────────────────────────────
//
// SQLite-backed `jobs` table:
//   id INTEGER PRIMARY KEY, name TEXT, args_json TEXT,
//   status TEXT (pending|running|done|failed),
//   queue TEXT (default), priority INTEGER,
//   created_at TEXT, locked_at TEXT, ran_at TEXT,
//   error TEXT, attempts INTEGER, max_attempts INTEGER
//
// Builtins:
//   web_jobs_init()                                    create table if missing
//   web_job_enqueue("name", +{...args}, queue=>..., max_attempts=>3, priority=>0)
//   web_job_dequeue(queue=>"default")                  → +{id,name,args,...} or undef
//   web_job_complete(id)                               → 1
//   web_job_fail(id, error=>"...")                     retry if attempts<max
//   web_jobs_list(queue=>..., status=>..., limit=>50)  → arrayref
//   web_jobs_stats()                                   → +{pending, running, done, failed}
//   web_job_purge(status=>"done", older_than=>"7d")    cleanup

const JOBS_DDL: &str = "\
CREATE TABLE IF NOT EXISTS jobs (\n\
  id INTEGER PRIMARY KEY,\n\
  name TEXT NOT NULL,\n\
  args_json TEXT NOT NULL DEFAULT '{}',\n\
  status TEXT NOT NULL DEFAULT 'pending',\n\
  queue TEXT NOT NULL DEFAULT 'default',\n\
  priority INTEGER NOT NULL DEFAULT 0,\n\
  attempts INTEGER NOT NULL DEFAULT 0,\n\
  max_attempts INTEGER NOT NULL DEFAULT 1,\n\
  created_at TEXT NOT NULL,\n\
  locked_at TEXT,\n\
  ran_at TEXT,\n\
  error TEXT\n\
);\n\
CREATE INDEX IF NOT EXISTS idx_jobs_status_queue ON jobs(status, queue, priority DESC, id);\n";

pub(crate) fn web_jobs_init(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    with_db(
        |c| {
            c.execute_batch(JOBS_DDL)
                .map_err(|e| PerlError::runtime(format!("web_jobs_init: {}", e), line))?;
            Ok(())
        },
        line,
    )?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_job_enqueue(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_job_enqueue: name required", line))?;
    let args_json = args
        .get(1)
        .map(|v| crate::native_data::json_encode(v).unwrap_or_else(|_| "{}".to_string()))
        .unwrap_or_else(|| "{}".to_string());

    let kv = parse_kv(&args[2.min(args.len())..]);
    let queue = kv
        .get("queue")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "default".to_string());
    let priority = kv.get("priority").map(|v| v.to_int()).unwrap_or(0);
    let max_attempts = kv
        .get("max_attempts")
        .map(|v| v.to_int().max(1))
        .unwrap_or(1);
    let created_at = current_timestamp();

    let id: i64 = with_db(
        |c| {
            // Upsert via raw INSERT to capture the rowid.
            let mut stmt = c.prepare("INSERT INTO jobs (name, args_json, status, queue, priority, max_attempts, created_at) VALUES (?, ?, 'pending', ?, ?, ?, ?)")
                .map_err(|e| PerlError::runtime(format!("web_job_enqueue: {}", e), line))?;
            stmt.execute(rusqlite::params![
                &name,
                &args_json,
                &queue,
                priority,
                max_attempts,
                &created_at,
            ])
            .map_err(|e| PerlError::runtime(format!("web_job_enqueue: {}", e), line))?;
            Ok(c.last_insert_rowid())
        },
        line,
    )?;
    Ok(PerlValue::integer(id))
}

pub(crate) fn web_job_dequeue(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let kv = parse_kv(args);
    let queue = kv
        .get("queue")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "default".to_string());
    let now = current_timestamp();
    let row: Option<(i64, String, String, i64, i64)> = with_db(
        |c| {
            // Pick highest-priority oldest pending job; mark as running.
            let id: Option<i64> = c
                .query_row(
                    "SELECT id FROM jobs WHERE status = 'pending' AND queue = ? ORDER BY priority DESC, id ASC LIMIT 1",
                    rusqlite::params![&queue],
                    |r| r.get(0),
                )
                .ok();
            let Some(id) = id else {
                return Ok(None);
            };
            let updated = c
                .execute(
                    "UPDATE jobs SET status = 'running', locked_at = ?, attempts = attempts + 1 WHERE id = ? AND status = 'pending'",
                    rusqlite::params![&now, id],
                )
                .map_err(|e| PerlError::runtime(format!("web_job_dequeue: {}", e), line))?;
            if updated == 0 {
                return Ok(None);
            }
            let row = c
                .query_row(
                    "SELECT id, name, args_json, attempts, max_attempts FROM jobs WHERE id = ?",
                    rusqlite::params![id],
                    |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    },
                )
                .map_err(|e| PerlError::runtime(format!("web_job_dequeue: {}", e), line))?;
            Ok(Some(row))
        },
        line,
    )?;
    match row {
        None => Ok(PerlValue::UNDEF),
        Some((id, name, args_json, attempts, max_attempts)) => {
            let mut h = IndexMap::new();
            h.insert("id".to_string(), PerlValue::integer(id));
            h.insert("name".to_string(), PerlValue::string(name));
            h.insert(
                "args_json".to_string(),
                PerlValue::string(args_json.clone()),
            );
            // Provide pre-decoded args hashref/arrayref for ergonomic dispatch.
            let parsed = crate::native_data::json_decode(&args_json).unwrap_or(PerlValue::UNDEF);
            h.insert("args".to_string(), parsed);
            h.insert("attempts".to_string(), PerlValue::integer(attempts));
            h.insert("max_attempts".to_string(), PerlValue::integer(max_attempts));
            Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
        }
    }
}

pub(crate) fn web_job_complete(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_int())
        .ok_or_else(|| PerlError::runtime("web_job_complete: id required", line))?;
    let now = current_timestamp();
    with_db(
        |c| {
            c.execute(
                "UPDATE jobs SET status = 'done', ran_at = ?, error = NULL WHERE id = ?",
                rusqlite::params![&now, id],
            )
            .map_err(|e| PerlError::runtime(format!("web_job_complete: {}", e), line))?;
            Ok(())
        },
        line,
    )?;
    Ok(PerlValue::integer(1))
}

pub(crate) fn web_job_fail(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_int())
        .ok_or_else(|| PerlError::runtime("web_job_fail: id required", line))?;
    let kv = parse_kv(&args[1.min(args.len())..]);
    let error = kv
        .get("error")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "<unspecified>".to_string());
    let now = current_timestamp();
    let new_status = with_db(
        |c| {
            // If attempts < max_attempts, retry; else mark failed.
            let (attempts, max_attempts): (i64, i64) = c
                .query_row(
                    "SELECT attempts, max_attempts FROM jobs WHERE id = ?",
                    rusqlite::params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| PerlError::runtime(format!("web_job_fail: {}", e), line))?;
            let next = if attempts < max_attempts {
                "pending"
            } else {
                "failed"
            };
            c.execute(
                "UPDATE jobs SET status = ?, ran_at = ?, error = ? WHERE id = ?",
                rusqlite::params![next, &now, &error, id],
            )
            .map_err(|e| PerlError::runtime(format!("web_job_fail: {}", e), line))?;
            Ok(next.to_string())
        },
        line,
    )?;
    Ok(PerlValue::string(new_status))
}

pub(crate) fn web_jobs_list(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let kv = parse_kv(args);
    let queue = kv.get("queue").map(|v| v.to_string());
    let status = kv.get("status").map(|v| v.to_string());
    let limit = kv.get("limit").map(|v| v.to_int().max(1)).unwrap_or(50);
    let mut sql = String::from(
        "SELECT id, name, args_json, status, queue, priority, attempts, max_attempts, created_at, locked_at, ran_at, error FROM jobs WHERE 1=1",
    );
    let mut binds: Vec<rusqlite::types::Value> = Vec::new();
    if let Some(q) = queue {
        sql.push_str(" AND queue = ?");
        binds.push(rusqlite::types::Value::Text(q));
    }
    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        binds.push(rusqlite::types::Value::Text(s));
    }
    sql.push_str(" ORDER BY id DESC LIMIT ?");
    binds.push(rusqlite::types::Value::Integer(limit));

    let rows: Vec<PerlValue> = with_db(
        |c| {
            let mut stmt = c
                .prepare(&sql)
                .map_err(|e| PerlError::runtime(format!("web_jobs_list: {}", e), line))?;
            let params = rusqlite::params_from_iter(binds.iter());
            let row_iter = stmt
                .query_map(params, |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, i64>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, String>(8)?,
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, Option<String>>(10)?,
                        r.get::<_, Option<String>>(11)?,
                    ))
                })
                .map_err(|e| PerlError::runtime(format!("web_jobs_list: {}", e), line))?;
            let mut out: Vec<PerlValue> = Vec::new();
            for r in row_iter {
                let (
                    id,
                    name,
                    args_json,
                    status,
                    queue,
                    priority,
                    attempts,
                    max_attempts,
                    created_at,
                    locked_at,
                    ran_at,
                    error,
                ) = r.map_err(|e| PerlError::runtime(format!("web_jobs_list: {}", e), line))?;
                let mut h = IndexMap::new();
                h.insert("id".to_string(), PerlValue::integer(id));
                h.insert("name".to_string(), PerlValue::string(name));
                h.insert("args_json".to_string(), PerlValue::string(args_json));
                h.insert("status".to_string(), PerlValue::string(status));
                h.insert("queue".to_string(), PerlValue::string(queue));
                h.insert("priority".to_string(), PerlValue::integer(priority));
                h.insert("attempts".to_string(), PerlValue::integer(attempts));
                h.insert("max_attempts".to_string(), PerlValue::integer(max_attempts));
                h.insert("created_at".to_string(), PerlValue::string(created_at));
                h.insert(
                    "locked_at".to_string(),
                    locked_at.map(PerlValue::string).unwrap_or(PerlValue::UNDEF),
                );
                h.insert(
                    "ran_at".to_string(),
                    ran_at.map(PerlValue::string).unwrap_or(PerlValue::UNDEF),
                );
                h.insert(
                    "error".to_string(),
                    error.map(PerlValue::string).unwrap_or(PerlValue::UNDEF),
                );
                out.push(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
            }
            Ok(out)
        },
        line,
    )?;
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        rows,
    ))))
}

pub(crate) fn web_jobs_stats(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let stats: IndexMap<String, i64> = with_db(
        |c| {
            let mut out = IndexMap::new();
            for status in &["pending", "running", "done", "failed"] {
                let n: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM jobs WHERE status = ?",
                        rusqlite::params![status],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);
                out.insert((*status).to_string(), n);
            }
            Ok(out)
        },
        line,
    )?;
    let mut h = IndexMap::new();
    for (k, v) in stats {
        h.insert(k, PerlValue::integer(v));
    }
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

pub(crate) fn web_job_purge(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let kv = parse_kv(args);
    let status = kv
        .get("status")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "done".to_string());
    let n: i64 = with_db(
        |c| {
            c.execute(
                "DELETE FROM jobs WHERE status = ?",
                rusqlite::params![&status],
            )
            .map(|n| n as i64)
            .map_err(|e| PerlError::runtime(format!("web_job_purge: {}", e), line))
        },
        line,
    )?;
    Ok(PerlValue::integer(n))
}
