//! Cryptographic digests, compression, config decoders, and datetime helpers (UTC epoch + IANA zones via `chrono-tz`).

use std::io::{Read, Write};

use base64::Engine;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use hmac::{Hmac, Mac};
use indexmap::IndexMap;
use md5::{Digest as Md5Digest, Md5};
use parking_lot::RwLock;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;
use std::sync::Arc;

use crate::error::{PerlError, PerlResult};
use crate::value::PerlValue;

type HmacSha256 = Hmac<Sha256>;

fn bytes_from_value(v: &PerlValue) -> Vec<u8> {
    if let Some(b) = v.as_bytes_arc() {
        return b.as_ref().clone();
    }
    v.to_string().into_bytes()
}

/// SHA-256 digest of the argument as UTF-8 bytes; returns lowercase hex (64 chars).
pub(crate) fn sha256(v: &PerlValue) -> PerlResult<PerlValue> {
    let d = Sha256::digest(bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(d)))
}

/// MD5 digest; lowercase hex (32 chars).
pub(crate) fn md5_digest(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Md5::new();
    Md5Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// SHA-1 digest; lowercase hex (40 chars).
pub(crate) fn sha1_digest(v: &PerlValue) -> PerlResult<PerlValue> {
    let mut h = Sha1::new();
    Sha1Digest::update(&mut h, bytes_from_value(v));
    Ok(PerlValue::string(hex::encode(h.finalize())))
}

/// HMAC-SHA256(key, message); both taken as bytes from string values; returns lowercase hex.
pub(crate) fn hmac_sha256(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = HmacSha256::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    mac.update(&bytes_from_value(msg));
    let out = mac.finalize().into_bytes();
    Ok(PerlValue::string(hex::encode(out)))
}

/// Raw HMAC-SHA256 bytes (for JWT and other binary signatures).
pub(crate) fn hmac_sha256_raw(key: &PerlValue, msg: &PerlValue) -> PerlResult<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    mac.update(&bytes_from_value(msg));
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Constant-time HMAC verification (rejects forged or truncated tags).
pub(crate) fn hmac_sha256_verify_raw(
    key: &PerlValue,
    msg: &PerlValue,
    tag: &[u8],
) -> PerlResult<()> {
    let mut mac = HmacSha256::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    mac.update(&bytes_from_value(msg));
    mac.verify_slice(tag)
        .map_err(|_| PerlError::runtime("HMAC verification failed", 0))
}

/// JWT / RFC 4648 URL-safe base64 **without** padding.
pub(crate) fn base64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Decode URL-safe base64 (adds `=` padding as needed).
pub(crate) fn base64url_decode(s: &str) -> Result<Vec<u8>, PerlError> {
    let s = s.trim().replace(' ', "");
    let pad = (4 - (s.len() % 4)) % 4;
    let mut padded = s;
    padded.push_str(&"=".repeat(pad));
    base64::engine::general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .map_err(|e| PerlError::runtime(format!("base64url_decode: {}", e), 0))
}

/// Random UUID (v4) as hyphenated lowercase string.
pub(crate) fn uuid_v4() -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        uuid::Uuid::new_v4().hyphenated().to_string(),
    ))
}

pub(crate) fn base64_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    Ok(PerlValue::string(
        base64::engine::general_purpose::STANDARD.encode(&b),
    ))
}

pub(crate) fn base64_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| PerlError::runtime(format!("base64_decode: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&raw).into_owned(),
    ))
}

/// Bytes to lowercase hex (two hex digits per byte).
pub(crate) fn hex_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(hex::encode(bytes_from_value(v))))
}

/// Hex string (even length) to a Perl string (may be non-UTF-8; uses lossy UTF-8 like other byte paths).
pub(crate) fn hex_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let raw =
        hex::decode(s.trim()).map_err(|e| PerlError::runtime(format!("hex_decode: {}", e), 0))?;
    Ok(PerlValue::string(
        String::from_utf8_lossy(&raw).into_owned(),
    ))
}

pub(crate) fn gzip(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&b)
        .map_err(|e| PerlError::runtime(format!("gzip: {}", e), 0))?;
    let out = enc
        .finish()
        .map_err(|e| PerlError::runtime(format!("gzip: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn gunzip(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let mut dec = GzDecoder::new(&b[..]);
    let mut out = Vec::new();
    dec.read_to_end(&mut out)
        .map_err(|e| PerlError::runtime(format!("gunzip: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn zstd_compress(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let out =
        zstd::encode_all(&b[..], 3).map_err(|e| PerlError::runtime(format!("zstd: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

pub(crate) fn zstd_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let b = bytes_from_value(v);
    let out = zstd::decode_all(&b[..])
        .map_err(|e| PerlError::runtime(format!("zstd_decode: {}", e), 0))?;
    Ok(PerlValue::bytes(Arc::new(out)))
}

// ── DateTime (UTC / epoch; uses chrono, no heap object type) ──

/// Current time as RFC 3339 UTC (e.g. `2024-01-02T15:04:05.123456789Z`).
pub(crate) fn datetime_utc() -> PerlResult<PerlValue> {
    let t = Utc::now();
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

/// Unix epoch seconds (float) → RFC 3339 UTC string. Fractional seconds preserved when representable.
pub(crate) fn datetime_from_epoch(v: &PerlValue) -> PerlResult<PerlValue> {
    let sec = v.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime(
            "datetime_from_epoch: non-finite value",
            0,
        ));
    }
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_from_epoch: out of range", 0))?;
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

fn fraction_nanos(sec: f64) -> u32 {
    let frac = sec - sec.floor();
    let n = (frac * 1_000_000_000.0).round() as i64;
    n.clamp(0, 999_999_999) as u32
}

/// Parse RFC 3339 / ISO-8601 datetime → Unix seconds as float (UTC).
pub(crate) fn datetime_parse_rfc3339(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    let t = DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| s.trim().parse::<DateTime<Utc>>())
        .map_err(|e| PerlError::runtime(format!("datetime_parse_rfc3339: {}", e), 0))?;
    let secs = t.timestamp() as f64 + f64::from(t.timestamp_subsec_nanos()) / 1e9;
    Ok(PerlValue::float(secs))
}

/// `strftime` formatting for UTC epoch seconds. `fmt` uses chrono's `strftime` specifiers.
pub(crate) fn datetime_strftime(epoch: &PerlValue, fmt: &PerlValue) -> PerlResult<PerlValue> {
    let sec = epoch.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime("datetime_strftime: non-finite epoch", 0));
    }
    let pattern = fmt.to_string();
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_strftime: out of range", 0))?;
    let out = t.format(&pattern).to_string();
    Ok(PerlValue::string(out))
}

/// Current time in an IANA timezone (e.g. `America/New_York`) as RFC 3339 with offset.
pub(crate) fn datetime_now_tz(tz_name: &PerlValue) -> PerlResult<PerlValue> {
    let tz: Tz = parse_tz(tz_name)?;
    let t = Utc::now().with_timezone(&tz);
    Ok(PerlValue::string(
        t.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
    ))
}

fn parse_tz(tz_name: &PerlValue) -> Result<Tz, PerlError> {
    tz_name
        .to_string()
        .trim()
        .parse()
        .map_err(|_| PerlError::runtime(format!("unknown timezone {:?}", tz_name.to_string()), 0))
}

/// Unix epoch seconds (UTC float) formatted with [`chrono::format::strftime`] in an IANA timezone.
pub(crate) fn datetime_format_tz(
    epoch: &PerlValue,
    tz_name: &PerlValue,
    fmt: &PerlValue,
) -> PerlResult<PerlValue> {
    let sec = epoch.to_number();
    if !sec.is_finite() {
        return Err(PerlError::runtime(
            "datetime_format_tz: non-finite epoch",
            0,
        ));
    }
    let tz: Tz = parse_tz(tz_name)?;
    let pattern = fmt.to_string();
    let t = Utc
        .timestamp_opt(sec.floor() as i64, fraction_nanos(sec))
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_format_tz: out of range", 0))?;
    let local = t.with_timezone(&tz);
    Ok(PerlValue::string(local.format(&pattern).to_string()))
}

/// Wall-clock / naive datetime string interpreted in an IANA timezone → UTC epoch seconds (float).
/// Accepts `%Y-%m-%d %H:%M:%S`, `%Y-%m-%dT%H:%M:%S`, or `%Y-%m-%d` (midnight).
pub(crate) fn datetime_parse_local(s: &PerlValue, tz_name: &PerlValue) -> PerlResult<PerlValue> {
    let tz: Tz = parse_tz(tz_name)?;
    let text = s.to_string();
    let naive = parse_naive_datetime(text.trim()).ok_or_else(|| {
        PerlError::runtime(
            "datetime_parse_local: expected YYYY-MM-DD [HH:MM:SS] or YYYY-MM-DDTHH:MM:SS",
            0,
        )
    })?;
    let mapped = tz
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| PerlError::runtime("datetime_parse_local: invalid local time", 0))?;
    let utc = mapped.with_timezone(&Utc);
    let secs = utc.timestamp() as f64 + f64::from(utc.timestamp_subsec_nanos()) / 1e9;
    Ok(PerlValue::float(secs))
}

fn parse_naive_datetime(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
        .or_else(|| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .and_then(|d| d.and_hms_opt(0, 0, 0))
        })
}

/// Epoch arithmetic: `datetime_add_seconds($epoch, $delta)` — both floats; result is float UTC seconds.
pub(crate) fn datetime_add_seconds(epoch: &PerlValue, secs: &PerlValue) -> PerlResult<PerlValue> {
    let a = epoch.to_number();
    let b = secs.to_number();
    if !a.is_finite() || !b.is_finite() {
        return Err(PerlError::runtime(
            "datetime_add_seconds: non-finite values",
            0,
        ));
    }
    Ok(PerlValue::float(a + b))
}

// ── XML (subset: elements, attributes, text, repeated child tags) ──

/// Decode XML to a single-root hashref: `{ root_tag => { ... } }`.
/// Attributes become keys `@name` (match XML local names). Text content uses `#text`. Repeated child
/// elements become an array.
///
/// When building hashes in Perl for [`xml_encode`], use **single-quoted** keys for attributes (e.g.
/// `'@id'`) so `@` is not interpolated inside double quotes.
pub(crate) fn xml_decode(s: &str) -> PerlResult<PerlValue> {
    let doc = roxmltree::Document::parse(s.trim())
        .map_err(|e| PerlError::runtime(format!("xml_decode: {}", e), 0))?;
    let root = doc.root_element();
    let name = root.tag_name().name().to_string();
    let inner = xml_element_to_perl(root)?;
    let mut m = IndexMap::new();
    m.insert(name, inner);
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(m))))
}

fn xml_element_to_perl(node: roxmltree::Node<'_, '_>) -> PerlResult<PerlValue> {
    let mut map = IndexMap::new();
    for a in node.attributes() {
        let an = a.name();
        map.insert(format!("@{an}"), PerlValue::string(a.value().to_string()));
    }
    let mut text_buf = String::new();
    for c in node.children() {
        if c.is_text() {
            text_buf.push_str(c.text().unwrap_or(""));
        }
    }
    let t = text_buf.trim();
    if !t.is_empty() {
        map.insert("#text".into(), PerlValue::string(t.to_string()));
    }
    let mut groups: IndexMap<String, Vec<PerlValue>> = IndexMap::new();
    for c in node.children() {
        if c.is_element() {
            let tag = c.tag_name().name().to_string();
            groups.entry(tag).or_default().push(xml_element_to_perl(c)?);
        }
    }
    for (tag, mut vals) in groups {
        let v = if vals.len() == 1 {
            vals.pop().expect("one")
        } else {
            PerlValue::array(vals)
        };
        map.insert(tag, v);
    }
    Ok(PerlValue::hash_ref(Arc::new(RwLock::new(map))))
}

fn is_valid_xml_element_name(name: &str) -> bool {
    let mut ch = name.chars();
    let Some(first) = ch.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
        return false;
    }
    ch.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn escape_xml_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn escape_xml_attr_value(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

fn xml_write_element(out: &mut String, tag: &str, v: &PerlValue) -> PerlResult<()> {
    if !is_valid_xml_element_name(tag) {
        return Err(PerlError::runtime(
            format!("xml_encode: invalid element name `{tag}`"),
            0,
        ));
    }
    if let Some(s) = v.as_str() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        escape_xml_text(&s, out);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    if let Some(n) = v.as_integer() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        out.push_str(&itoa::Buffer::new().format(n));
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    if v.as_float().is_some() {
        out.push('<');
        out.push_str(tag);
        out.push('>');
        out.push_str(&v.to_string());
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        return Ok(());
    }
    let map = if let Some(m) = v.as_hash_map() {
        m.clone()
    } else if let Some(r) = v.as_hash_ref() {
        r.read().clone()
    } else {
        return Err(PerlError::runtime(
            "xml_encode: element value must be hash(ref), string, or number",
            0,
        ));
    };
    if map.is_empty() {
        out.push('<');
        out.push_str(tag);
        out.push_str("/>");
        return Ok(());
    }
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut text: Option<String> = None;
    let mut children: Vec<(String, PerlValue)> = Vec::new();
    for (k, val) in map {
        if k.starts_with('@') && k.len() > 1 {
            let an = &k[1..];
            if !is_valid_xml_element_name(an) {
                return Err(PerlError::runtime(
                    format!("xml_encode: invalid attribute name `{an}`"),
                    0,
                ));
            }
            attrs.push((an.to_string(), val.to_string()));
        } else if k == "#text" || k == "_" {
            text = Some(val.to_string());
        } else {
            if !is_valid_xml_element_name(&k) {
                return Err(PerlError::runtime(
                    format!("xml_encode: invalid child element name `{k}`"),
                    0,
                ));
            }
            children.push((k, val));
        }
    }
    out.push('<');
    out.push_str(tag);
    for (a, aval) in &attrs {
        out.push(' ');
        out.push_str(a);
        out.push_str("=\"");
        escape_xml_attr_value(aval, out);
        out.push('"');
    }
    let has_body = text.is_some() || !children.is_empty();
    if !has_body {
        out.push_str("/>");
        return Ok(());
    }
    out.push('>');
    if let Some(ref t) = text {
        escape_xml_text(t, out);
    }
    for (cn, cv) in children {
        if let Some(a) = cv.as_array_vec() {
            for item in a {
                xml_write_element(out, &cn, &item)?;
            }
        } else {
            xml_write_element(out, &cn, &cv)?;
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Ok(())
}

/// Encode `v` as XML: `v` must be a hash(ref) with **exactly one** top-level key (the root element name).
///
/// Attribute keys use a leading `@` in Perl (`'@href' => '...'`); the `@` is stripped in the XML output.
pub(crate) fn xml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let map = if let Some(m) = v.as_hash_map() {
        m.clone()
    } else if let Some(r) = v.as_hash_ref() {
        r.read().clone()
    } else {
        return Err(PerlError::runtime(
            "xml_encode: need hash or hashref with one root element key",
            0,
        ));
    };
    if map.len() != 1 {
        return Err(PerlError::runtime(
            "xml_encode: top-level hash must have exactly one key (root element name)",
            0,
        ));
    }
    let (root_name, inner) = map.iter().next().expect("one");
    if !is_valid_xml_element_name(root_name) {
        return Err(PerlError::runtime(
            format!("xml_encode: invalid root element name `{root_name}`"),
            0,
        ));
    }
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    xml_write_element(&mut out, root_name, inner)?;
    Ok(PerlValue::string(out))
}

// ── TOML / YAML → PerlValue (same spirit as JSON) ──

pub(crate) fn toml_decode(s: &str) -> PerlResult<PerlValue> {
    let v: toml::Value = toml::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("toml_decode: {}", e), 0))?;
    Ok(toml_to_perl(v))
}

/// Serialize a [`PerlValue`] to TOML text (via JSON as intermediate; `null` / unsupported shapes error).
pub(crate) fn toml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let j = crate::native_data::perl_to_json_value(v)?;
    let s =
        toml::to_string(&j).map_err(|e| PerlError::runtime(format!("toml_encode: {}", e), 0))?;
    Ok(PerlValue::string(s))
}

fn toml_to_perl(v: toml::Value) -> PerlValue {
    match v {
        toml::Value::String(s) => PerlValue::string(s),
        toml::Value::Integer(n) => PerlValue::integer(n),
        toml::Value::Float(x) => PerlValue::float(x),
        toml::Value::Boolean(b) => PerlValue::integer(i64::from(b)),
        toml::Value::Datetime(d) => PerlValue::string(d.to_string()),
        toml::Value::Array(a) => PerlValue::array(a.into_iter().map(toml_to_perl).collect()),
        toml::Value::Table(t) => {
            let mut map = IndexMap::new();
            for (k, v) in t {
                map.insert(k, toml_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        }
    }
}

pub(crate) fn yaml_decode(s: &str) -> PerlResult<PerlValue> {
    let v: serde_yaml::Value = serde_yaml::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("yaml_decode: {}", e), 0))?;
    Ok(yaml_to_perl(v))
}

/// Serialize a [`PerlValue`] to YAML text (via JSON as intermediate).
pub(crate) fn yaml_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let j = crate::native_data::perl_to_json_value(v)?;
    let s = serde_yaml::to_string(&j)
        .map_err(|e| PerlError::runtime(format!("yaml_encode: {}", e), 0))?;
    Ok(PerlValue::string(s))
}

/// Percent-encode for URI components (RFC 3986 unreserved kept; space → `%20`).
pub(crate) fn url_encode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    Ok(PerlValue::string(
        utf8_percent_encode(s.as_str(), &NON_ALPHANUMERIC).to_string(),
    ))
}

/// Decode `%XX` plus unescaped bytes; UTF-8 is lossy-decoded.
pub(crate) fn url_decode(v: &PerlValue) -> PerlResult<PerlValue> {
    let s = v.to_string();
    Ok(PerlValue::string(
        percent_decode_str(s.trim())
            .decode_utf8_lossy()
            .into_owned(),
    ))
}

fn yaml_to_perl(v: serde_yaml::Value) -> PerlValue {
    match v {
        serde_yaml::Value::Null => PerlValue::UNDEF,
        serde_yaml::Value::Bool(b) => PerlValue::integer(i64::from(b)),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(u) = n.as_u64() {
                PerlValue::integer(u as i64)
            } else {
                PerlValue::float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_yaml::Value::String(s) => PerlValue::string(s),
        serde_yaml::Value::Sequence(seq) => {
            PerlValue::array(seq.into_iter().map(yaml_to_perl).collect())
        }
        serde_yaml::Value::Mapping(m) => {
            let mut map = IndexMap::new();
            for (k, v) in m {
                let key = match k {
                    serde_yaml::Value::String(s) => s,
                    _ => yaml_to_perl(k).to_string(),
                };
                map.insert(key, yaml_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(RwLock::new(map)))
        }
        serde_yaml::Value::Tagged(t) => yaml_to_perl(t.value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vector() {
        let p = sha256(&PerlValue::string("abc".into())).expect("sha256");
        assert_eq!(
            p.to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn md5_and_sha1_vectors() {
        assert_eq!(
            md5_digest(&PerlValue::string("".into()))
                .unwrap()
                .to_string(),
            "d41d8cd98f00b204e9800998ecf8427e"
        );
        assert_eq!(
            md5_digest(&PerlValue::string("abc".into()))
                .unwrap()
                .to_string(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            sha1_digest(&PerlValue::string("abc".into()))
                .unwrap()
                .to_string(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn url_encode_decode_roundtrip() {
        let s = "a b+c?foo";
        let e = url_encode(&PerlValue::string(s.into())).unwrap();
        assert_eq!(e.to_string(), "a%20b%2Bc%3Ffoo");
        let d = url_decode(&e).unwrap();
        assert_eq!(d.to_string(), s);
    }

    #[test]
    fn toml_yaml_encode_roundtrip_simple() {
        let h = PerlValue::hash_ref(Arc::new(RwLock::new({
            let mut m = IndexMap::new();
            m.insert("x".into(), PerlValue::integer(7));
            m.insert("k".into(), PerlValue::string("v".into()));
            m
        })));
        let t = toml_encode(&h).unwrap().to_string();
        let back = toml_decode(&t).unwrap();
        assert_eq!(
            back.as_hash_ref()
                .unwrap()
                .read()
                .get("x")
                .unwrap()
                .to_int(),
            7
        );
        let y = yaml_encode(&h).unwrap().to_string();
        let yback = yaml_decode(&y).unwrap();
        assert_eq!(
            yback
                .as_hash_ref()
                .unwrap()
                .read()
                .get("k")
                .unwrap()
                .to_string(),
            "v"
        );
    }

    #[test]
    fn gzip_roundtrip() {
        let s = "hello compression";
        let g = gzip(&PerlValue::string(s.into())).expect("gzip");
        let back = gunzip(&g).expect("gunzip");
        assert_eq!(back.to_string(), s);
    }

    #[test]
    fn zstd_roundtrip() {
        let s = "zstd payload";
        let z = zstd_compress(&PerlValue::string(s.into())).expect("zstd");
        let back = zstd_decode(&z).expect("zstd_decode");
        assert_eq!(back.to_string(), s);
    }

    #[test]
    fn yaml_mapping() {
        let p = yaml_decode("a: 1\nb: [x, y]").expect("yaml");
        let h = p.as_hash_ref().expect("hash");
        let g = h.read();
        assert_eq!(g.get("a").unwrap().to_int(), 1);
    }

    #[test]
    fn xml_decode_attrs_and_repeated_children() {
        let x = r#"<root k="1"><item>a</item><item>b</item></root>"#;
        let p = xml_decode(x).expect("xml_decode");
        let h = p.as_hash_ref().expect("root hash");
        let g = h.read();
        let root = g.get("root").expect("root");
        let root_h = root.as_hash_ref().expect("inner");
        let rh = root_h.read();
        assert_eq!(rh.get("@k").unwrap().to_string(), "1");
        let items = rh.get("item").unwrap().as_array_vec().expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0]
                .as_hash_ref()
                .unwrap()
                .read()
                .get("#text")
                .unwrap()
                .to_string(),
            "a"
        );
    }

    #[test]
    fn xml_encode_decode_roundtrip() {
        let mut inner = IndexMap::new();
        inner.insert("@id".into(), PerlValue::string("9".into()));
        inner.insert("#text".into(), PerlValue::string("hi".into()));
        let mut root = IndexMap::new();
        root.insert(
            "msg".into(),
            PerlValue::hash_ref(Arc::new(RwLock::new(inner))),
        );
        let v = PerlValue::hash_ref(Arc::new(RwLock::new(root)));
        let s = xml_encode(&v).unwrap().to_string();
        let back = xml_decode(&s).unwrap();
        let back_h = back.as_hash_ref().unwrap();
        let h = back_h.read();
        let msg_h = h.get("msg").unwrap().as_hash_ref().unwrap();
        let msg = msg_h.read();
        assert_eq!(msg.get("@id").unwrap().to_string(), "9");
        assert_eq!(msg.get("#text").unwrap().to_string(), "hi");
    }

    #[test]
    fn toml_table() {
        let p = toml_decode("key = \"v\"\nn = 3").expect("toml");
        let h = p.as_hash_ref().expect("hash");
        let g = h.read();
        assert_eq!(g.get("key").unwrap().to_string(), "v");
        assert_eq!(g.get("n").unwrap().to_int(), 3);
    }

    #[test]
    fn datetime_parse_format_america_new_york() {
        let epoch = datetime_parse_local(
            &PerlValue::string("2024-06-15 12:00:00".into()),
            &PerlValue::string("America/New_York".into()),
        )
        .expect("parse");
        let wall = datetime_format_tz(
            &epoch,
            &PerlValue::string("America/New_York".into()),
            &PerlValue::string("%Y-%m-%d %H:%M:%S".into()),
        )
        .expect("fmt");
        assert_eq!(wall.to_string(), "2024-06-15 12:00:00");
    }

    #[test]
    fn datetime_add_seconds_delta() {
        let out =
            datetime_add_seconds(&PerlValue::float(1_000.0), &PerlValue::float(2.25)).unwrap();
        assert!((out.to_number() - 1002.25).abs() < 1e-9);
    }
}
