//! Native CSV (`csv` crate), SQLite (`rusqlite`), and HTTP JSON (`ureq` + `serde_json`) helpers.

use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::{Mutex, RwLock};
use rayon::prelude::*;
use rusqlite::{types::Value, Connection};
use serde_json::Value as JsonValue;

use crate::ast::StructDef;
use crate::error::{PerlError, PerlResult};
use crate::value::{HeapObject, PerlDataFrame, PerlValue, StructInstance};

/// Parallel row→hashref conversion after a sequential CSV parse (good CPU parallelism on wide files).
pub(crate) fn par_csv_read(path: &str) -> PerlResult<PerlValue> {
    let mut rdr = csv::Reader::from_path(path)
        .map_err(|e| PerlError::runtime(format!("par_csv_read: {}: {}", path, e), 0))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| PerlError::runtime(format!("par_csv_read: {}: {}", path, e), 0))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut raw_rows: Vec<csv::StringRecord> = Vec::new();
    for rec in rdr.records() {
        raw_rows.push(rec.map_err(|e| PerlError::runtime(format!("par_csv_read: {}", e), 0))?);
    }
    let rows: Vec<PerlValue> = raw_rows
        .into_par_iter()
        .map(|record| {
            let mut map = IndexMap::new();
            for (i, h) in headers.iter().enumerate() {
                let cell = record.get(i).unwrap_or("");
                map.insert(h.clone(), PerlValue::string(cell.to_string()));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        })
        .collect();
    Ok(PerlValue::array(rows))
}

/// Columnar dataframe from a CSV path (header row + string cells; use `sum` etc. with numeric strings).
pub(crate) fn dataframe_from_path(path: &str) -> PerlResult<PerlValue> {
    let mut rdr = csv::Reader::from_path(path)
        .map_err(|e| PerlError::runtime(format!("dataframe: {}: {}", path, e), 0))?;
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| PerlError::runtime(format!("dataframe: {}: {}", path, e), 0))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let ncols = headers.len();
    let mut cols: Vec<Vec<PerlValue>> = (0..ncols).map(|_| Vec::new()).collect();
    for rec in rdr.records() {
        let record = rec.map_err(|e| PerlError::runtime(format!("dataframe: {}", e), 0))?;
        for i in 0..ncols {
            let cell = record.get(i).unwrap_or("");
            cols[i].push(PerlValue::string(cell.to_string()));
        }
    }
    let df = PerlDataFrame {
        columns: headers,
        cols,
        group_by: None,
    };
    Ok(PerlValue::dataframe(Arc::new(Mutex::new(df))))
}

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
            map.insert(h.clone(), PerlValue::string(cell.to_string()));
        }
        rows.push(PerlValue::hash_ref(Arc::new(RwLock::new(map))));
    }
    Ok(PerlValue::array(rows))
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
    Ok(PerlValue::integer(normalized.len() as i64))
}

fn hash_like(v: &PerlValue) -> PerlResult<IndexMap<String, PerlValue>> {
    if let Some(h) = v.as_hash_map() {
        return Ok(h);
    }
    if let Some(r) = v.as_hash_ref() {
        return Ok(r.read().clone());
    }
    if let Some(b) = v.as_blessed_ref() {
        let d = b.data.read();
        if let Some(h) = d.as_hash_map() {
            return Ok(h);
        }
    }
    Err(PerlError::runtime(
        "csv_write: row must be hash or hashref",
        0,
    ))
}

pub(crate) fn sqlite_open(path: &str) -> PerlResult<PerlValue> {
    let conn = Connection::open(path)
        .map_err(|e| PerlError::runtime(format!("sqlite: {}: {}", path, e), 0))?;
    Ok(PerlValue::sqlite_conn(Arc::new(Mutex::new(conn))))
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
            let n = exec_sql(&c, &sql, &params)?;
            Ok(PerlValue::integer(n as i64))
        }
        "query" => {
            if args.is_empty() {
                return Err(PerlError::runtime("sqlite->query needs SQL string", line));
            }
            let sql = args[0].to_string();
            let params: Vec<Value> = args[1..].iter().map(perl_to_sql_value).collect();
            query_sql(&c, &sql, &params, line)
        }
        "last_insert_rowid" => {
            if !args.is_empty() {
                return Err(PerlError::runtime(
                    "sqlite->last_insert_rowid takes no arguments",
                    line,
                ));
            }
            Ok(PerlValue::integer(c.last_insert_rowid()))
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

fn query_sql(conn: &Connection, sql: &str, params: &[Value], line: usize) -> PerlResult<PerlValue> {
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
        rows_out.push(PerlValue::hash_ref(Arc::new(RwLock::new(map))));
    }
    Ok(PerlValue::array(rows_out))
}

fn perl_to_sql_value(v: &PerlValue) -> Value {
    if v.is_undef() {
        return Value::Null;
    }
    if let Some(i) = v.as_integer() {
        return Value::Integer(i);
    }
    if let Some(f) = v.as_float() {
        return Value::Real(f);
    }
    if let Some(s) = v.as_str() {
        return Value::Text(s);
    }
    if let Some(b) = v.as_bytes_arc() {
        return Value::Blob((*b).clone());
    }
    Value::Text(v.to_string())
}

fn sqlite_value_to_perl(v: Value) -> PerlValue {
    match v {
        Value::Null => PerlValue::UNDEF,
        Value::Integer(i) => PerlValue::integer(i),
        Value::Real(r) => PerlValue::float(r),
        Value::Text(s) => PerlValue::string(s),
        Value::Blob(b) => PerlValue::bytes(Arc::new(b)),
    }
}

/// Build a struct instance from `Class->new(k => v, ...)` arguments (pairs after class name).
pub(crate) fn struct_new(
    def: &Arc<StructDef>,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let mut values = vec![PerlValue::UNDEF; def.fields.len()];
    let mut i = 1;
    while i + 1 < args.len() {
        let k = args[i].to_string();
        let v = args[i + 1].clone();
        let idx = def.field_index(&k).ok_or_else(|| {
            PerlError::runtime(format!("struct {}: unknown field `{}`", def.name, k), line)
        })?;
        let ty = def.fields[idx].1;
        ty.check_value(&v).map_err(|msg| {
            PerlError::type_error(format!("struct {} field `{}`: {}", def.name, k, msg), line)
        })?;
        values[idx] = v;
        i += 2;
    }
    for ((name, ty), val) in def.fields.iter().zip(values.iter()) {
        if val.is_undef() {
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
    Ok(PerlValue::struct_inst(Arc::new(StructInstance {
        def: Arc::clone(def),
        values,
    })))
}

/// GET `url` and return the response body as a UTF-8 string (invalid UTF-8 is lossy).
pub(crate) fn fetch(url: &str) -> PerlResult<PerlValue> {
    let s = http_get_body(url)?;
    Ok(PerlValue::string(s))
}

/// GET `url`, parse JSON, map to [`PerlValue`] (objects → `HashRef`, arrays → `Array`, etc.).
pub(crate) fn fetch_json(url: &str) -> PerlResult<PerlValue> {
    let s = http_get_body(url)?;
    let v: JsonValue = serde_json::from_str(&s)
        .map_err(|e| PerlError::runtime(format!("fetch_json: {}", e), 0))?;
    Ok(json_to_perl(v))
}

fn http_get_body(url: &str) -> PerlResult<String> {
    ureq::get(url)
        .call()
        .map_err(|e| PerlError::runtime(format!("fetch: {}", e), 0))?
        .into_string()
        .map_err(|e| PerlError::runtime(format!("fetch: {}", e), 0))
}

/// Serialize a [`PerlValue`] to a JSON string (arrays, hashes, refs, structs, scalars; not code/refs/IO).
pub(crate) fn json_encode(v: &PerlValue) -> PerlResult<String> {
    let j = perl_to_json_value(v)?;
    serde_json::to_string(&j).map_err(|e| PerlError::runtime(format!("json_encode: {}", e), 0))
}

/// Parse a JSON string into [`PerlValue`] (same mapping as [`fetch_json`]).
pub(crate) fn json_decode(s: &str) -> PerlResult<PerlValue> {
    let v: JsonValue = serde_json::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("json_decode: {}", e), 0))?;
    Ok(json_to_perl(v))
}

fn perl_to_json_value(v: &PerlValue) -> PerlResult<JsonValue> {
    if v.is_undef() {
        return Ok(JsonValue::Null);
    }
    if let Some(n) = v.as_integer() {
        return Ok(JsonValue::Number(n.into()));
    }
    if let Some(f) = v.as_float() {
        return serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .ok_or_else(|| PerlError::runtime("json_encode: non-finite float", 0));
    }
    if crate::nanbox::is_raw_float_bits(v.0) {
        let f = f64::from_bits(v.0);
        return serde_json::Number::from_f64(f)
            .map(JsonValue::Number)
            .ok_or_else(|| PerlError::runtime("json_encode: non-finite float", 0));
    }
    if let Some(a) = v.as_array_vec() {
        let mut out = Vec::with_capacity(a.len());
        for x in &a {
            out.push(perl_to_json_value(x)?);
        }
        return Ok(JsonValue::Array(out));
    }
    if let Some(h) = v.as_hash_map() {
        let mut m = serde_json::Map::new();
        for (k, val) in h.iter() {
            m.insert(k.clone(), perl_to_json_value(val)?);
        }
        return Ok(JsonValue::Object(m));
    }
    if let Some(r) = v.as_array_ref() {
        let g = r.read();
        let mut out = Vec::with_capacity(g.len());
        for x in g.iter() {
            out.push(perl_to_json_value(x)?);
        }
        return Ok(JsonValue::Array(out));
    }
    if let Some(r) = v.as_hash_ref() {
        let g = r.read();
        let mut m = serde_json::Map::new();
        for (k, val) in g.iter() {
            m.insert(k.clone(), perl_to_json_value(val)?);
        }
        return Ok(JsonValue::Object(m));
    }
    if let Some(r) = v.as_scalar_ref() {
        return perl_to_json_value(&r.read());
    }
    if let Some(a) = v.as_atomic_arc() {
        return perl_to_json_value(&a.lock().clone());
    }
    if let Some(s) = v.as_str() {
        return Ok(JsonValue::String(s));
    }
    if let Some(b) = v.as_bytes_arc() {
        return Ok(JsonValue::String(String::from_utf8_lossy(&b).into_owned()));
    }
    if let Some(si) = v.as_struct_inst() {
        let mut m = serde_json::Map::new();
        for (i, (fname, _)) in si.def.fields.iter().enumerate() {
            if let Some(fv) = si.values.get(i) {
                m.insert(fname.clone(), perl_to_json_value(fv)?);
            }
        }
        return Ok(JsonValue::Object(m));
    }
    if let Some(b) = v.as_blessed_ref() {
        let inner = b.data.read().clone();
        return perl_to_json_value(&inner);
    }
    if let Some(vals) = v
        .with_heap(|h| match h {
            HeapObject::Set(s) => Some(s.values().cloned().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
    {
        let mut out = Vec::with_capacity(vals.len());
        for x in vals {
            out.push(perl_to_json_value(&x)?);
        }
        return Ok(JsonValue::Array(out));
    }
    if let Some(vals) = v
        .with_heap(|h| match h {
            HeapObject::Deque(d) => Some(d.lock().iter().cloned().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
    {
        let mut out = Vec::with_capacity(vals.len());
        for x in vals {
            out.push(perl_to_json_value(&x)?);
        }
        return Ok(JsonValue::Array(out));
    }

    if let Some(df) = v.as_dataframe() {
        let g = df.lock();
        let n = g.nrows();
        let mut rows = Vec::with_capacity(n);
        for r in 0..n {
            let mut m = serde_json::Map::new();
            for (i, col) in g.columns.iter().enumerate() {
                m.insert(col.clone(), perl_to_json_value(&g.cols[i][r])?);
            }
            rows.push(JsonValue::Object(m));
        }
        return Ok(JsonValue::Array(rows));
    }

    Err(PerlError::runtime(
        format!(
            "json_encode: value cannot be encoded as JSON ({})",
            v.type_name()
        ),
        0,
    ))
}

fn json_to_perl(v: JsonValue) -> PerlValue {
    match v {
        JsonValue::Null => PerlValue::UNDEF,
        JsonValue::Bool(b) => PerlValue::integer(i64::from(b)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(u) = n.as_u64() {
                PerlValue::integer(u as i64)
            } else {
                PerlValue::float(n.as_f64().unwrap_or(0.0))
            }
        }
        JsonValue::String(s) => PerlValue::string(s),
        JsonValue::Array(a) => PerlValue::array(a.into_iter().map(json_to_perl).collect()),
        JsonValue::Object(o) => {
            let mut map = IndexMap::new();
            for (k, v) in o {
                map.insert(k, json_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        }
    }
}

#[cfg(test)]
mod http_json_tests {
    use super::*;

    #[test]
    fn json_to_perl_object_hashref() {
        let v: JsonValue = serde_json::from_str(r#"{"name":"a","n":1}"#).unwrap();
        let p = json_to_perl(v);
        let r = p.as_hash_ref().expect("expected HashRef");
        let g = r.read();
        assert_eq!(g.get("name").unwrap().to_string(), "a");
        assert_eq!(g.get("n").unwrap().to_int(), 1);
    }

    #[test]
    fn json_to_perl_array() {
        let v: JsonValue = serde_json::from_str(r#"[1,"x",null]"#).unwrap();
        let p = json_to_perl(v);
        let a = p.as_array_vec().expect("expected Array");
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].to_int(), 1);
        assert_eq!(a[1].to_string(), "x");
        assert!(a[2].is_undef());
    }

    #[test]
    fn json_encode_decode_roundtrip() {
        let p = PerlValue::array(vec![
            PerlValue::integer(1),
            PerlValue::string("x".into()),
            PerlValue::UNDEF,
        ]);
        let s = json_encode(&p).expect("encode");
        let back = json_decode(&s).expect("decode");
        let a = back.as_array_vec().expect("array");
        assert_eq!(a.len(), 3);
        assert_eq!(a[0].to_int(), 1);
        assert_eq!(a[1].to_string(), "x");
        assert!(a[2].is_undef());
    }

    #[test]
    fn json_encode_hash_roundtrip() {
        let mut m = IndexMap::new();
        m.insert("a".into(), PerlValue::integer(2));
        let p = PerlValue::hash(m);
        let s = json_encode(&p).expect("encode");
        assert!(s.contains("\"a\""));
        let back = json_decode(&s).expect("decode");
        let h = back.as_hash_ref().expect("hashref");
        assert_eq!(h.read().get("a").unwrap().to_int(), 2);
    }
}
