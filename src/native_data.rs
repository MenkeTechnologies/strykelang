//! Native CSV (`csv` crate) and SQLite (`rusqlite`) helpers.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use rusqlite::{types::Value, Connection};

use crate::ast::StructDef;
use crate::error::{PerlError, PerlResult};
use crate::value::{PerlValue, StructInstance};

pub(crate) fn csv_read(path: &str) -> PerlResult<PerlValue> {
    let mut rdr = csv::Reader::from_path(path)
        .map_err(|e| PerlError::runtime(format!("csv_read: {}: {}", path, e), 0))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| PerlError::runtime(format!("csv_read: {}: {}", path, e), 0))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let record = rec.map_err(|e| PerlError::runtime(format!("csv_read: {}", e), 0))?;
        let mut map = IndexMap::new();
        for (i, h) in headers.iter().enumerate() {
            let cell = record.get(i).unwrap_or("");
            map.insert(h.clone(), PerlValue::String(cell.to_string()));
        }
        rows.push(PerlValue::HashRef(Arc::new(RwLock::new(map))));
    }
    Ok(PerlValue::Array(rows))
}

/// Writes rows as CSV. Each row is a hash or hashref; header row is the union of keys
/// (first-seen order, then keys from later rows in order).
pub(crate) fn csv_write(path: &str, rows: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut header: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();
    let mut normalized: Vec<IndexMap<String, PerlValue>> = Vec::new();

    for row in rows {
        let map = hash_like(row)?;
        for k in map.keys() {
            if seen.insert(k.clone()) {
                header.push(k.clone());
            }
        }
        normalized.push(map);
    }

    let mut wtr = csv::Writer::from_path(path)
        .map_err(|e| PerlError::runtime(format!("csv_write: {}: {}", path, e), 0))?;
    wtr.write_record(&header)
        .map_err(|e| PerlError::runtime(format!("csv_write: {}", e), 0))?;
    for map in &normalized {
        let record: Vec<String> = header
            .iter()
            .map(|k| map.get(k).map(|v| v.to_string()).unwrap_or_default())
            .collect();
        wtr.write_record(&record)
            .map_err(|e| PerlError::runtime(format!("csv_write: {}", e), 0))?;
    }
    wtr.flush()
        .map_err(|e| PerlError::runtime(format!("csv_write: {}", e), 0))?;
    Ok(PerlValue::Integer(normalized.len() as i64))
}

fn hash_like(v: &PerlValue) -> PerlResult<IndexMap<String, PerlValue>> {
    match v {
        PerlValue::Hash(h) => Ok(h.clone()),
        PerlValue::HashRef(r) => Ok(r.read().clone()),
        PerlValue::Blessed(b) => match &*b.data.read() {
            PerlValue::Hash(h) => Ok(h.clone()),
            _ => Err(PerlError::runtime("csv_write: row must be hash or hashref", 0)),
        },
        _ => Err(PerlError::runtime(
            "csv_write: row must be hash or hashref",
            0,
        )),
    }
}

pub(crate) fn sqlite_open(path: &str) -> PerlResult<PerlValue> {
    let conn = Connection::open(path)
        .map_err(|e| PerlError::runtime(format!("sqlite: {}: {}", path, e), 0))?;
    Ok(PerlValue::SqliteConn(Arc::new(Mutex::new(conn))))
}

pub(crate) fn sqlite_dispatch(
    conn: &Arc<Mutex<Connection>>,
    method: &str,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let c = conn.lock();
    match method {
        "exec" => {
            if args.is_empty() {
                return Err(PerlError::runtime("sqlite->exec needs SQL string", line));
            }
            let sql = args[0].to_string();
            let params: Vec<Value> = args[1..].iter().map(perl_to_sql_value).collect();
            let n = exec_sql(&*c, &sql, &params)?;
            Ok(PerlValue::Integer(n as i64))
        }
        "query" => {
            if args.is_empty() {
                return Err(PerlError::runtime("sqlite->query needs SQL string", line));
            }
            let sql = args[0].to_string();
            let params: Vec<Value> = args[1..].iter().map(perl_to_sql_value).collect();
            query_sql(&*c, &sql, &params, line)
        }
        "last_insert_rowid" => {
            if !args.is_empty() {
                return Err(PerlError::runtime(
                    "sqlite->last_insert_rowid takes no arguments",
                    line,
                ));
            }
            Ok(PerlValue::Integer(c.last_insert_rowid()))
        }
        _ => Err(PerlError::runtime(
            format!("unknown sqlite method: {}", method),
            line,
        )),
    }
}

fn exec_sql(conn: &Connection, sql: &str, params: &[Value]) -> PerlResult<usize> {
    conn.execute(sql, rusqlite::params_from_iter(params.iter()))
        .map_err(|e| PerlError::runtime(format!("sqlite exec: {}", e), 0))
}

fn query_sql(
    conn: &Connection,
    sql: &str,
    params: &[Value],
    line: usize,
) -> PerlResult<PerlValue> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| PerlError::runtime(format!("sqlite query: {}", e), line))?;
    let col_count = stmt.column_count();
    let mut col_names = Vec::with_capacity(col_count);
    for i in 0..col_count {
        col_names.push(
            stmt.column_name(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| format!("col{}", i)),
        );
    }
    let mut rows = stmt
        .query(rusqlite::params_from_iter(params.iter()))
        .map_err(|e| PerlError::runtime(format!("sqlite query: {}", e), line))?;
    let mut rows_out = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| PerlError::runtime(format!("sqlite query: {}", e), line))?
    {
        let mut map = IndexMap::new();
        for i in 0..col_count {
            let v = row
                .get::<_, Value>(i)
                .map_err(|e| PerlError::runtime(format!("sqlite query: {}", e), line))?;
            map.insert(col_names[i].clone(), sqlite_value_to_perl(v));
        }
        rows_out.push(PerlValue::HashRef(Arc::new(RwLock::new(map))));
    }
    Ok(PerlValue::Array(rows_out))
}

fn perl_to_sql_value(v: &PerlValue) -> Value {
    match v {
        PerlValue::Undef => Value::Null,
        PerlValue::Integer(i) => Value::Integer(*i),
        PerlValue::Float(f) => Value::Real(*f),
        PerlValue::String(s) => Value::Text(s.clone()),
        PerlValue::Bytes(b) => Value::Blob((**b).clone()),
        _ => Value::Text(v.to_string()),
    }
}

fn sqlite_value_to_perl(v: Value) -> PerlValue {
    match v {
        Value::Null => PerlValue::Undef,
        Value::Integer(i) => PerlValue::Integer(i),
        Value::Real(r) => PerlValue::Float(r),
        Value::Text(s) => PerlValue::String(s),
        Value::Blob(b) => PerlValue::Bytes(Arc::new(b)),
    }
}

/// Build a struct instance from `Class->new(k => v, ...)` arguments (pairs after class name).
pub(crate) fn struct_new(
    def: &Arc<StructDef>,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let mut values = vec![PerlValue::Undef; def.fields.len()];
    let mut i = 1;
    while i + 1 < args.len() {
        let k = args[i].to_string();
        let v = args[i + 1].clone();
        let idx = def.field_index(&k).ok_or_else(|| {
            PerlError::runtime(
                format!("struct {}: unknown field `{}`", def.name, k),
                line,
            )
        })?;
        let ty = def.fields[idx].1;
        ty.check_value(&v).map_err(|msg| {
            PerlError::type_error(format!("struct {} field `{}`: {}", def.name, k, msg), line)
        })?;
        values[idx] = v;
        i += 2;
    }
    for ((name, ty), val) in def.fields.iter().zip(values.iter()) {
        if matches!(val, PerlValue::Undef) {
            return Err(PerlError::runtime(
                format!(
                    "struct {}: missing field `{}` ({})",
                    def.name,
                    name,
                    match ty {
                        crate::ast::PerlTypeName::Int => "Int",
                        crate::ast::PerlTypeName::Str => "Str",
                        crate::ast::PerlTypeName::Float => "Float",
                    }
                ),
                line,
            ));
        }
    }
    Ok(PerlValue::StructInst(Arc::new(StructInstance {
        def: Arc::clone(def),
        values,
    })))
}
