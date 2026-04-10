//! Cryptographic digests, compression, config decoders, and UTC/epoch datetime helpers.

use std::io::{Read, Write};

use base64::Engine;
use chrono::{DateTime, TimeZone, Utc};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use hmac::{Hmac, Mac};
use indexmap::IndexMap;
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
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

/// HMAC-SHA256(key, message); both taken as bytes from string values; returns lowercase hex.
pub(crate) fn hmac_sha256(key: &PerlValue, msg: &PerlValue) -> PerlResult<PerlValue> {
    let mut mac = HmacSha256::new_from_slice(&bytes_from_value(key))
        .map_err(|e| PerlError::runtime(format!("hmac_sha256: {}", e), 0))?;
    mac.update(&bytes_from_value(msg));
    let out = mac.finalize().into_bytes();
    Ok(PerlValue::string(hex::encode(out)))
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

// ── TOML / YAML → PerlValue (same spirit as JSON) ──

pub(crate) fn toml_decode(s: &str) -> PerlResult<PerlValue> {
    let v: toml::Value = toml::from_str(s.trim())
        .map_err(|e| PerlError::runtime(format!("toml_decode: {}", e), 0))?;
    Ok(toml_to_perl(v))
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
    fn toml_table() {
        let p = toml_decode("key = \"v\"\nn = 3").expect("toml");
        let h = p.as_hash_ref().expect("hash");
        let g = h.read();
        assert_eq!(g.get("key").unwrap().to_string(), "v");
        assert_eq!(g.get("n").unwrap().to_int(), 3);
    }
}
