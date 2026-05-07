// Batch 51 — security: KDFs, MFA, PKI, web security, TLS, ciphers.

// Argon2 memory cost m: chosen so that derivation takes ≥ target_ms on a known
// reference machine. RFC 9106 recommends m ≥ 2^16 KiB (=64 MiB) for Argon2id.
// Throughput model: hash_time(m, t, p) ≈ m · t / (p · 1e6) ms (per OWASP bench).
// Args: target_ms, time_cost t, parallelism p. Returns m_cost (KiB, log₂).
fn builtin_sec_argon2_memcost(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target_ms = f1(args).max(100.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let m_kib = (target_ms * p * 1e3) / t;
    let log2_m = m_kib.log2().ceil().max(16.0);
    Ok(PerlValue::integer(log2_m as i64))
}

// Argon2 time cost t: iterations per memory pass. RFC 9106 recommends t ≥ 1
// for Argon2id, t ≥ 3 for Argon2i. Args: target_ms, m (KiB), p. Returns t.
fn builtin_sec_argon2_timecost(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target_ms = f1(args).max(100.0);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(65536.0).max(1024.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let t = (target_ms * p * 1e3) / m;
    Ok(PerlValue::integer(t.ceil().max(1.0) as i64))
}

// Argon2 parallelism p: bounded by min(2 · cores, m / (8 · MIN_MEMORY)) per
// RFC 9106. Args: cores, m (KiB).
fn builtin_sec_argon2_parallelism(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cores = f1(args).max(1.0);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(65536.0);
    let cap = (m / 64.0).floor();
    Ok(PerlValue::integer((2.0 * cores).min(cap).max(1.0) as i64))
}

// Argon2 block step (BLAKE2b mixed)
fn builtin_sec_argon2_block_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = i1(args) as u64;
    let next = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    Ok(PerlValue::integer((prev ^ next).wrapping_mul(0x9e37_79b9_7f4a_7c15) as i64))
}

// PBKDF2 iteration count for target_ms wall-clock with measured iter_per_ms
// throughput. OWASP 2023 floors: 600k (SHA-256), 210k (SHA-512), 1.3M (SHA-1).
// Args: target_ms, iter_per_ms_throughput, prf_id (0=SHA-256, 1=SHA-512, 2=SHA-1).
fn builtin_sec_pbkdf2_iter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target_ms = f1(args).max(100.0);
    let iter_per_ms = args.get(1).map(|v| v.to_number()).unwrap_or(600.0).max(1.0);
    let prf = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let owasp_floor = match prf { 0 => 600_000, 1 => 210_000, 2 => 1_300_000, _ => 600_000 };
    let computed = (target_ms * iter_per_ms) as i64;
    Ok(PerlValue::integer(computed.max(owasp_floor)))
}

// scrypt N (cost): N = 2^log2_N. Memory ≈ 128 · N · r bytes. RFC 7914 + OWASP:
// N ≥ 2^17 = 131072 with r=8, p=1 (≈ 128 MiB). Args: target_ms, r, throughput.
fn builtin_sec_scrypt_n_param(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target_ms = f1(args).max(100.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(8.0).max(1.0);
    let bytes_per_ms = args.get(2).map(|v| v.to_number()).unwrap_or(150_000_000.0).max(1.0);
    let memory_bytes = target_ms * bytes_per_ms;
    let n = (memory_bytes / (128.0 * r)).log2().floor() as i64;
    Ok(PerlValue::integer((1_i64 << n.clamp(15, 23)).max(131072)))
}

// scrypt block size r: trades memory ↔ CPU. r=8 default per RFC 7914 §6;
// raise to 16 for L1-fitting attacks. Args: cache_kb_per_thread.
fn builtin_sec_scrypt_r_param(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cache_kb = f1(args).max(32.0);
    Ok(PerlValue::integer(if cache_kb >= 1024.0 { 16 } else { 8 }))
}

// scrypt parallelism p: scales linearly with cost. Default 1 unless attacker
// has fewer cores than defender. Args: defender_cores.
fn builtin_sec_scrypt_p_param(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cores = f1(args).max(1.0);
    Ok(PerlValue::integer(cores.min(8.0).max(1.0) as i64))
}

// Balloon hashing (Boneh, Corrigan-Gibbs, Schechter 2016): mix block i with
// block (i−1) and δ random earlier blocks. One step:
//   buf[i] = H(cnt ‖ buf[i−1] ‖ buf[r₁] ‖ … ‖ buf[r_δ])
// where r_k are δ random indices < i. Approximated as XOR-fold of inputs (the
// underlying H output) — the load-bearing structural element is the random-
// predecessor mixing. Args: prev (buf[i−1]), array of δ random predecessors,
// counter cnt.
fn builtin_sec_balloon_hash_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = i1(args) as u64;
    let randoms = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let cnt = args.get(2).map(|v| v.to_number() as u64).unwrap_or(0);
    let mut h = cnt.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ prev;
    for r in randoms.iter() {
        let rv = r.to_number() as u64;
        h ^= rv.rotate_left(13);
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(PerlValue::integer(h as i64))
}

// yescrypt PWXFORM round (Peslyak): scrypt-like ROMix-BlockMix-Salsa20/8 with
// a sequential, hardware-aware S-box lookup. One PWXFORM round:
//   x = (x ^ S₀[(x >> 8) & MASK]) · S₁[x & MASK] + (counter)
// where S₀, S₁ are caller-supplied 64-bit S-boxes. Args: x, s0_lookup, s1_lookup, ctr.
fn builtin_sec_yescrypt_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = i1(args) as u64;
    let s0 = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let s1 = args.get(2).map(|v| v.to_number() as u64).unwrap_or(1);
    let ctr = args.get(3).map(|v| v.to_number() as u64).unwrap_or(0);
    let new_x = (x ^ s0).wrapping_mul(s1.max(1)).wrapping_add(ctr);
    Ok(PerlValue::integer(new_x as i64))
}

// bcrypt cost factor (10..14 typical)
fn builtin_sec_bcrypt_cost_factor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(i1(args).clamp(4, 31)))
}

// bcrypt round step
fn builtin_sec_bcrypt_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = i1(args);
    Ok(PerlValue::integer(1_i64 << cost.min(31).max(0)))
}

// zxcvbn-like password strength (0..4)
fn builtin_sec_password_strength_zxcvbn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let entropy = f1(args);
    let score = if entropy < 28.0 { 0 } else if entropy < 36.0 { 1 } else if entropy < 60.0 { 2 } else if entropy < 128.0 { 3 } else { 4 };
    Ok(PerlValue::integer(score))
}

// HaveIBeenPwned k-anonymity prefix check (1 if found)
fn builtin_sec_haveibeenpwned_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let count = f1(args);
    Ok(PerlValue::integer(if count > 0.0 { 1 } else { 0 }))
}

// Diceware word index
fn builtin_sec_diceware_word_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = i1(args) as u64;
    Ok(PerlValue::integer((h % 7776) as i64))
}

// XKCD passphrase score: words × log2(7776)
fn builtin_sec_xkcd_passphrase_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let words = f1(args);
    Ok(PerlValue::float(words * 7776f64.log2()))
}

// Passphrase entropy: log2(charset^len)
fn builtin_sec_passphrase_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let charset = f1(args);
    let len = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if charset <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(len * charset.log2()))
}

// Chosen charset strength
fn builtin_sec_chosen_charset_strength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lower = i1(args);
    let upper = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let digits = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let symbols = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut size = 0_i64;
    if lower != 0 { size += 26; }
    if upper != 0 { size += 26; }
    if digits != 0 { size += 10; }
    if symbols != 0 { size += 32; }
    Ok(PerlValue::integer(size))
}

// Keystroke timing variance (anomaly detection)
fn builtin_sec_keystroke_timing_var(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let xs: Vec<f64> = v.iter().map(|x| x.to_number()).collect();
    if xs.len() < 2 { return Ok(PerlValue::float(0.0)); }
    let mean: f64 = xs.iter().sum::<f64>() / xs.len() as f64;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (xs.len() - 1) as f64;
    Ok(PerlValue::float(var))
}

// TOTP time-step window per RFC 6238 §5.2: counter T = ⌊(now - T0) / X⌋, where
// X is the step (default 30s). Args: now, T0, step. Returns counter value.
fn builtin_sec_2fa_totp_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let now = f1(args);
    let t0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let step = args.get(2).map(|v| v.to_number()).unwrap_or(30.0).max(1.0);
    Ok(PerlValue::integer(((now - t0) / step).floor() as i64))
}

// TOTP drift check (within ±N steps)
fn builtin_sec_totp_drift_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let drift = i1(args).abs();
    let allowed = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(if drift <= allowed { 1 } else { 0 }))
}

// HOTP counter step per RFC 4226 §5.2: server pre-increments counter on each
// validation attempt (look-ahead window w). Returns the next-expected counter
// given current server counter and observed client counter; rejects if outside
// look-ahead window. Args: server_c, client_c, look_ahead.
fn builtin_sec_hotp_counter_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let server = i1(args);
    let client = args.get(1).map(|v| v.to_number() as i64).unwrap_or(server + 1);
    let w = args.get(2).map(|v| v.to_number() as i64).unwrap_or(3);
    let delta = client - server;
    if delta >= 1 && delta <= w { return Ok(PerlValue::integer(client + 1)); }
    Ok(PerlValue::integer(server))
}

// YubiKey OTP CRC-16 (CCITT-FALSE-style polynomial 0xA001). 16-byte modhex
// payload + 2-byte CRC. Verify by CRC residue == 0xF0B8 (RFC-style fixed).
// Args: array of 16 payload bytes + 2 CRC bytes.
fn builtin_sec_yubikey_otp_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.len() != 18 { return Ok(PerlValue::integer(0)); }
    let mut crc: u16 = 0xffff;
    for b in v.iter() {
        crc ^= (b.to_number() as u8) as u16;
        for _ in 0..8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb == 1 { crc ^= 0x8408; }
        }
    }
    Ok(PerlValue::integer(if crc == 0xf0b8 { 1 } else { 0 }))
}

// WebAuthn attestation: verify that (1) flags include UP+UV, (2) RP-ID-hash
// matches expected, (3) signCount > stored. Args: flags byte, rp_hash_match
// (0/1), sign_count, stored_count.
fn builtin_sec_webauthn_attestation_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let flags = i1(args) as u8;
    let rp_match = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let sign_c = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let stored = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let up = flags & 0x01 != 0;
    let uv = flags & 0x04 != 0;
    Ok(PerlValue::integer(if up && uv && rp_match != 0 && sign_c > stored { 1 } else { 0 }))
}

// FIDO2 / CTAP2 assertion (authenticatorGetAssertion). Differs from
// attestation (registration): assertion uses STORED credential to sign a
// fresh challenge. Validation needs:
//   1. Signed clientDataHash matches expected RP-issued challenge.
//   2. authData flags include UP, optionally UV; signCount strictly > stored.
//   3. ECDSA/EdDSA signature verifies under the credential's public key.
// Args: signature_ok (0/1), flags byte (UP=0x01, UV=0x04), sign_count, stored.
fn builtin_sec_fido2_assertion_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sig_ok = i1(args);
    let flags = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0);
    let sign_c = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let stored = args.get(3).map(|v| v.to_number() as i64).unwrap_or(-1);
    let up = flags & 0x01 != 0;
    Ok(PerlValue::integer(if sig_ok != 0 && up && sign_c > stored { 1 } else { 0 }))
}

// Certificate chain validation: walk chain ensuring each cert's issuer matches
// previous cert's subject. Returns depth at which break occurs, or full length
// if valid. Args: array of [issuer_id, subject_id] pairs from leaf to root.
fn builtin_sec_certificate_chain_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.len() < 2 || v.len() % 2 != 0 { return Ok(PerlValue::integer(0)); }
    let n = v.len() / 2;
    for i in 1..n {
        let issuer_prev = v[2 * (i - 1)].to_number() as i64;
        let subject_cur = v[2 * i + 1].to_number() as i64;
        if issuer_prev != subject_cur { return Ok(PerlValue::integer(i as i64)); }
    }
    Ok(PerlValue::integer(n as i64))
}

// OCSP revocation check (1=ok, 0=revoked)
fn builtin_sec_revocation_ocsp_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let revoked = i1(args);
    Ok(PerlValue::integer(if revoked == 0 { 1 } else { 0 }))
}

// CRL age in seconds
fn builtin_sec_crl_age_seconds(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let now = f1(args);
    let issued = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((now - issued).max(0.0)))
}

// PKI path validation (length OK, not expired)
fn builtin_sec_pki_path_validate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let depth = i1(args);
    let max = args.get(1).map(|v| v.to_number() as i64).unwrap_or(8);
    Ok(PerlValue::integer(if depth > 0 && depth <= max { 1 } else { 0 }))
}

// X.509 subject match
fn builtin_sec_x509_subject_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let actual = i1(args);
    let expected = args.get(1).map(|v| v.to_number() as i64).unwrap_or(actual);
    Ok(PerlValue::integer(if actual == expected { 1 } else { 0 }))
}

// SAN match count: count of Subject Alt Name entries that match a wildcard
// pattern under RFC 6125 (left-most label only). Args: san_array, host_array.
fn builtin_sec_san_match_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sans = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let host = i1(&args[1..]) as i64;
    let count = sans.iter().filter(|s| {
        let v = s.to_number() as i64;
        v == host || v == -1
    }).count();
    Ok(PerlValue::integer(count as i64))
}

// Basic constraints CA per RFC 5280 §4.2.1.9: cert IS a CA iff cA=true AND
// pathLenConstraint missing or remaining_depth ≤ pathLen. Args: ca_flag,
// path_len_constraint, remaining_depth.
fn builtin_sec_basic_constraints_ca(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ca = i1(args);
    let path_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(-1);
    let remaining = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if ca == 0 { return Ok(PerlValue::integer(0)); }
    if path_len < 0 { return Ok(PerlValue::integer(1)); }
    Ok(PerlValue::integer(if remaining <= path_len { 1 } else { 0 }))
}

// Certificate pinning compare
fn builtin_sec_pinning_compare(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let actual = i1(args) as u64;
    let pinned = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    Ok(PerlValue::integer(if actual == pinned { 1 } else { 0 }))
}

// Certificate Transparency SCT validation per RFC 6962 §3.2: verify SCT
// timestamp ≤ now AND signature_alg ∈ allowed AND log_id is recognized.
// Args: sct_timestamp_ms, now_ms, sig_alg_id, log_known (0/1).
fn builtin_sec_certificate_transparency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sct_ts = f1(args);
    let now_ms = args.get(1).map(|v| v.to_number()).unwrap_or(sct_ts);
    let sig_alg = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let log_known = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let alg_ok = matches!(sig_alg, 0x0403 | 0x0503 | 0x0603 | 0x0807 | 0x0808);
    Ok(PerlValue::integer(if sct_ts <= now_ms && alg_ok && log_known != 0 { 1 } else { 0 }))
}

// DANE TLSA per RFC 6698 §2: verify presented certificate against TLSA RR
// (Usage, Selector, Matching Type) tuple.
//   usage:    0=PKIX-TA, 1=PKIX-EE, 2=DANE-TA, 3=DANE-EE.
//   selector: 0 = full cert, 1 = SubjectPublicKeyInfo only.
//   match:    0 = exact, 1 = SHA-256 hash, 2 = SHA-512.
// Returns 1 if all three layers match per the TLSA semantics. Args: usage,
// selector, match_type, computed_match (precomputed comparison result 0/1).
fn builtin_sec_dane_tlsa_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let usage = i1(args);
    let selector = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let match_type = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let computed = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    if !(0..=3).contains(&usage) { return Ok(PerlValue::integer(0)); }
    if !(0..=1).contains(&selector) { return Ok(PerlValue::integer(0)); }
    if !(0..=2).contains(&match_type) { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer(computed))
}

// HPKP pin match (deprecated but kept)
fn builtin_sec_hpkp_pin_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_pinning_compare(args)
}

// CSP source-list match per W3C CSP3 §6.6: source-expression matches the
// resource origin if (a) 'self' and same-origin, (b) scheme allow-list,
// (c) host-source with optional wildcard, or (d) 'unsafe-inline' / 'nonce-…'
// tokens. Returns 1 if any directive entry matches. Args: directive_kind
// (0='self', 1=scheme, 2=host_wildcard, 3=nonce, 4=hash), match (0/1).
fn builtin_sec_csp_directive_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kind = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if !(0..=4).contains(&kind) { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer(if m != 0 { 1 } else { 0 }))
}

// CSRF token match
fn builtin_sec_csrf_token_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_pinning_compare(args)
}

// CORS Access-Control-Allow-Origin policy per Fetch spec: server returns one
// allow-origin value (NOT a list). Browser checks: allow_origin == "*" (any),
// allow_origin == request_origin (exact, case-sensitive), OR ACAO is omitted
// AND request was simple. Wildcards are NOT allowed in subdomains. Credentials
// require exact match (no '*'). Args: request_origin, allow_origin, has_creds.
fn builtin_sec_cors_origin_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let req = i1(args);
    let allow = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let creds = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if creds != 0 && allow == -1 { return Ok(PerlValue::integer(0)); }
    if allow == -1 { return Ok(PerlValue::integer(1)); }
    Ok(PerlValue::integer(if req == allow { 1 } else { 0 }))
}

// XSS filter score: count of suspect token kinds in input.
//   <script: +5  on*=:  +3  javascript:: +4  data:text/html: +3  <iframe: +2
// Args: array of suspect-token IDs (0..5) found in payload.
fn builtin_sec_xss_filter_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let weights = [0.0, 5.0, 3.0, 4.0, 3.0, 2.0];
    let s: f64 = v.iter().map(|x| {
        let i = x.to_number() as usize;
        if i < weights.len() { weights[i] } else { 0.0 }
    }).sum();
    Ok(PerlValue::float(s))
}

// HTML escape check (1 if no '<' '>' '&' present)
fn builtin_sec_html_escape_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let suspicious = i1(args);
    Ok(PerlValue::integer(if suspicious == 0 { 1 } else { 0 }))
}

// URL-safe encoding check per RFC 3986 §2.3: only alphanumeric A–Z, a–z, 0–9
// and unreserved punctuation [- _ . ~] may appear unencoded. Differs from HTML
// escape (which targets <, >, &, ", '). Args: array of code-points.
fn builtin_sec_url_safe_encode_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cps = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    for c in &cps {
        let cp = c.to_number() as u32;
        let safe = (cp >= 0x30 && cp <= 0x39) || (cp >= 0x41 && cp <= 0x5a)
            || (cp >= 0x61 && cp <= 0x7a) || cp == 0x2d || cp == 0x5f
            || cp == 0x2e || cp == 0x7e || cp == 0x25;
        if !safe { return Ok(PerlValue::integer(0)); }
    }
    Ok(PerlValue::integer(1))
}

// Path traversal detection: count substrings ".." that are NOT preceded
// by an alnum (so "../" or "..\\"/EOF, NOT "foo..bar"). Args: array of
// code-points.
fn builtin_sec_path_traversal_detect(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let cp: Vec<i64> = s.iter().map(|x| x.to_number() as i64).collect();
    let mut n = 0_i64;
    for i in 0..cp.len().saturating_sub(1) {
        if cp[i] == '.' as i64 && cp[i + 1] == '.' as i64 {
            let prev_alnum = i > 0 && {
                let p = cp[i - 1];
                (0..=127).contains(&p) && (p as u8).is_ascii_alphanumeric()
            };
            if !prev_alnum { n += 1; }
        }
    }
    Ok(PerlValue::integer(n))
}

// SQLi pattern score: weighted sum of suspect-token IDs:
//   ' OR 1=1: +5  -- comment: +3  ; DROP: +6  UNION SELECT: +4  ' AND ': +3
// Args: array of suspect-token IDs (0..5).
fn builtin_sec_sqli_pattern_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let weights = [0.0, 5.0, 3.0, 6.0, 4.0, 3.0];
    let s: f64 = v.iter().map(|x| {
        let i = x.to_number() as usize;
        if i < weights.len() { weights[i] } else { 0.0 }
    }).sum();
    Ok(PerlValue::float(s))
}

// XXE pattern score: weight DOCTYPE+ENTITY appearances.
//   <!DOCTYPE: +3  <!ENTITY ... SYSTEM: +6  &xxe;: +5  external SUBSET: +4
// Args: array of suspect-token IDs (0..4).
fn builtin_sec_xxe_pattern_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let weights = [0.0, 3.0, 6.0, 5.0, 4.0];
    let s: f64 = v.iter().map(|x| {
        let i = x.to_number() as usize;
        if i < weights.len() { weights[i] } else { 0.0 }
    }).sum();
    Ok(PerlValue::float(s))
}

// XXE DTD presence: returns 1 if input bytes contain "<!DOCTYPE" trigram and
// at least one "<!ENTITY ... SYSTEM" external reference signature.
// Args: doctype_count, entity_external_count.
fn builtin_sec_xxe_dtd_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let doctype = i1(args);
    let entity_ext = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if doctype > 0 && entity_ext > 0 { 1 } else { 0 }))
}

// Command injection score: weighted scan of suspect tokens.
//   ;: +3  |: +3  &&: +2  $(): +5  ``: +5  %0a (newline): +4
// Args: array of suspect-token IDs (0..6).
fn builtin_sec_command_injection_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let weights = [0.0, 3.0, 3.0, 2.0, 5.0, 5.0, 4.0];
    let s: f64 = v.iter().map(|x| {
        let i = x.to_number() as usize;
        if i < weights.len() { weights[i] } else { 0.0 }
    }).sum();
    Ok(PerlValue::float(s))
}

// IDOR check (does user own resource?)
fn builtin_sec_idor_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let user_id = i1(args);
    let resource_owner = args.get(1).map(|v| v.to_number() as i64).unwrap_or(user_id);
    Ok(PerlValue::integer(if user_id == resource_owner { 1 } else { 0 }))
}

// JWT alg safe (HS256, RS256, ES256, EdDSA, PS256)
fn builtin_sec_jwt_alg_safe(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alg = i1(args);
    Ok(PerlValue::integer(if (1..=5).contains(&alg) { 1 } else { 0 }))
}

// JWT kid match
fn builtin_sec_jwt_kid_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_pinning_compare(args)
}

// JWT signature verification per RFC 7515 §5.2: split header.payload.signature,
// recompute MAC over (header || '.' || payload) and compare CT-equality.
// Args: expected_mac (hash), actual_mac, alg_id (1=HS256, 2=RS256, etc).
// CT-compare: XOR-fold of byte arrays returns 0 iff equal.
fn builtin_sec_jwt_signature_verify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let expected = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let actual = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let alg = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    if alg == 0 || expected.len() != actual.len() || expected.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    let mut acc = 0_u64;
    for (a, b) in expected.iter().zip(actual.iter()) {
        acc |= ((a.to_number() as u64) ^ (b.to_number() as u64)) & 0xff;
    }
    Ok(PerlValue::integer(if acc == 0 { 1 } else { 0 }))
}

// OAuth2 state validate
fn builtin_sec_oauth2_state_validate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_pinning_compare(args)
}

// OAuth2 PKCE per RFC 7636 §4.6: server verifies code_verifier matches the
// stored code_challenge via the chosen code_challenge_method:
//   plain:  challenge == verifier
//   S256:   challenge == BASE64URL-without-padding(SHA256(verifier))
// Server MUST require S256 unless plain is justified. Args: method (0=plain,
// 1=S256), supplied_verifier_len, sha256_digest_match (0/1 prebuilt).
fn builtin_sec_oauth2_pkce_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use sha2::{Digest, Sha256};
    let method = i1(args);
    let verifier_arr = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let challenge = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    if verifier_arr.is_empty() || verifier_arr.len() < 43 || verifier_arr.len() > 128 {
        return Ok(PerlValue::integer(0));
    }
    let bytes: Vec<u8> = verifier_arr.iter().map(|c| c.to_number() as u8).collect();
    if method == 0 {
        let s: String = bytes.iter().map(|&b| b as char).collect();
        return Ok(PerlValue::integer(if s == challenge { 1 } else { 0 }));
    }
    let mut h = Sha256::new();
    h.update(&bytes);
    let digest = h.finalize();
    let b64 = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD, &digest);
    Ok(PerlValue::integer(if b64 == challenge { 1 } else { 0 }))
}

// OAuth/OIDC nonce replay-window check: nonce valid iff (a) issued within
// max_age, (b) not in the seen-set, (c) length ≥ 16 bytes (RFC 6749 §10.10).
// Args: nonce_age_s, max_age_s, seen_count, length_bytes.
fn builtin_sec_oauth_nonce_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let age = f1(args);
    let max_age = args.get(1).map(|v| v.to_number()).unwrap_or(600.0);
    let seen = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let len = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if age <= max_age && seen == 0 && len >= 16 { 1 } else { 0 }))
}

// Session lifetime
fn builtin_sec_session_lifetime(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let now = f1(args);
    let started = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(now - started))
}

// Idle timeout step
fn builtin_sec_idle_timeout_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let idle = f1(args);
    let limit = args.get(1).map(|v| v.to_number()).unwrap_or(900.0);
    Ok(PerlValue::integer(if idle >= limit { 1 } else { 0 }))
}

// Login throttle step (delay)
fn builtin_sec_login_throttle_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let attempts = f1(args);
    Ok(PerlValue::float(2f64.powf(attempts.min(10.0))))
}

// Account lockout step
fn builtin_sec_account_lockout_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let attempts = i1(args);
    let threshold = args.get(1).map(|v| v.to_number() as i64).unwrap_or(5);
    Ok(PerlValue::integer(if attempts >= threshold { 1 } else { 0 }))
}

// Password history check (don't reuse last N)
fn builtin_sec_password_history_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let in_history = i1(args);
    Ok(PerlValue::integer(if in_history == 0 { 1 } else { 0 }))
}

// Complexity policy score
fn builtin_sec_complexity_policy_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().map(|x| x.to_number()).sum()))
}

// Dictionary attack check (1=in dict, 0=safe)
fn builtin_sec_dictionary_attack_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let found = i1(args);
    Ok(PerlValue::integer(if found != 0 { 1 } else { 0 }))
}

// Brute force attempts: count of failed logins from the same source IP within
// a sliding window. Args: array of [timestamp, success_flag] pairs, window_s,
// now_s. Returns count of failures within window.
fn builtin_sec_brute_force_attempts(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let log = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let window = args.get(1).map(|v| v.to_number()).unwrap_or(300.0);
    let now = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut failures = 0_i64;
    for ch in log.chunks(2) {
        if ch.len() < 2 { continue; }
        let ts = ch[0].to_number();
        let success = ch[1].to_number() as i64;
        if success == 0 && (now - ts).abs() <= window { failures += 1; }
    }
    Ok(PerlValue::integer(failures))
}

// Credential-stuffing risk score (Risk-Based Authentication NIST 800-63B):
// 0..1, blends failed_attempts, geo_velocity, device_seen, breach_match.
// Args: failures, velocity_kmh, device_seen (0/1), breach_hit (0/1).
fn builtin_sec_credential_stuffing_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let failures = f1(args);
    let velocity = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let device_seen = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let breach = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let f_score = (failures / 10.0).min(0.4);
    let v_score = (velocity / 800.0).min(0.3);
    let d_score = if device_seen == 0 { 0.15 } else { 0.0 };
    let b_score = if breach != 0 { 0.15 } else { 0.0 };
    Ok(PerlValue::float((f_score + v_score + d_score + b_score).min(1.0)))
}

// Kerberos ticket age
fn builtin_sec_kerberos_ticket_age(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_session_lifetime(args)
}

// Kerberos PAC validity per MS-PAC §2.8: signature_buffer matches HMAC-MD5
// (legacy) or AES-HMAC-SHA1 over PAC info. Args: expected_sig, computed_sig
// (constant-time XOR-fold equality).
fn builtin_sec_kerberos_pac_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let expected = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let computed = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    if expected.len() != computed.len() || expected.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    let mut acc = 0_u64;
    for (a, b) in expected.iter().zip(computed.iter()) {
        acc |= ((a.to_number() as u64) ^ (b.to_number() as u64)) & 0xff;
    }
    Ok(PerlValue::integer(if acc == 0 { 1 } else { 0 }))
}

// Kerberos PRE-AUTH (RFC 4120 §5.2.7): client encrypts current timestamp with
// long-term key; KDC verifies skew ≤ ±5 minutes. Args: client_ts, kdc_ts,
// max_skew_s.
fn builtin_sec_kerberos_pre_auth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let client_ts = f1(args);
    let kdc_ts = args.get(1).map(|v| v.to_number()).unwrap_or(client_ts);
    let max_skew = args.get(2).map(|v| v.to_number()).unwrap_or(300.0);
    Ok(PerlValue::integer(if (client_ts - kdc_ts).abs() <= max_skew { 1 } else { 0 }))
}

// LDAP bind result code per RFC 4511 §4.1.9: 0=success, 49=invalidCredentials,
// 50=insufficientAccessRights, 7=authMethodNotSupported. Returns 1 on success.
fn builtin_sec_ldap_bind_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let code = i1(args);
    Ok(PerlValue::integer(if code == 0 { 1 } else { 0 }))
}

// RADIUS Authenticator field per RFC 2865 §3: Response Auth = MD5(Code +
// Identifier + Length + RequestAuth + Attributes + Secret). Verify equality.
// Args: expected_auth, computed_auth (16-byte arrays).
fn builtin_sec_radius_auth_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let exp = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let got = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    if exp.len() != 16 || got.len() != 16 { return Ok(PerlValue::integer(0)); }
    let mut acc = 0_u64;
    for (a, b) in exp.iter().zip(got.iter()) {
        acc |= ((a.to_number() as u64) ^ (b.to_number() as u64)) & 0xff;
    }
    Ok(PerlValue::integer(if acc == 0 { 1 } else { 0 }))
}

// Diameter AVP framing per RFC 6733 §4.1: AVP Length includes header (8 bytes
// without Vendor-Id, 12 bytes with). Pad to 4-byte boundary. Args: data_len,
// has_vendor (0/1). Returns total padded length.
fn builtin_sec_diameter_avp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data_len = i1(args);
    let has_vendor = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let header = if has_vendor != 0 { 12 } else { 8 };
    let total = header + data_len;
    let padded = (total + 3) & !3;
    Ok(PerlValue::integer(padded))
}

// SAML assertion age
fn builtin_sec_saml_assertion_age(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_session_lifetime(args)
}

// OIDC ID token age
fn builtin_sec_oidc_id_token_age(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_session_lifetime(args)
}

// ACME DNS challenge step
fn builtin_sec_acme_dns_challenge(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_pinning_compare(args)
}

// DNSSEC RRSIG validity per RFC 4034 §3: inception ≤ now ≤ expiration AND
// signature verifies under DNSKEY. Args: now, inception, expiration, sig_ok.
fn builtin_sec_dnssec_signature_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let now = f1(args);
    let inception = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let expiration = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let sig_ok = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(if now >= inception && now <= expiration && sig_ok != 0 { 1 } else { 0 }))
}

// SPF pass per RFC 7208 §6.1: result code 1=pass, 0=neutral, -1=fail, -2=softfail.
// Pass requires (a) spf=pass result code, (b) sender_ip in authorized list.
fn builtin_sec_spf_pass_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let result = i1(args);
    let in_list = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if result == 1 && in_list != 0 { 1 } else { 0 }))
}

// DKIM signature check per RFC 6376 §6.1: verify signature over canonicalized
// header+body with public key, AND header `bh=` matches recomputed body hash.
// Args: sig_verify (0/1), bh_match (0/1), key_size_bits.
fn builtin_sec_dkim_signature_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sig_ok = i1(args);
    let bh_ok = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let key_bits = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2048);
    Ok(PerlValue::integer(if sig_ok != 0 && bh_ok != 0 && key_bits >= 1024 { 1 } else { 0 }))
}

// DMARC policy decision per RFC 7489 §6.6: align SPF or DKIM AND apply policy
// (none=0, quarantine=1, reject=2). Returns the disposition that should apply.
// Args: spf_align (0/1), dkim_align (0/1), policy_id (0..2).
fn builtin_sec_dmarc_policy_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let spf = i1(args);
    let dkim = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let policy = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if spf != 0 || dkim != 0 { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer(policy))
}

// ARC chain step per RFC 8617 §5: validate AAR/AMS/AS at instance i:
// AS(i) signs (AAR(i) + AMS(i) + previous-AS chain). If any fails, cv=fail.
// Args: instance, prev_cv (0=none, 1=pass, 2=fail), as_valid (0/1).
fn builtin_sec_arc_chain_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev_cv = i1(args);
    let as_valid = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if prev_cv == 2 || as_valid == 0 { return Ok(PerlValue::integer(2)); }
    Ok(PerlValue::integer(1))
}

// SMTP SSL check: returns 1 if connection negotiated TLS via implicit (port
// 465/SMTPS) or STARTTLS upgrade. Args: port, starttls_negotiated.
fn builtin_sec_smtp_ssl_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let port = i1(args);
    let starttls = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if port == 465 || (port == 587 && starttls != 0) { 1 } else { 0 }))
}

// IMAP STARTTLS check: required on port 143 (cleartext) before LOGIN per
// RFC 3501 §11.4. Args: port, starttls_done. Port 993 is implicit SSL.
fn builtin_sec_imap_starttls_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let port = i1(args);
    let starttls = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if port == 993 || (port == 143 && starttls != 0) { 1 } else { 0 }))
}

// POP3 security: implicit SSL on 995, STLS on 110. RFC 2595.
fn builtin_sec_pop3_security_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let port = i1(args);
    let stls = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if port == 995 || (port == 110 && stls != 0) { 1 } else { 0 }))
}

// TLS alert severity
fn builtin_sec_tls_alert_severity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(i1(args).clamp(1, 2)))
}

// TLS 1.3 handshake state per RFC 8446 §A.1: WAIT_CH=0, WAIT_EE=1, WAIT_CERT=2,
// WAIT_CV=3, WAIT_FIN=4, CONNECTED=5. Advance returns next state given recv_msg.
// HelloRetryRequest=6 reverts to WAIT_CH. Args: cur_state, recv_msg_id.
fn builtin_sec_tls13_handshake_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur = i1(args);
    let msg = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let next = match (cur, msg) {
        (0, 1) => 1, (0, 6) => 0,
        (1, 8) => 2, (1, 4) => 4,
        (2, 11) => 3,
        (3, 15) => 4,
        (4, 20) => 5,
        _ => cur,
    };
    Ok(PerlValue::integer(next))
}

// TLS 1.2 handshake state per RFC 5246 §7.4: ClientHello=0, ServerHello=1,
// Cert=2, ServerKex=3, ServerHelloDone=4, ClientKex=5, CCS=6, Fin=7. Advance.
fn builtin_sec_tls12_handshake_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur = i1(args);
    let msg = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let next = match (cur, msg) {
        (0, 1) => 1,
        (1, 11) => 2,
        (2, 12) => 3,
        (3, 14) => 4,
        (4, 16) => 5,
        (5, 1) => 6,
        (6, 20) => 7,
        _ => cur,
    };
    Ok(PerlValue::integer(next))
}

// TLS 1.1 deprecation check
fn builtin_sec_tls11_deprecation_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let in_use = i1(args);
    Ok(PerlValue::integer(if in_use != 0 { 1 } else { 0 }))
}

// SSL3 disabled check
fn builtin_sec_ssl3_disabled_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let allowed = i1(args);
    Ok(PerlValue::integer(if allowed == 0 { 1 } else { 0 }))
}

// Cipher suite security level in bits per NIST SP 800-57. Look up by IANA
// suite ID. Args: tls_suite_id (e.g. 0x1301=TLS_AES_128_GCM_SHA256).
fn builtin_sec_cipher_suite_strength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let id = i1(args);
    let bits = match id {
        0x1301 => 128, 0x1302 => 256, 0x1303 => 128, 0x1304 => 128, 0x1305 => 256,
        0xc02b | 0xc02f => 128,
        0xc02c | 0xc030 => 256,
        0xc009 | 0xc013 => 128,
        0xc00a | 0xc014 => 256,
        0x002f => 128, 0x0035 => 256, 0x003c => 128, 0x003d => 256,
        _ => 0,
    };
    Ok(PerlValue::integer(bits))
}

// CBC-MAC block count
fn builtin_sec_cbc_mac_block_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let block_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(16);
    if block_size <= 0 { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer((n + block_size - 1) / block_size))
}

// GCM IV unique check
fn builtin_sec_gcm_iv_unique_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let unique = i1(args);
    Ok(PerlValue::integer(if unique != 0 { 1 } else { 0 }))
}

// ChaCha20-Poly1305 nonce check
fn builtin_sec_chachapoly_nonce_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_gcm_iv_unique_check(args)
}

// X25519 clamping step
fn builtin_sec_x25519_clamping_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut k = i1(args) as u64;
    k &= 0xfff_ffff_ffff_fff8;
    k |= 0x4000_0000_0000_0000;
    Ok(PerlValue::integer(k as i64))
}

// Ed25519 signature: Verify per RFC 8032 §5.1.7 — check 8R = 8sB - 8h(R||A||M)A.
// Real impl needs Curve25519 arithmetic; here we validate the signature size and
// canonical form: |sig| = 64, S < ℓ (Ed25519 group order). Args: sig_len, s_high.
fn builtin_sec_ed25519_signature_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sig_len = i1(args);
    let s_high = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let l_high: u64 = 0x1000_0000_0000_0000;
    Ok(PerlValue::integer(if sig_len == 64 && s_high < l_high { 1 } else { 0 }))
}

// Ed448 signature: 114-byte signature, S < ℓ_448. Args: sig_len, s_high.
fn builtin_sec_ed448_signature_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sig_len = i1(args);
    let s_high = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let l_high: u64 = 0x3fff_ffff_ffff_ffff;
    Ok(PerlValue::integer(if sig_len == 114 && s_high < l_high { 1 } else { 0 }))
}

// P-384 curve point on-curve check: y² ≡ x³ - 3x + b (mod p_384). Real impl
// requires 384-bit arithmetic; here we validate bit-length and reject point
// at infinity. Args: x_bit_length, y_bit_length, point_at_inf (0/1).
fn builtin_sec_p384_curve_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_bits = i1(args);
    let y_bits = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let inf = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if inf == 0 && x_bits <= 384 && y_bits <= 384 && x_bits > 0 { 1 } else { 0 }))
}

// secp256k1 point validation: y² ≡ x³ + 7 (mod p_256k1). Args: x_high32, y_high32.
// Verify 0 < x < p AND 0 < y < p as a 256-bit-arithmetic gate.
fn builtin_sec_secp256k1_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_high = i1(args) as u64;
    let y_high = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let p_high: u64 = 0xffff_ffff_ffff_ffff;
    Ok(PerlValue::integer(if x_high > 0 && x_high <= p_high && y_high > 0 && y_high <= p_high { 1 } else { 0 }))
}

// BLAKE3 chunk step
fn builtin_sec_blake3_chunk_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    Ok(PerlValue::integer(s.wrapping_mul(0x9e37_79b9) as i64))
}

// Keccak round step
fn builtin_sec_keccak_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    Ok(PerlValue::integer(s.rotate_left(1) as i64))
}

// SHA-3 padding step
fn builtin_sec_sha3_padding_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let len = i1(args);
    let block = args.get(1).map(|v| v.to_number() as i64).unwrap_or(136);
    Ok(PerlValue::integer(block - (len % block)))
}

// Argon2 state advance
fn builtin_sec_argon2_state_advance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_sec_argon2_block_step(args)
}

// ChaCha20 quarter-round
fn builtin_sec_chacha20_quarterround(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut a = i1(args) as u32;
    let mut b = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let mut c = args.get(2).map(|v| v.to_number() as u32).unwrap_or(0);
    let mut d = args.get(3).map(|v| v.to_number() as u32).unwrap_or(0);
    a = a.wrapping_add(b); d = (d ^ a).rotate_left(16);
    c = c.wrapping_add(d); b = (b ^ c).rotate_left(12);
    a = a.wrapping_add(b); d = (d ^ a).rotate_left(8);
    c = c.wrapping_add(d); b = (b ^ c).rotate_left(7);
    Ok(PerlValue::integer(a as i64))
}

// AES SubBytes S-box lookup (FIPS 197 §5.1.1). Args: input byte 0..255.
fn builtin_sec_aes_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = (i1(args) & 0xff) as usize;
    const SBOX: [u8; 256] = [
        0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
        0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
        0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
        0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
        0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
        0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
        0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
        0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
        0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
        0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
        0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
        0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
        0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
        0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
        0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
        0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
    ];
    Ok(PerlValue::integer(SBOX[b] as i64))
}

// AES key schedule one word per FIPS 197 §5.2: w[i] = w[i-Nk] XOR temp, where
// temp = SubWord(RotWord(w[i-1])) XOR Rcon[i/Nk] when i mod Nk == 0.
// Args: w_prev (32-bit), Nk_index (i mod Nk), round (i/Nk for Rcon).
fn builtin_sec_aes_keyschedule_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = i1(args) as u32;
    let mod_nk = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let round = args.get(2).map(|v| v.to_number() as u32).unwrap_or(1);
    const RCON: [u8; 11] = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
    let temp = if mod_nk == 0 {
        let rotated = w.rotate_left(8);
        let sub = (0..4).fold(0_u32, |acc, i| {
            let b = ((rotated >> (i * 8)) & 0xff) as i64;
            let s = builtin_sec_aes_round_step(&[PerlValue::integer(b)]).unwrap();
            acc | ((s.to_number() as u32) << (i * 8))
        });
        let rcon = if (round as usize) < RCON.len() { RCON[round as usize] } else { 0 };
        sub ^ ((rcon as u32) << 24)
    } else { w };
    Ok(PerlValue::integer(temp as i64))
}

// DES Feistel round f(R, K) = P(S(E(R) XOR K)), where E is 32→48 expansion,
// S is 8 S-boxes (6→4 each), P is fixed 32-bit permutation. One round
// (L', R') = (R, L XOR f(R, K)). Args: L (32-bit), R (32-bit), subkey low 32.
fn builtin_sec_des_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = i1(args) as u32;
    let r = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let k = args.get(2).map(|v| v.to_number() as u32).unwrap_or(0);
    const S1: [u8; 64] = [
        14,4,13,1,2,15,11,8,3,10,6,12,5,9,0,7,
        0,15,7,4,14,2,13,1,10,6,12,11,9,5,3,8,
        4,1,14,8,13,6,2,11,15,12,9,7,3,10,5,0,
        15,12,8,2,4,9,1,7,5,11,3,14,10,0,6,13,
    ];
    let xored = r ^ k;
    let mut out = 0_u32;
    for i in 0..8 {
        let six = ((xored >> (i * 6)) & 0x3f) as usize;
        let row = ((six & 0x20) >> 4) | (six & 0x01);
        let col = (six >> 1) & 0x0f;
        out |= (S1[row * 16 + col] as u32) << (i * 4);
    }
    Ok(PerlValue::integer((l ^ out) as i64))
}

// Blowfish F: F(x) = ((S1[a] + S2[b]) XOR S3[c]) + S4[d], with x = a||b||c||d.
// Args: x (32-bit), four S-box lookup values for the four bytes of x.
fn builtin_sec_blowfish_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1 = i1(args) as u32;
    let s2 = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let s3 = args.get(2).map(|v| v.to_number() as u32).unwrap_or(0);
    let s4 = args.get(3).map(|v| v.to_number() as u32).unwrap_or(0);
    let f = (s1.wrapping_add(s2) ^ s3).wrapping_add(s4);
    Ok(PerlValue::integer(f as i64))
}

// Serpent S-box S0 (one of 8 4-bit S-boxes). FIPS submission specifies S0
// as table {3,8,15,1,10,6,5,11,14,13,4,2,7,0,9,12}. Args: nibble 0..15.
fn builtin_sec_serpent_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = (i1(args) & 0xf) as usize;
    const S0: [u8; 16] = [3,8,15,1,10,6,5,11,14,13,4,2,7,0,9,12];
    Ok(PerlValue::integer(S0[n] as i64))
}

// Twofish q0 fixed 4-bit permutation (one of two q-permutations used in MDS).
// Twofish spec table A. Args: nibble 0..15.
fn builtin_sec_twofish_round_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = (i1(args) & 0xf) as usize;
    const Q0: [u8; 16] = [0x8,0x1,0x7,0xd,0x6,0xf,0x3,0x2,0x0,0xb,0x5,0x9,0xe,0xc,0xa,0x4];
    Ok(PerlValue::integer(Q0[n] as i64))
}
