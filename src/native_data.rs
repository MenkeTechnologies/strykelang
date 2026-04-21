//! Native CSV (`csv` crate), SQLite (`rusqlite`), and HTTP JSON (`ureq` + `serde_json`) helpers.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use indexmap::IndexMap;
use jaq_core::data::JustLut;
use num_traits::cast::ToPrimitive;
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
pub(crate) fn dataframe_from_elements(val: &PerlValue) -> PerlResult<PerlValue> {
    let rows = val.map_flatten_outputs(true);
    if rows.is_empty() {
        return Ok(PerlValue::dataframe(Arc::new(Mutex::new(PerlDataFrame {
            columns: vec![],
            cols: vec![],
            group_by: None,
        }))));
    }

    // Detect format: list of hashrefs or list of arrayrefs
    let first_row = &rows[0];
    if let Some(first_row_map) = first_row.as_hash_ref() {
        // List of hashrefs: use keys of the first row as columns
        let columns: Vec<String> = first_row_map.read().keys().cloned().collect();
        let mut cols: Vec<Vec<PerlValue>> = (0..columns.len()).map(|_| Vec::new()).collect();
        for row_val in rows {
            if let Some(row_lock) = row_val.as_hash_ref() {
                let row_map = row_lock.read();
                for (i, col_name) in columns.iter().enumerate() {
                    cols[i].push(row_map.get(col_name).cloned().unwrap_or(PerlValue::UNDEF));
                }
            }
        }
        return Ok(PerlValue::dataframe(Arc::new(Mutex::new(PerlDataFrame {
            columns,
            cols,
            group_by: None,
        }))));
    } else if let Some(first_row_lock) = first_row.as_array_ref() {
        // List of arrayrefs: first row is headers
        let first_row_arr = first_row_lock.read();
        let columns: Vec<String> = first_row_arr.iter().map(|v| v.to_string()).collect();
        let mut cols: Vec<Vec<PerlValue>> = (0..columns.len()).map(|_| Vec::new()).collect();
        for row_val in rows.iter().skip(1) {
            if let Some(row_lock) = row_val.as_array_ref() {
                let row_arr = row_lock.read();
                for (i, col) in cols.iter_mut().enumerate().take(columns.len()) {
                    col.push(row_arr.get(i).cloned().unwrap_or(PerlValue::UNDEF));
                }
            }
        }
        return Ok(PerlValue::dataframe(Arc::new(Mutex::new(PerlDataFrame {
            columns,
            cols,
            group_by: None,
        }))));
    }

    Err(PerlError::runtime(
        "dataframe expects a file path or a list of hashrefs/arrayrefs",
        0,
    ))
}

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
        for (i, col) in cols.iter_mut().enumerate().take(ncols) {
            let cell = record.get(i).unwrap_or("");
            col.push(PerlValue::string(cell.to_string()));
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
        for (i, col_name) in col_names.iter().enumerate().take(col_count) {
            let v = row
                .get::<_, Value>(i)
                .map_err(|e| PerlError::runtime(format!("sqlite query: {}", e), line))?;
            map.insert(col_name.clone(), sqlite_value_to_perl(v));
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

/// Build a struct instance with defaults evaluated by the interpreter.
/// Called from interpreter when constructing structs so default expressions can be evaluated.
pub(crate) fn struct_new_with_defaults(
    def: &Arc<StructDef>,
    provided: &[(String, PerlValue)],
    defaults: &[Option<PerlValue>],
    line: usize,
) -> PerlResult<PerlValue> {
    let mut values = vec![PerlValue::UNDEF; def.fields.len()];
    for (k, v) in provided {
        let idx = def.field_index(k).ok_or_else(|| {
            PerlError::runtime(format!("struct {}: unknown field `{}`", def.name, k), line)
        })?;
        let field = &def.fields[idx];
        field.ty.check_value(v).map_err(|msg| {
            PerlError::type_error(format!("struct {} field `{}`: {}", def.name, k, msg), line)
        })?;
        values[idx] = v.clone();
    }
    for (idx, field) in def.fields.iter().enumerate() {
        if values[idx].is_undef() {
            if let Some(dv) = defaults.get(idx).and_then(|o| o.as_ref()) {
                // Skip type check if default is undef (nullable field pattern)
                if !dv.is_undef() {
                    field.ty.check_value(dv).map_err(|msg| {
                        PerlError::type_error(
                            format!(
                                "struct {} field `{}` default: {}",
                                def.name, field.name, msg
                            ),
                            line,
                        )
                    })?;
                }
                values[idx] = dv.clone();
            } else if field.default.is_none() && !matches!(field.ty, crate::ast::PerlTypeName::Any)
            {
                return Err(PerlError::runtime(
                    format!(
                        "struct {}: missing field `{}` ({})",
                        def.name,
                        field.name,
                        field.ty.display_name()
                    ),
                    line,
                ));
            }
        }
    }
    Ok(PerlValue::struct_inst(Arc::new(StructInstance::new(
        Arc::clone(def),
        values,
    ))))
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

fn perl_hash_lookup(v: &PerlValue, key: &str) -> Option<PerlValue> {
    v.hash_get(key)
        .or_else(|| v.as_hash_ref().and_then(|r| r.read().get(key).cloned()))
}

fn perl_opt_lookup(opts: Option<&PerlValue>, key: &str) -> Option<PerlValue> {
    let o = opts?;
    perl_hash_lookup(o, key)
}

fn perl_opt_bool(opts: Option<&PerlValue>, key: &str) -> bool {
    perl_opt_lookup(opts, key).is_some_and(|v| v.is_true())
}

fn perl_opt_u64(opts: Option<&PerlValue>, key: &str) -> Option<u64> {
    perl_opt_lookup(opts, key).map(|v| v.to_int().max(0) as u64)
}

fn body_bytes_from_perl(v: &PerlValue) -> Vec<u8> {
    if let Some(b) = v.as_bytes_arc() {
        return b.as_ref().clone();
    }
    v.to_string().into_bytes()
}

fn headers_map_has_content_type(headers_val: &PerlValue) -> bool {
    if let Some(m) = headers_val.as_hash_map() {
        return m.keys().any(|k| k.eq_ignore_ascii_case("content-type"));
    }
    if let Some(r) = headers_val.as_hash_ref() {
        return r
            .read()
            .keys()
            .any(|k| k.eq_ignore_ascii_case("content-type"));
    }
    false
}

fn apply_request_headers(
    mut req: ureq::Request,
    headers_val: &PerlValue,
) -> PerlResult<ureq::Request> {
    let pairs: Vec<(String, String)> = if let Some(m) = headers_val.as_hash_map() {
        m.iter().map(|(k, v)| (k.clone(), v.to_string())).collect()
    } else if let Some(r) = headers_val.as_hash_ref() {
        r.read()
            .iter()
            .map(|(k, v)| (k.clone(), v.to_string()))
            .collect()
    } else {
        return Err(PerlError::runtime(
            "http_request: headers must be a hash or hashref",
            0,
        ));
    };
    for (k, v) in pairs {
        req = req.set(&k, &v);
    }
    Ok(req)
}

/// Full HTTP request: `opts` hash(ref) keys: `method` (default GET), `headers`, `body`, `json`
/// (encodes body, sets `Content-Type` unless already in `headers`), `timeout` / `timeout_secs`
/// (omit for 30s; `0` disables client timeout), `binary_response` (body as `BYTES` instead of decoded string).
///
/// Returns a hashref: `status`, `status_text`, `headers` (hashref, lowercased names), `body`.
pub(crate) fn http_request(url: &str, opts: Option<&PerlValue>) -> PerlResult<PerlValue> {
    let method = perl_opt_lookup(opts, "method")
        .map(|v| v.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "GET".to_string());
    let method_uc = method.to_ascii_uppercase();
    let timeout_secs = perl_opt_u64(opts, "timeout_secs").or_else(|| perl_opt_u64(opts, "timeout"));
    let binary_response = perl_opt_bool(opts, "binary_response");

    let mut req = ureq::request(method_uc.as_str(), url);
    match timeout_secs {
        None => {
            req = req.timeout(Duration::from_secs(30));
        }
        Some(0) => {}
        Some(n) => {
            req = req.timeout(Duration::from_secs(n));
        }
    }

    if let Some(hv) = opts.and_then(|o| perl_hash_lookup(o, "headers")) {
        req = apply_request_headers(req, &hv)?;
    }

    let mut body: Vec<u8> = Vec::new();
    if let Some(o) = opts {
        if let Some(jv) = perl_hash_lookup(o, "json") {
            let jstr = json_encode(&jv)?;
            if let Some(hv) = perl_hash_lookup(o, "headers") {
                if !headers_map_has_content_type(&hv) {
                    req = req.set("Content-Type", "application/json; charset=utf-8");
                }
            } else {
                req = req.set("Content-Type", "application/json; charset=utf-8");
            }
            body = jstr.into_bytes();
        } else if let Some(bv) = perl_hash_lookup(o, "body") {
            body = body_bytes_from_perl(&bv);
        }
    }

    let resp = if body.is_empty() {
        req.call()
    } else {
        req.send_bytes(&body)
    }
    .map_err(|e| PerlError::runtime(format!("http_request: {}", e), 0))?;

    let status = resp.status();
    let status_text = resp.status_text().to_string();
    let mut hdr_map = IndexMap::new();
    let mut names = resp.headers_names();
    names.sort();
    names.dedup();
    for n in names {
        let vals: Vec<&str> = resp.all(&n);
        if !vals.is_empty() {
            hdr_map.insert(n, PerlValue::string(vals.join(", ")));
        }
    }
    let headers_ref = PerlValue::hash_ref(Arc::new(RwLock::new(hdr_map)));

    let body_val = if binary_response {
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| PerlError::runtime(format!("http_request: body read: {}", e), 0))?;
        PerlValue::bytes(Arc::new(buf))
    } else {
        let s = resp
            .into_string()
            .map_err(|e| PerlError::runtime(format!("http_request: body: {}", e), 0))?;
        PerlValue::string(s)
    };

    let mut out = IndexMap::new();
    out.insert("status".into(), PerlValue::integer(status as i64));
    out.insert("status_text".into(), PerlValue::string(status_text));
    out.insert("headers".into(), headers_ref);
    out.insert("body".into(), body_val);
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(out))))
}

/// Parse JSON from the `body` field of an [`http_request`] result hashref.
pub(crate) fn http_response_json_body(res: &PerlValue) -> PerlResult<PerlValue> {
    let body = perl_hash_lookup(res, "body")
        .ok_or_else(|| PerlError::runtime("fetch_json: http response missing body", 0))?;
    let s = if let Some(b) = body.as_bytes_arc() {
        String::from_utf8_lossy(b.as_ref()).into_owned()
    } else {
        body.to_string()
    };
    json_decode(&s)
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

/// Run a [jq](https://jqlang.org/)-syntax filter (via [jaq](https://github.com/01mf02/jaq)) on JSON
/// derived from `data` (same encodable shapes as [`json_encode`]).
///
/// Returns `undef` if the filter yields no values, a single Perl value if it yields one output,
/// or an array of values if it yields more than one (e.g. `.items[]`).
pub(crate) fn json_jq(data: &PerlValue, filter_src: &str) -> PerlResult<PerlValue> {
    let j = perl_to_json_value(data)?;
    let input: jaq_json::Val = serde_json::from_value(j)
        .map_err(|e| PerlError::runtime(format!("json_jq: could not convert input: {}", e), 0))?;

    let arena = jaq_core::load::Arena::default();
    let defs = jaq_core::defs()
        .chain(jaq_std::defs())
        .chain(jaq_json::defs());
    let loader = jaq_core::load::Loader::new(defs);
    let file = jaq_core::load::File {
        code: filter_src,
        path: (),
    };
    let modules = loader
        .load(&arena, file)
        .map_err(|e| PerlError::runtime(format!("json_jq: parse/load: {:?}", e), 0))?;

    type JData = JustLut<jaq_json::Val>;
    let filter = jaq_core::Compiler::default()
        .with_funs(
            jaq_core::funs::<JData>()
                .chain(jaq_std::funs::<JData>())
                .chain(jaq_json::funs::<JData>()),
        )
        .compile(modules)
        .map_err(|e| PerlError::runtime(format!("json_jq: compile: {:?}", e), 0))?;

    let ctx = jaq_core::Ctx::<JData>::new(&filter.lut, jaq_core::Vars::new([]));
    let mut results = Vec::new();
    for x in filter.id.run((ctx, input)) {
        match jaq_core::unwrap_valr(x) {
            Ok(v) => results.push(jaq_json_val_to_perl(v)?),
            Err(e) => {
                return Err(PerlError::runtime(format!("json_jq: {}", e), 0));
            }
        }
    }

    match results.len() {
        0 => Ok(PerlValue::UNDEF),
        1 => Ok(results.pop().expect("one")),
        _ => Ok(PerlValue::array(results)),
    }
}

fn jaq_json_val_to_perl(v: jaq_json::Val) -> PerlResult<PerlValue> {
    use jaq_json::Val as Jv;
    match v {
        Jv::Null => Ok(PerlValue::UNDEF),
        Jv::Bool(b) => Ok(PerlValue::integer(i64::from(b))),
        Jv::Num(n) => jaq_num_to_perl(n),
        Jv::BStr(b) => Ok(PerlValue::string(String::from_utf8_lossy(&b).into_owned())),
        Jv::TStr(b) => Ok(PerlValue::string(String::from_utf8_lossy(&b).into_owned())),
        Jv::Arr(a) => {
            let v = a.as_ref();
            let mut out = Vec::with_capacity(v.len());
            for x in v.iter() {
                out.push(jaq_json_val_to_perl(x.clone())?);
            }
            Ok(PerlValue::array(out))
        }
        Jv::Obj(o) => {
            let mut map = IndexMap::new();
            for (k, val) in o.iter() {
                map.insert(k.to_string(), jaq_json_val_to_perl(val.clone())?);
            }
            Ok(PerlValue::hash_ref(Arc::new(RwLock::new(map))))
        }
    }
}

fn jaq_num_to_perl(n: jaq_json::Num) -> PerlResult<PerlValue> {
    use jaq_json::Num as Jn;
    match n {
        Jn::Int(i) => Ok(PerlValue::integer(i as i64)),
        Jn::Float(f) => Ok(PerlValue::float(f)),
        Jn::BigInt(r) => {
            let bi = (*r).clone();
            if let Some(i) = bi.to_i64() {
                Ok(PerlValue::integer(i))
            } else if let Some(f) = bi.to_f64() {
                Ok(PerlValue::float(f))
            } else {
                Ok(PerlValue::string(bi.to_string()))
            }
        }
        Jn::Dec(s) => {
            let f: f64 = s.parse().unwrap_or(f64::NAN);
            Ok(PerlValue::float(f))
        }
    }
}

pub(crate) fn perl_to_json_value(v: &PerlValue) -> PerlResult<JsonValue> {
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
        let values = si.get_values();
        for (i, field) in si.def.fields.iter().enumerate() {
            if let Some(fv) = values.get(i) {
                m.insert(field.name.clone(), perl_to_json_value(fv)?);
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

    #[test]
    fn json_jq_field_select() {
        let p = json_decode(r#"{"a":1,"b":{"c":3}}"#).unwrap();
        let out = json_jq(&p, ".b.c").unwrap();
        assert_eq!(out.to_int(), 3);
    }

    #[test]
    fn json_jq_map_select_multiple_yields_array() {
        let p = json_decode(r#"[1,2,3,4]"#).unwrap();
        let out = json_jq(&p, "map(select(. > 2))").unwrap();
        let a = out.as_array_vec().expect("array");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].to_int(), 3);
        assert_eq!(a[1].to_int(), 4);
    }

    #[test]
    fn test_dataframe_from_path() {
        let tmp = std::env::temp_dir().join(format!("test_df_{}.csv", std::process::id()));
        let csv_data = "id,name,val\n1,alice,10.5\n2,bob,20.0\n";
        std::fs::write(&tmp, csv_data).expect("write csv");

        let df_val = dataframe_from_path(tmp.to_str().unwrap()).expect("dataframe_from_path");
        let df_lock = df_val.as_dataframe().expect("as_dataframe");
        let df = df_lock.lock();

        assert_eq!(df.columns, vec!["id", "name", "val"]);
        assert_eq!(df.cols.len(), 3);
        assert_eq!(df.cols[0][0].to_string(), "1");
        assert_eq!(df.cols[1][1].to_string(), "bob");
        assert_eq!(df.cols[2][0].to_string(), "10.5");

        let _ = std::fs::remove_file(&tmp);
    }
}
