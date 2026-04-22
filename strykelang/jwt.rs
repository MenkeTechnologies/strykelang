//! JWT HS256 using HMAC-SHA256 and base64url (no JWE/JWKS).

use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::error::{PerlError, PerlResult};
use crate::native_codec::{
    base64url_decode, base64url_encode, hmac_sha256_raw, hmac_sha256_verify_raw,
};
use crate::native_data::{json_decode, json_encode};
use crate::value::PerlValue;

pub(crate) fn jwt_encode(
    payload: &PerlValue,
    secret: &PerlValue,
    alg: &str,
    line: usize,
) -> PerlResult<PerlValue> {
    if alg != "HS256" {
        return Err(PerlError::runtime(
            format!("jwt_encode: only alg HS256 is supported (got {alg})"),
            line,
        ));
    }
    let header = serde_json::json!({ "alg": "HS256", "typ": "JWT" });
    let header_str = serde_json::to_string(&header)
        .map_err(|e| PerlError::runtime(format!("jwt_encode: {e}"), line))?;
    let header_b64 = base64url_encode(header_str.as_bytes());
    let payload_str = json_encode(payload)?;
    let payload_b64 = base64url_encode(payload_str.as_bytes());
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = hmac_sha256_raw(secret, &PerlValue::string(signing_input.clone()))?;
    let sig_b64 = base64url_encode(&sig);
    Ok(PerlValue::string(format!("{signing_input}.{sig_b64}")))
}

pub(crate) fn jwt_decode(token: &str, secret: &PerlValue, line: usize) -> PerlResult<PerlValue> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 {
        return Err(PerlError::runtime(
            format!(
                "jwt_decode: expected 3 dot-separated segments, got {}",
                parts.len()
            ),
            line,
        ));
    }
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let sig = base64url_decode(parts[2])?;
    hmac_sha256_verify_raw(secret, &PerlValue::string(signing_input), &sig)
        .map_err(|_| PerlError::runtime("jwt_decode: signature verification failed", line))?;

    let header_bytes = base64url_decode(parts[0])?;
    let header_str = std::str::from_utf8(&header_bytes)
        .map_err(|_| PerlError::runtime("jwt_decode: header is not UTF-8", line))?;
    let header: JsonValue = serde_json::from_str(header_str)
        .map_err(|e| PerlError::runtime(format!("jwt_decode: invalid header JSON: {e}"), line))?;
    if header.get("alg").and_then(|v| v.as_str()) != Some("HS256") {
        return Err(PerlError::runtime(
            "jwt_decode: only HS256 tokens are supported",
            line,
        ));
    }

    let payload_bytes = base64url_decode(parts[1])?;
    let payload_str = std::str::from_utf8(&payload_bytes)
        .map_err(|_| PerlError::runtime("jwt_decode: payload is not UTF-8", line))?;
    let payload_json: JsonValue = serde_json::from_str(payload_str)
        .map_err(|e| PerlError::runtime(format!("jwt_decode: invalid payload JSON: {e}"), line))?;

    let now = Utc::now().timestamp();
    if let Some(exp) = payload_json.get("exp") {
        let exp_t = jwt_claim_time(exp, line)?;
        if now >= exp_t {
            return Err(PerlError::runtime("jwt_decode: token expired (exp)", line));
        }
    }
    if let Some(nbf) = payload_json.get("nbf") {
        let nbf_t = jwt_claim_time(nbf, line)?;
        if now < nbf_t {
            return Err(PerlError::runtime(
                "jwt_decode: token not yet valid (nbf)",
                line,
            ));
        }
    }

    json_decode(payload_str)
}

fn jwt_claim_time(v: &JsonValue, line: usize) -> PerlResult<i64> {
    if let Some(i) = v.as_i64() {
        return Ok(i);
    }
    if let Some(u) = v.as_u64() {
        return Ok(u as i64);
    }
    if let Some(f) = v.as_f64() {
        return Ok(f as i64);
    }
    Err(PerlError::runtime(
        "jwt_decode: exp/nbf must be a number",
        line,
    ))
}

pub(crate) fn jwt_decode_unsafe(token: &str, line: usize) -> PerlResult<PerlValue> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 {
        return Err(PerlError::runtime(
            format!(
                "jwt_decode_unsafe: expected 3 dot-separated segments, got {}",
                parts.len()
            ),
            line,
        ));
    }
    let payload_bytes = base64url_decode(parts[1])?;
    let payload_str = std::str::from_utf8(&payload_bytes)
        .map_err(|_| PerlError::runtime("jwt_decode_unsafe: payload is not UTF-8", line))?;
    json_decode(payload_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::PerlValue;

    #[test]
    fn test_jwt_roundtrip() {
        let payload = PerlValue::hash(
            [
                (
                    "sub".to_string(),
                    PerlValue::string("1234567890".to_string()),
                ),
                (
                    "name".to_string(),
                    PerlValue::string("John Doe".to_string()),
                ),
                ("admin".to_string(), PerlValue::integer(1)),
            ]
            .into_iter()
            .collect(),
        );
        let secret = PerlValue::string("secret".to_string());

        let token = jwt_encode(&payload, &secret, "HS256", 0).unwrap();
        let token_str = token.to_string();

        let decoded = jwt_decode(&token_str, &secret, 0).unwrap();
        let decoded_ref = decoded.as_hash_ref().unwrap();
        let decoded_hash = decoded_ref.read();

        assert_eq!(decoded_hash.get("sub").unwrap().to_string(), "1234567890");
        assert_eq!(decoded_hash.get("name").unwrap().to_string(), "John Doe");
        assert_eq!(decoded_hash.get("admin").unwrap().to_int(), 1);
    }

    #[test]
    fn test_jwt_decode_unsafe() {
        let payload = PerlValue::hash(
            [("foo".to_string(), PerlValue::string("bar".to_string()))]
                .into_iter()
                .collect(),
        );
        let secret = PerlValue::string("secret".to_string());
        let token = jwt_encode(&payload, &secret, "HS256", 0).unwrap();
        let token_str = token.to_string();

        let decoded = jwt_decode_unsafe(&token_str, 0).unwrap();
        let decoded_ref = decoded.as_hash_ref().unwrap();
        let decoded_hash = decoded_ref.read();
        assert_eq!(decoded_hash.get("foo").unwrap().to_string(), "bar");
    }

    #[test]
    fn test_jwt_invalid_signature() {
        let payload = PerlValue::hash(
            [("foo".to_string(), PerlValue::string("bar".to_string()))]
                .into_iter()
                .collect(),
        );
        let secret = PerlValue::string("secret".to_string());
        let token = jwt_encode(&payload, &secret, "HS256", 0).unwrap();
        let mut token_str = token.to_string();

        // Corrupt the signature (last part)
        token_str.push('x');

        let res = jwt_decode(&token_str, &secret, 0);
        assert!(res.is_err());
    }

    #[test]
    fn test_jwt_expiration() {
        let now = Utc::now().timestamp();
        let payload = PerlValue::hash(
            [("exp".to_string(), PerlValue::integer(now - 10))]
                .into_iter()
                .collect(),
        );
        let secret = PerlValue::string("secret".to_string());
        let token = jwt_encode(&payload, &secret, "HS256", 0).unwrap();

        let res = jwt_decode(&token.to_string(), &secret, 0);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("expired"));
    }
}
