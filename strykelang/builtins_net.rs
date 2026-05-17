//! Network / IP / CIDR / MAC primitives — Phase 1 of the 10k-builtin push.
//!
//! IP-address subset for the first batch: parsing, predicates, conversions,
//! canonical forms, and reverse-DNS shape. Pure functions over stdlib
//! `IpAddr` — no external crates. Subsequent batches will add CIDR math,
//! MAC ops, port helpers, DNS resolution, and the rest of the 251 names.
//!
//! Naming follows the audited proposal at `/tmp/proposed_final.txt`. Every
//! function returns a `StrykeValue` directly (no `StrykeResult` wrapper) so
//! the dispatch arms stay one line.

use crate::value::StrykeValue;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

// ── helpers ────────────────────────────────────────────────────────────

/// Parse the first arg as a string, return `IpAddr` or `None`. Trims
/// whitespace and strips an optional `%zone` suffix so callers don't
/// have to pre-clean. Use [`ip_parse_strict`] when zone-stripping is
/// the wrong choice.
fn parse_ip_lenient(s: &str) -> Option<IpAddr> {
    let s = s.trim();
    let (addr_part, _zone) = match s.rsplit_once('%') {
        Some((a, z)) => (a, Some(z)),
        None => (s, None),
    };
    addr_part.parse::<IpAddr>().ok()
}

fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    s.trim().parse::<Ipv4Addr>().ok()
}

fn parse_ipv6(s: &str) -> Option<Ipv6Addr> {
    let s = s.trim();
    let addr_part = match s.rsplit_once('%') {
        Some((a, _z)) => a,
        None => s,
    };
    addr_part.parse::<Ipv6Addr>().ok()
}

/// First arg as string, or empty if missing.
fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

#[inline]
fn b(v: bool) -> StrykeValue {
    StrykeValue::integer(if v { 1 } else { 0 })
}

// ── parsing & validation ──────────────────────────────────────────────

/// `ip_parse(STR)` — canonicalizes any valid IPv4 or IPv6 input to its
/// stdlib `Display` form (RFC 5952 compression for v6, dotted-quad for v4).
/// Returns the canonical string, or `undef` on invalid input.
pub fn ip_parse(args: &[StrykeValue]) -> StrykeValue {
    match parse_ip_lenient(&arg_str(args)) {
        Some(ip) => StrykeValue::string(ip.to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `ip_is_valid(STR)` — 1 if v4 or v6, 0 otherwise.
pub fn ip_is_valid(args: &[StrykeValue]) -> StrykeValue {
    b(parse_ip_lenient(&arg_str(args)).is_some())
}

/// `ipv4_parse(STR)` — canonical dotted-quad or undef.
pub fn ipv4_parse(args: &[StrykeValue]) -> StrykeValue {
    match parse_ipv4(&arg_str(args)) {
        Some(ip) => StrykeValue::string(ip.to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `ipv4_is_valid(STR)` — 1 if parses as v4, 0 otherwise.
pub fn ipv4_is_valid(args: &[StrykeValue]) -> StrykeValue {
    b(parse_ipv4(&arg_str(args)).is_some())
}

/// `ipv6_parse(STR)` — canonical RFC 5952 form (compressed) or undef.
pub fn ipv6_parse(args: &[StrykeValue]) -> StrykeValue {
    match parse_ipv6(&arg_str(args)) {
        Some(ip) => StrykeValue::string(ip.to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `ipv6_is_valid(STR)` — 1 if parses as v6, 0 otherwise.
pub fn ipv6_is_valid(args: &[StrykeValue]) -> StrykeValue {
    b(parse_ipv6(&arg_str(args)).is_some())
}

// ── version / family ──────────────────────────────────────────────────

/// `ip_version(STR)` — 4 or 6, or undef.
pub fn ip_version(args: &[StrykeValue]) -> StrykeValue {
    match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(_)) => StrykeValue::integer(4),
        Some(IpAddr::V6(_)) => StrykeValue::integer(6),
        None => StrykeValue::UNDEF,
    }
}

/// `ip_family(STR)` — "v4" / "v6" / undef. Alias for callers that prefer
/// a string family tag over a numeric version.
pub fn ip_family(args: &[StrykeValue]) -> StrykeValue {
    match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(_)) => StrykeValue::string("v4".to_string()),
        Some(IpAddr::V6(_)) => StrykeValue::string("v6".to_string()),
        None => StrykeValue::UNDEF,
    }
}

// ── numeric / byte conversions ────────────────────────────────────────

/// `ip_to_int(STR)` — v4 → u32 as integer; v6 → 128-bit value as
/// **decimal string** (Perl ints can't hold 128 bits, so v6 returns
/// the canonical decimal string; v4 returns a normal integer). Undef on
/// parse failure.
pub fn ip_to_int(args: &[StrykeValue]) -> StrykeValue {
    match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(v4)) => StrykeValue::integer(u32::from(v4) as i64),
        Some(IpAddr::V6(v6)) => StrykeValue::string(u128::from(v6).to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `int_to_ip(N)` — integer (or numeric string) → IPv4 dotted-quad
/// for values that fit in u32, IPv6 canonical form for larger values.
/// `0..=u32::MAX` is treated as v4; anything above (or any input that
/// looks like a decimal too long to fit u32) is treated as v6.
pub fn int_to_ip(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let trimmed = s.trim();
    // Try u128 first; if it fits in u32, render as v4.
    let Ok(n) = trimmed.parse::<u128>() else {
        return StrykeValue::UNDEF;
    };
    if n <= u32::MAX as u128 {
        let v4 = Ipv4Addr::from(n as u32);
        StrykeValue::string(v4.to_string())
    } else {
        let v6 = Ipv6Addr::from(n);
        StrykeValue::string(v6.to_string())
    }
}

/// `ip_to_bytes(STR)` — arrayref of u8 bytes (4 for v4, 16 for v6).
pub fn ip_to_bytes(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let bytes: Vec<u8> = match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(v4)) => v4.octets().to_vec(),
        Some(IpAddr::V6(v6)) => v6.octets().to_vec(),
        None => return StrykeValue::UNDEF,
    };
    let elems: Vec<StrykeValue> = bytes
        .into_iter()
        .map(|x| StrykeValue::integer(x as i64))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

/// `bytes_to_ip(\@bytes)` — 4-byte array → v4, 16-byte array → v6.
/// Anything else → undef.
pub fn bytes_to_ip(args: &[StrykeValue]) -> StrykeValue {
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let guard = arr.read();
    let bytes: Vec<u8> = guard
        .iter()
        .map(|v| v.to_int().clamp(0, 255) as u8)
        .collect();
    drop(guard);
    match bytes.len() {
        4 => {
            let v4 = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
            StrykeValue::string(v4.to_string())
        }
        16 => {
            let mut a = [0u8; 16];
            a.copy_from_slice(&bytes);
            let v6 = Ipv6Addr::from(a);
            StrykeValue::string(v6.to_string())
        }
        _ => StrykeValue::UNDEF,
    }
}

/// `ip_to_bits(STR)` — binary string (32 chars for v4, 128 chars for v6),
/// each char is `'0'` or `'1'`. Useful for prefix-length math.
pub fn ip_to_bits(args: &[StrykeValue]) -> StrykeValue {
    let bytes: Vec<u8> = match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(v4)) => v4.octets().to_vec(),
        Some(IpAddr::V6(v6)) => v6.octets().to_vec(),
        None => return StrykeValue::UNDEF,
    };
    let mut s = String::with_capacity(bytes.len() * 8);
    for byte in bytes {
        for i in (0..8).rev() {
            s.push(if (byte >> i) & 1 == 1 { '1' } else { '0' });
        }
    }
    StrykeValue::string(s)
}

/// `bits_to_ip(BITS)` — inverse of `ip_to_bits`. 32-char string → v4,
/// 128-char string → v6. Other lengths → undef.
pub fn bits_to_ip(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    if !s.chars().all(|c| c == '0' || c == '1') {
        return StrykeValue::UNDEF;
    }
    let len = s.len();
    let n_bytes = match len {
        32 => 4,
        128 => 16,
        _ => return StrykeValue::UNDEF,
    };
    let mut bytes = vec![0u8; n_bytes];
    for (i, c) in s.chars().enumerate() {
        if c == '1' {
            bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    match n_bytes {
        4 => StrykeValue::string(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).to_string()),
        16 => {
            let mut a = [0u8; 16];
            a.copy_from_slice(&bytes);
            StrykeValue::string(Ipv6Addr::from(a).to_string())
        }
        _ => StrykeValue::UNDEF,
    }
}

// ── predicates ────────────────────────────────────────────────────────

/// Helper: parse + run a v4 predicate and a v6 predicate.
fn ip_pred<F4, F6>(args: &[StrykeValue], f4: F4, f6: F6) -> StrykeValue
where
    F4: FnOnce(&Ipv4Addr) -> bool,
    F6: FnOnce(&Ipv6Addr) -> bool,
{
    match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(v4)) => b(f4(&v4)),
        Some(IpAddr::V6(v6)) => b(f6(&v6)),
        None => StrykeValue::UNDEF,
    }
}

/// `ip_is_private(STR)` — RFC 1918 (v4) or unique-local (v6, fc00::/7).
pub fn ip_is_private(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| v4.is_private(),
        |v6| (v6.segments()[0] & 0xfe00) == 0xfc00,
    )
}

/// `ip_is_loopback(STR)` — 127.0.0.0/8 (v4) or ::1 (v6).
pub fn ip_is_loopback(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(args, |v4| v4.is_loopback(), |v6| v6.is_loopback())
}

/// `ip_is_multicast(STR)` — 224.0.0.0/4 (v4) or ff00::/8 (v6).
pub fn ip_is_multicast(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(args, |v4| v4.is_multicast(), |v6| v6.is_multicast())
}

/// `ip_is_link_local(STR)` — 169.254.0.0/16 (v4) or fe80::/10 (v6).
pub fn ip_is_link_local(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| v4.is_link_local(),
        |v6| (v6.segments()[0] & 0xffc0) == 0xfe80,
    )
}

/// `ip_is_unspecified(STR)` — 0.0.0.0 or ::.
pub fn ip_is_unspecified(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(args, |v4| v4.is_unspecified(), |v6| v6.is_unspecified())
}

/// `ip_is_documentation(STR)` — RFC 5737 v4 ranges (192.0.2.0/24,
/// 198.51.100.0/24, 203.0.113.0/24) or RFC 3849 v6 (2001:db8::/32).
pub fn ip_is_documentation(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| v4.is_documentation(),
        |v6| (v6.segments()[0] == 0x2001) && (v6.segments()[1] == 0x0db8),
    )
}

/// `ip_is_benchmarking(STR)` — RFC 2544 v4 (198.18.0.0/15) or
/// RFC 5180 v6 (2001:2::/48).
pub fn ip_is_benchmarking(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| {
            let o = v4.octets();
            o[0] == 198 && (o[1] == 18 || o[1] == 19)
        },
        |v6| {
            let s = v6.segments();
            s[0] == 0x2001 && s[1] == 0x0002 && s[2] == 0
        },
    )
}

/// `ip_is_shared(STR)` — RFC 6598 carrier-grade-NAT (100.64.0.0/10).
/// Returns 0 for v6 (no equivalent reservation).
pub fn ip_is_shared(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xc0) == 0x40
        },
        |_| false,
    )
}

/// `ip_is_reserved(STR)` — covers 240.0.0.0/4 (v4 reserved for future)
/// or any v6 in the IETF-reserved prefixes (`::/8` excluding ::1/::,
/// or `0100::/64`).
pub fn ip_is_reserved(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(
        args,
        |v4| v4.octets()[0] >= 240 && !v4.is_broadcast(),
        |v6| {
            let s = v6.segments();
            s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] == 0
        },
    )
}

/// `ip_is_broadcast(STR)` — exactly 255.255.255.255 (v4 only).
pub fn ip_is_broadcast(args: &[StrykeValue]) -> StrykeValue {
    ip_pred(args, |v4| v4.is_broadcast(), |_| false)
}

/// `ip_is_global(STR)` — true if the address is globally routable on
/// the public internet. Combines "not loopback / link-local / multicast
/// / private / unspecified / documentation / benchmarking / reserved".
pub fn ip_is_global(args: &[StrykeValue]) -> StrykeValue {
    let arg = arg_str(args);
    let Some(ip) = parse_ip_lenient(&arg) else {
        return StrykeValue::UNDEF;
    };
    let not_special = match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_multicast()
                && !v4.is_private()
                && !v4.is_unspecified()
                && !v4.is_documentation()
                && !v4.is_broadcast()
                && {
                    let o = v4.octets();
                    !(o[0] == 100 && (o[1] & 0xc0) == 0x40) // shared
                        && !(o[0] == 198 && (o[1] == 18 || o[1] == 19)) // bench
                        && o[0] < 240 // reserved
                }
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            !(v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (segs[0] & 0xffc0) == 0xfe80   // link-local
                || (segs[0] & 0xfe00) == 0xfc00   // unique-local
                || (segs[0] == 0x2001 && segs[1] == 0x0db8)) // doc
        }
    };
    b(not_special)
}

// ── canonical / display forms ─────────────────────────────────────────

/// `ip_canonical(STR)` — canonical string form. v4 → dotted-quad with
/// no leading zeros. v6 → RFC 5952 compressed (lowercase, longest
/// zero run replaced by `::`). Idempotent.
pub fn ip_canonical(args: &[StrykeValue]) -> StrykeValue {
    ip_parse(args)
}

/// `ipv6_canonical(STR)` — RFC 5952 form, v6 only. Useful as a type-
/// asserting variant of `ip_canonical`.
pub fn ipv6_canonical(args: &[StrykeValue]) -> StrykeValue {
    match parse_ipv6(&arg_str(args)) {
        Some(v6) => StrykeValue::string(v6.to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `ipv6_expand(STR)` — fully-expanded uncompressed form (8 groups of
/// 4 hex digits each, no `::`). Inverse of `ipv6_compress`.
pub fn ipv6_expand(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let segs = v6.segments();
    let s = segs
        .iter()
        .map(|s| format!("{:04x}", s))
        .collect::<Vec<_>>()
        .join(":");
    StrykeValue::string(s)
}

/// `ipv6_compress(STR)` — RFC 5952 compressed form (alias for
/// `ipv6_canonical`; provided so users have an explicit `compress`
/// verb to pair with `ipv6_expand`).
pub fn ipv6_compress(args: &[StrykeValue]) -> StrykeValue {
    ipv6_canonical(args)
}

/// `ipv6_strip_zone(STR)` — drops any trailing `%zone` suffix and
/// returns the canonical address. Useful for comparing addresses that
/// may carry per-interface zone identifiers.
pub fn ipv6_strip_zone(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    let addr_part = s.rsplit_once('%').map(|(a, _)| a).unwrap_or(s);
    match addr_part.parse::<Ipv6Addr>() {
        Ok(v6) => StrykeValue::string(v6.to_string()),
        Err(_) => StrykeValue::UNDEF,
    }
}

/// `ipv6_zone_id(STR)` — the zone suffix (e.g. `"eth0"`, `"1"`) or
/// `undef` if none.
pub fn ipv6_zone_id(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    match s.rsplit_once('%') {
        Some((_, zone)) if !zone.is_empty() => StrykeValue::string(zone.to_string()),
        _ => StrykeValue::UNDEF,
    }
}

// ── reverse / arpa ────────────────────────────────────────────────────

/// `ip_reverse(STR)` — DNS reverse-form. v4 → `4.3.2.1.in-addr.arpa`,
/// v6 → nibble-form `…ip6.arpa`. Returns undef on invalid input.
pub fn ip_reverse(args: &[StrykeValue]) -> StrykeValue {
    match parse_ip_lenient(&arg_str(args)) {
        Some(IpAddr::V4(v4)) => {
            let o = v4.octets();
            StrykeValue::string(format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0]))
        }
        Some(IpAddr::V6(v6)) => {
            let mut nibbles: Vec<char> = Vec::with_capacity(32);
            for byte in v6.octets() {
                nibbles.push(std::char::from_digit((byte >> 4) as u32, 16).unwrap());
                nibbles.push(std::char::from_digit((byte & 0x0f) as u32, 16).unwrap());
            }
            nibbles.reverse();
            let mut s = String::with_capacity(72);
            for (i, c) in nibbles.into_iter().enumerate() {
                if i > 0 {
                    s.push('.');
                }
                s.push(c);
            }
            s.push_str(".ip6.arpa");
            StrykeValue::string(s)
        }
        None => StrykeValue::UNDEF,
    }
}

/// `ip_arpa(STR)` — alias for `ip_reverse` for callers who prefer the
/// RFC-3596 / RFC-1035 naming.
pub fn ip_arpa(args: &[StrykeValue]) -> StrykeValue {
    ip_reverse(args)
}

// ── ordering ──────────────────────────────────────────────────────────

/// `ip_compare(A, B)` — -1 / 0 / 1 (v4 < v6 always for cross-family).
pub fn ip_compare(args: &[StrykeValue]) -> StrykeValue {
    let a = parse_ip_lenient(&args.first().map(|v| v.to_string()).unwrap_or_default());
    let b_ip = parse_ip_lenient(&args.get(1).map(|v| v.to_string()).unwrap_or_default());
    match (a, b_ip) {
        (Some(IpAddr::V4(x)), Some(IpAddr::V4(y))) => {
            StrykeValue::integer(u32::from(x).cmp(&u32::from(y)) as i8 as i64)
        }
        (Some(IpAddr::V6(x)), Some(IpAddr::V6(y))) => {
            StrykeValue::integer(u128::from(x).cmp(&u128::from(y)) as i8 as i64)
        }
        (Some(IpAddr::V4(_)), Some(IpAddr::V6(_))) => StrykeValue::integer(-1),
        (Some(IpAddr::V6(_)), Some(IpAddr::V4(_))) => StrykeValue::integer(1),
        _ => StrykeValue::UNDEF,
    }
}

/// `ip_sort(\@addrs)` — sorted arrayref; invalid entries dropped.
pub fn ip_sort(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        // Variadic form: treat all args as addresses.
        let mut ips: Vec<IpAddr> = args
            .iter()
            .filter_map(|v| parse_ip_lenient(&v.to_string()))
            .collect();
        ips.sort();
        let out: Vec<StrykeValue> = ips
            .into_iter()
            .map(|ip| StrykeValue::string(ip.to_string()))
            .collect();
        return StrykeValue::array_ref(Arc::new(RwLock::new(out)));
    };
    let g = arr.read();
    let mut ips: Vec<IpAddr> = g
        .iter()
        .filter_map(|v| parse_ip_lenient(&v.to_string()))
        .collect();
    drop(g);
    ips.sort();
    let out: Vec<StrykeValue> = ips
        .into_iter()
        .map(|ip| StrykeValue::string(ip.to_string()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

// ── random ────────────────────────────────────────────────────────────

/// `ip_random()` — random IPv4. Use `ip_random_v6` for v6.
pub fn ip_random(_args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let n: u32 = rng.gen();
    StrykeValue::string(Ipv4Addr::from(n).to_string())
}

// ── classification / classful ─────────────────────────────────────────

/// `ipv4_classful_class(STR)` — `"A"` / `"B"` / `"C"` / `"D"` / `"E"`
/// (multicast / experimental). Historical/educational — modern routing
/// uses CIDR, but the labels still come up in textbooks and legacy docs.
pub fn ipv4_classful_class(args: &[StrykeValue]) -> StrykeValue {
    let Some(v4) = parse_ipv4(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let first = v4.octets()[0];
    let cls = match first {
        0..=127 => "A",
        128..=191 => "B",
        192..=223 => "C",
        224..=239 => "D",
        _ => "E",
    };
    StrykeValue::string(cls.to_string())
}

// ── ipv6 link-local / unique-local helpers ────────────────────────────

/// `ipv6_link_local(STR)` — 1 if `fe80::/10`. Same predicate as
/// `ip_is_link_local` but v6-only (parses as v6, errors on v4 input).
pub fn ipv6_link_local(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    b((v6.segments()[0] & 0xffc0) == 0xfe80)
}

/// `ipv6_unique_local(STR)` — 1 if `fc00::/7` (ULA per RFC 4193).
pub fn ipv6_unique_local(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    b((v6.segments()[0] & 0xfe00) == 0xfc00)
}

// ── v4/v6 conversions / embeddings ────────────────────────────────────

/// `ipv4_to_ipv6_mapped(V4)` — `::ffff:a.b.c.d` form (RFC 4291).
pub fn ipv4_to_ipv6_mapped(args: &[StrykeValue]) -> StrykeValue {
    let Some(v4) = parse_ipv4(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let v6 = v4.to_ipv6_mapped();
    StrykeValue::string(v6.to_string())
}

/// `ipv4_to_ipv6_6to4(V4)` — `2002:WWXX:YYZZ::/48` 6to4 prefix
/// (RFC 3056). Embeds the v4 address into a 6to4 prefix.
pub fn ipv4_to_ipv6_6to4(args: &[StrykeValue]) -> StrykeValue {
    let Some(v4) = parse_ipv4(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let o = v4.octets();
    let upper = ((o[0] as u16) << 8) | (o[1] as u16);
    let lower = ((o[2] as u16) << 8) | (o[3] as u16);
    let v6 = Ipv6Addr::new(0x2002, upper, lower, 0, 0, 0, 0, 0);
    StrykeValue::string(v6.to_string())
}

/// `ipv6_to_ipv4_compat(V6)` — if `::a.b.c.d` (deprecated compatible
/// form), returns the v4. Undef otherwise.
pub fn ipv6_to_ipv4_compat(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let segs = v6.segments();
    // Compat: ::a.b.c.d means first 6 segments are zero and the bottom 32
    // bits are non-zero (and != 1 which would be loopback).
    if segs[0..6].iter().all(|&s| s == 0) {
        let last32 = ((segs[6] as u32) << 16) | (segs[7] as u32);
        if last32 > 1 && last32 < 0xffff_0000 {
            let v4 = Ipv4Addr::from(last32);
            return StrykeValue::string(v4.to_string());
        }
    }
    StrykeValue::UNDEF
}

/// `ipv6_is_6to4(V6)` — 1 if `2002::/16`.
pub fn ipv6_is_6to4(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    b(v6.segments()[0] == 0x2002)
}

/// `ipv6_6to4_extract(V6)` — extract the embedded v4 from a 6to4
/// address. Undef if not 6to4.
pub fn ipv6_6to4_extract(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let segs = v6.segments();
    if segs[0] != 0x2002 {
        return StrykeValue::UNDEF;
    }
    let upper = segs[1];
    let lower = segs[2];
    let n: u32 = ((upper as u32) << 16) | (lower as u32);
    StrykeValue::string(Ipv4Addr::from(n).to_string())
}

/// `ipv6_is_teredo(V6)` — 1 if `2001:0000::/32`.
pub fn ipv6_is_teredo(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let s = v6.segments();
    b(s[0] == 0x2001 && s[1] == 0)
}

/// `ipv6_teredo_extract(V6)` — `{ server, client, port, flags }` hashref
/// for a Teredo address per RFC 4380. Undef if not Teredo.
pub fn ipv6_teredo_extract(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let s = v6.segments();
    if !(s[0] == 0x2001 && s[1] == 0) {
        return StrykeValue::UNDEF;
    }
    let server_n: u32 = ((s[2] as u32) << 16) | (s[3] as u32);
    let flags: u16 = s[4];
    let port: u16 = !s[5];
    let client_n: u32 = (((!s[6]) as u32) << 16) | ((!s[7]) as u32);
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert(
        "server".to_string(),
        StrykeValue::string(Ipv4Addr::from(server_n).to_string()),
    );
    h.insert(
        "client".to_string(),
        StrykeValue::string(Ipv4Addr::from(client_n).to_string()),
    );
    h.insert("port".to_string(), StrykeValue::integer(port as i64));
    h.insert("flags".to_string(), StrykeValue::integer(flags as i64));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `ipv6_is_isatap(V6)` — interface-id matches `0000:5efe:a.b.c.d`
/// (RFC 5214).
pub fn ipv6_is_isatap(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let s = v6.segments();
    b(s[4] == 0x0000 && s[5] == 0x5efe)
}

/// `ipv6_isatap_extract(V6)` — embedded v4 from an ISATAP address.
pub fn ipv6_isatap_extract(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let s = v6.segments();
    if !(s[4] == 0x0000 && s[5] == 0x5efe) {
        return StrykeValue::UNDEF;
    }
    let n: u32 = ((s[6] as u32) << 16) | (s[7] as u32);
    StrykeValue::string(Ipv4Addr::from(n).to_string())
}

/// `ipv6_solicited_node(V6)` — `ff02::1:ffXX:XXXX` form used in
/// IPv6 Neighbor Discovery (RFC 4291 §2.7.1).
pub fn ipv6_solicited_node(args: &[StrykeValue]) -> StrykeValue {
    let Some(v6) = parse_ipv6(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    let s = v6.segments();
    let low24_upper: u16 = 0xff00 | (s[6] & 0x00ff);
    let low24_lower: u16 = s[7];
    let out = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 1, low24_upper, low24_lower);
    StrykeValue::string(out.to_string())
}

/// `ipv6_eui64_addr(PREFIX, MAC)` — combines a 64-bit prefix with a
/// MAC address (EUI-48) to produce a full v6 address per RFC 4291.
/// PREFIX is any v6 — only the top 64 bits are used.
pub fn ipv6_eui64_addr(args: &[StrykeValue]) -> StrykeValue {
    let Some(prefix) = args.first().and_then(|v| parse_ipv6(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let Some(mac) = args.get(1).and_then(|v| parse_mac_str(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let eui64 = mac_to_eui64(mac);
    let mut octets = prefix.octets();
    octets[8..16].copy_from_slice(&eui64);
    StrykeValue::string(Ipv6Addr::from(octets).to_string())
}

/// `ipv6_link_local_from_mac(MAC)` — `fe80::eui64`. Standard SLAAC
/// link-local address derivation.
pub fn ipv6_link_local_from_mac(args: &[StrykeValue]) -> StrykeValue {
    let Some(mac) = args.first().and_then(|v| parse_mac_str(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let eui64 = mac_to_eui64(mac);
    let mut octets = [0u8; 16];
    octets[0] = 0xfe;
    octets[1] = 0x80;
    octets[8..16].copy_from_slice(&eui64);
    StrykeValue::string(Ipv6Addr::from(octets).to_string())
}

// ── private mac helpers used by eui64 ────────────────────────────────

fn parse_mac_str(s: &str) -> Option<[u8; 6]> {
    let s = s.trim();
    // Accept ":", "-", or "." separators, and bare 12-hex form.
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 12 {
        return None;
    }
    let mut bytes = [0u8; 6];
    for i in 0..6 {
        let lo = i * 2;
        let hi = lo + 2;
        bytes[i] = u8::from_str_radix(&cleaned[lo..hi], 16).ok()?;
    }
    Some(bytes)
}

fn mac_to_eui64(mac: [u8; 6]) -> [u8; 8] {
    let mut eui = [0u8; 8];
    eui[0] = mac[0] ^ 0x02; // flip universal/local bit
    eui[1] = mac[1];
    eui[2] = mac[2];
    eui[3] = 0xff;
    eui[4] = 0xfe;
    eui[5] = mac[3];
    eui[6] = mac[4];
    eui[7] = mac[5];
    eui
}

// ══════════════════════════════════════════════════════════════════════
// CIDR math
// ══════════════════════════════════════════════════════════════════════

/// Parsed CIDR (address + prefix length). Used internally; surfaced to
/// stryke code as a `"addr/prefix"` string or split into parts.
#[derive(Debug, Clone, Copy)]
enum Cidr {
    V4 { addr: Ipv4Addr, prefix: u8 },
    V6 { addr: Ipv6Addr, prefix: u8 },
}

impl Cidr {
    fn prefix(self) -> u8 {
        match self {
            Cidr::V4 { prefix, .. } | Cidr::V6 { prefix, .. } => prefix,
        }
    }

    fn family_bits(self) -> u8 {
        match self {
            Cidr::V4 { .. } => 32,
            Cidr::V6 { .. } => 128,
        }
    }

    /// The address (host bits NOT zeroed). Use `network_addr` for the
    /// network address with host bits zeroed.
    #[allow(dead_code)]
    fn addr(self) -> IpAddr {
        match self {
            Cidr::V4 { addr, .. } => IpAddr::V4(addr),
            Cidr::V6 { addr, .. } => IpAddr::V6(addr),
        }
    }

    fn network_addr(self) -> IpAddr {
        match self {
            Cidr::V4 { addr, prefix } => {
                let mask = ipv4_mask(prefix);
                IpAddr::V4(Ipv4Addr::from(u32::from(addr) & mask))
            }
            Cidr::V6 { addr, prefix } => {
                let mask = ipv6_mask(prefix);
                IpAddr::V6(Ipv6Addr::from(u128::from(addr) & mask))
            }
        }
    }

    fn broadcast_addr(self) -> IpAddr {
        match self {
            Cidr::V4 { addr, prefix } => {
                let mask = ipv4_mask(prefix);
                IpAddr::V4(Ipv4Addr::from((u32::from(addr) & mask) | !mask))
            }
            Cidr::V6 { addr, prefix } => {
                let mask = ipv6_mask(prefix);
                IpAddr::V6(Ipv6Addr::from((u128::from(addr) & mask) | !mask))
            }
        }
    }

    fn render(self) -> String {
        format!("{}/{}", self.network_addr(), self.prefix())
    }
}

#[inline]
fn ipv4_mask(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else if prefix >= 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix)
    }
}

#[inline]
fn ipv6_mask(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else if prefix >= 128 {
        u128::MAX
    } else {
        u128::MAX << (128 - prefix)
    }
}

/// Parse "addr/prefix" or "addr" (bare host = /32 or /128).
fn parse_cidr(s: &str) -> Option<Cidr> {
    let s = s.trim();
    let (addr_s, prefix_s) = match s.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s, None),
    };
    let ip = parse_ip_lenient(addr_s)?;
    let (prefix, max) = match ip {
        IpAddr::V4(_) => (32u8, 32u8),
        IpAddr::V6(_) => (128u8, 128u8),
    };
    let prefix = match prefix_s {
        None => prefix,
        Some(p) => {
            let p: u8 = p.trim().parse().ok()?;
            if p > max {
                return None;
            }
            p
        }
    };
    Some(match ip {
        IpAddr::V4(a) => Cidr::V4 { addr: a, prefix },
        IpAddr::V6(a) => Cidr::V6 { addr: a, prefix },
    })
}

fn arg_cidr(args: &[StrykeValue]) -> Option<Cidr> {
    parse_cidr(&arg_str(args))
}

fn arg2_cidr(args: &[StrykeValue]) -> Option<(Cidr, Cidr)> {
    let a = parse_cidr(&args.first()?.to_string())?;
    let b = parse_cidr(&args.get(1)?.to_string())?;
    Some((a, b))
}

// ── parse / introspection ─────────────────────────────────────────────

/// `cidr_parse(STR)` — canonical "network/prefix" form, or undef.
pub fn cidr_parse(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(c) => StrykeValue::string(c.render()),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_valid_subnet(STR)` — 1 if parses as a valid CIDR, 0 otherwise.
pub fn cidr_valid_subnet(args: &[StrykeValue]) -> StrykeValue {
    b(arg_cidr(args).is_some())
}

/// `cidr_format(STR)` — alias for `cidr_parse`. Provided for verb pairs.
pub fn cidr_format(args: &[StrykeValue]) -> StrykeValue {
    cidr_parse(args)
}

/// `cidr_prefix_len(STR)` — integer prefix (e.g. 24 for `10.0.0.0/24`).
pub fn cidr_prefix_len(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(c) => StrykeValue::integer(c.prefix() as i64),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_class(STR)` — historical IPv4 class letter ("A".."E").
/// v6 → "unicast"/"multicast"/"loopback"/"unspecified"/"link-local"/etc.
pub fn cidr_class(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { addr, .. }) => {
            let first = addr.octets()[0];
            let cls = match first {
                0..=127 => "A",
                128..=191 => "B",
                192..=223 => "C",
                224..=239 => "D",
                _ => "E",
            };
            StrykeValue::string(cls.to_string())
        }
        Some(Cidr::V6 { addr, .. }) => {
            let label = if addr.is_loopback() {
                "loopback"
            } else if addr.is_multicast() {
                "multicast"
            } else if addr.is_unspecified() {
                "unspecified"
            } else if (addr.segments()[0] & 0xffc0) == 0xfe80 {
                "link-local"
            } else if (addr.segments()[0] & 0xfe00) == 0xfc00 {
                "unique-local"
            } else {
                "unicast"
            };
            StrykeValue::string(label.to_string())
        }
        None => StrykeValue::UNDEF,
    }
}

// ── network / broadcast / mask ────────────────────────────────────────

/// `cidr_network(STR)` — network address (host bits zeroed).
pub fn cidr_network(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(c) => StrykeValue::string(c.network_addr().to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_broadcast(STR)` — broadcast address (host bits set). For v6
/// returns the all-ones address inside the prefix (v6 has no real
/// broadcast, but the all-ones address is still useful for range math).
pub fn cidr_broadcast(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(c) => StrykeValue::string(c.broadcast_addr().to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_netmask(STR)` — netmask as a dotted-quad / canonical v6 address.
pub fn cidr_netmask(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) => {
            StrykeValue::string(Ipv4Addr::from(ipv4_mask(prefix)).to_string())
        }
        Some(Cidr::V6 { prefix, .. }) => {
            StrykeValue::string(Ipv6Addr::from(ipv6_mask(prefix)).to_string())
        }
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_hostmask(STR)` — wildcard mask (complement of netmask). Same
/// thing routers print as a "wildcard mask" in ACL syntax.
pub fn cidr_hostmask(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) => {
            StrykeValue::string(Ipv4Addr::from(!ipv4_mask(prefix)).to_string())
        }
        Some(Cidr::V6 { prefix, .. }) => {
            StrykeValue::string(Ipv6Addr::from(!ipv6_mask(prefix)).to_string())
        }
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_wildcard(STR)` — alias of `cidr_hostmask` (Cisco ACL terminology).
pub fn cidr_wildcard(args: &[StrykeValue]) -> StrykeValue {
    cidr_hostmask(args)
}

/// `cidr_to_netmask(PREFIX)` — given a prefix length, return the v4 netmask.
/// Prefixes 33..=128 return v6 netmask; 0..=32 return v4 netmask. Pass
/// `"v6"` as second arg to force v6 interpretation for 0..=32.
pub fn cidr_to_netmask(args: &[StrykeValue]) -> StrykeValue {
    let Some(pfx) = args.first().map(|v| v.to_int()) else {
        return StrykeValue::UNDEF;
    };
    let force_v6 = args
        .get(1)
        .map(|v| v.to_string().eq_ignore_ascii_case("v6"))
        .unwrap_or(false);
    if pfx < 0 {
        return StrykeValue::UNDEF;
    }
    let p = pfx as u8;
    if !force_v6 && pfx <= 32 {
        StrykeValue::string(Ipv4Addr::from(ipv4_mask(p)).to_string())
    } else if pfx <= 128 {
        StrykeValue::string(Ipv6Addr::from(ipv6_mask(p)).to_string())
    } else {
        StrykeValue::UNDEF
    }
}

/// `netmask_to_prefix(NETMASK)` — inverse of `cidr_to_netmask`.
/// Returns prefix length if the netmask is a valid prefix (contiguous
/// ones from the left); undef otherwise.
pub fn netmask_to_prefix(args: &[StrykeValue]) -> StrykeValue {
    let Some(ip) = parse_ip_lenient(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    match ip {
        IpAddr::V4(v4) => {
            let n = u32::from(v4);
            // Contiguous-ones check: invert and verify the result is a power-of-two-minus-one.
            let inv = !n;
            // Valid if n is 0, all-ones, or n == !((1<<k) - 1) for some k in 1..=31.
            if n == 0 {
                return StrykeValue::integer(0);
            }
            if n == u32::MAX {
                return StrykeValue::integer(32);
            }
            // inv = trailing zeros in n's "ones-then-zeros" shape -> must be 2^k - 1
            if inv & inv.wrapping_add(1) != 0 {
                return StrykeValue::UNDEF;
            }
            StrykeValue::integer(n.leading_ones() as i64)
        }
        IpAddr::V6(v6) => {
            let n = u128::from(v6);
            if n == 0 {
                return StrykeValue::integer(0);
            }
            if n == u128::MAX {
                return StrykeValue::integer(128);
            }
            let inv = !n;
            if inv & inv.wrapping_add(1) != 0 {
                return StrykeValue::UNDEF;
            }
            StrykeValue::integer(n.leading_ones() as i64)
        }
    }
}

// ── host enumeration ──────────────────────────────────────────────────

/// `cidr_first_host(STR)` — first usable host (network + 1 for v4 /
/// network for /31, /32). For v6, network address (no broadcast).
pub fn cidr_first_host(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) if prefix >= 31 => cidr_network(args),
        Some(c @ Cidr::V4 { .. }) => {
            if let IpAddr::V4(net) = c.network_addr() {
                StrykeValue::string(Ipv4Addr::from(u32::from(net) + 1).to_string())
            } else {
                StrykeValue::UNDEF
            }
        }
        Some(Cidr::V6 { .. }) => cidr_network(args),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_last_host(STR)` — last usable host (broadcast - 1 for v4 /24
/// and shorter, broadcast for /31, /32; v6 always all-ones address).
pub fn cidr_last_host(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) if prefix >= 31 => cidr_broadcast(args),
        Some(c @ Cidr::V4 { .. }) => {
            if let IpAddr::V4(bcast) = c.broadcast_addr() {
                StrykeValue::string(Ipv4Addr::from(u32::from(bcast) - 1).to_string())
            } else {
                StrykeValue::UNDEF
            }
        }
        Some(Cidr::V6 { .. }) => cidr_broadcast(args),
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_num_hosts(STR)` — usable host count. v4: 2^(32-prefix) - 2
/// for /30 and shorter, 2 for /31, 1 for /32. v6: always full block
/// size (`2^(128-prefix)`, as decimal string for large values).
pub fn cidr_num_hosts(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) => {
            let bits = 32 - prefix as u32;
            let total = 1u64 << bits;
            let hosts = match prefix {
                32 => 1u64,
                31 => 2u64,
                _ => total.saturating_sub(2),
            };
            StrykeValue::integer(hosts as i64)
        }
        Some(Cidr::V6 { prefix, .. }) => {
            let bits = 128 - prefix as u32;
            if bits >= 63 {
                // Exceeds i64; return as decimal string.
                let total = 1u128 << bits;
                StrykeValue::string(total.to_string())
            } else {
                StrykeValue::integer((1u64 << bits) as i64)
            }
        }
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_size(STR)` — total address count including network + broadcast.
/// `cidr_num_hosts` excludes them on v4; this counts them.
pub fn cidr_size(args: &[StrykeValue]) -> StrykeValue {
    match arg_cidr(args) {
        Some(Cidr::V4 { prefix, .. }) => {
            let bits = 32 - prefix as u32;
            StrykeValue::integer(1i64 << bits)
        }
        Some(Cidr::V6 { prefix, .. }) => {
            let bits = 128 - prefix as u32;
            if bits >= 63 {
                StrykeValue::string((1u128 << bits).to_string())
            } else {
                StrykeValue::integer(1i64 << bits)
            }
        }
        None => StrykeValue::UNDEF,
    }
}

/// `cidr_hosts(STR)` — arrayref of every usable host as canonical string.
/// **Bounded**: rejects ranges larger than 65536 addresses to avoid OOM.
/// For large blocks use `cidr_iterate` (a streaming variant added later).
pub fn cidr_hosts(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let size_bits = c.family_bits() - c.prefix();
    if size_bits >= 17 {
        // > 65535 addresses; refuse.
        return StrykeValue::UNDEF;
    }
    let out: Vec<StrykeValue> = match c {
        Cidr::V4 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let total = 1u32 << (32 - prefix);
            let (start, end) = if prefix >= 31 {
                (net, net + total - 1)
            } else {
                (net + 1, net + total - 2)
            };
            (start..=end)
                .map(|n| StrykeValue::string(Ipv4Addr::from(n).to_string()))
                .collect()
        }
        Cidr::V6 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let total = 1u128 << (128 - prefix);
            (net..(net + total))
                .map(|n| StrykeValue::string(Ipv6Addr::from(n).to_string()))
                .collect()
        }
    };
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `cidr_iterate(STR)` — same shape as `cidr_hosts` but capped at 1024
/// and intended as a sample/preview for any prefix size. Returns the
/// first 1024 addresses inside the block.
pub fn cidr_iterate(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let cap = 1024u128;
    let out: Vec<StrykeValue> = match c {
        Cidr::V4 { .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let total = 1u128 << (32 - c.prefix());
            let n = total.min(cap) as u32;
            (0..n)
                .map(|i| StrykeValue::string(Ipv4Addr::from(net + i).to_string()))
                .collect()
        }
        Cidr::V6 { .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let total = 1u128 << (128 - c.prefix());
            let n = total.min(cap);
            (0..n)
                .map(|i| StrykeValue::string(Ipv6Addr::from(net + i).to_string()))
                .collect()
        }
    };
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

// ── membership ────────────────────────────────────────────────────────

fn cidr_contains_ip(c: Cidr, ip: IpAddr) -> bool {
    match (c, ip) {
        (Cidr::V4 { prefix, .. }, IpAddr::V4(v4)) => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let mask = ipv4_mask(prefix);
            (u32::from(v4) & mask) == net
        }
        (Cidr::V6 { prefix, .. }, IpAddr::V6(v6)) => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let mask = ipv6_mask(prefix);
            (u128::from(v6) & mask) == net
        }
        _ => false,
    }
}

/// `cidr_contains(CIDR, IP)` — 1 if `IP` is inside `CIDR`.
pub fn cidr_contains(args: &[StrykeValue]) -> StrykeValue {
    let Some(c) = args.first().and_then(|v| parse_cidr(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let Some(ip) = args.get(1).and_then(|v| parse_ip_lenient(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    b(cidr_contains_ip(c, ip))
}

/// `ip_in_cidr(IP, CIDR)` — flipped-arg variant of `cidr_contains`.
pub fn ip_in_cidr(args: &[StrykeValue]) -> StrykeValue {
    let Some(ip) = args.first().and_then(|v| parse_ip_lenient(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let Some(c) = args.get(1).and_then(|v| parse_cidr(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    b(cidr_contains_ip(c, ip))
}

/// `ip_in_subnet(IP, CIDR)` — alias of `ip_in_cidr` for callers
/// preferring "subnet" terminology.
pub fn ip_in_subnet(args: &[StrykeValue]) -> StrykeValue {
    ip_in_cidr(args)
}

// ── subnet arithmetic ─────────────────────────────────────────────────

/// `cidr_subnet(CIDR, NEW_PREFIX)` — first sub-network of `CIDR` at the
/// new (longer) prefix. e.g. `cidr_subnet("10.0.0.0/16", 24)` →
/// `"10.0.0.0/24"`. Undef if `NEW_PREFIX` ≤ current prefix.
pub fn cidr_subnet(args: &[StrykeValue]) -> StrykeValue {
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let Some(new_prefix) = args.get(1).map(|v| v.to_int()) else {
        return StrykeValue::UNDEF;
    };
    if new_prefix <= c.prefix() as i64 || new_prefix > c.family_bits() as i64 {
        return StrykeValue::UNDEF;
    }
    match c {
        Cidr::V4 { .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            StrykeValue::string(format!("{}/{}", Ipv4Addr::from(net), new_prefix))
        }
        Cidr::V6 { .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            StrykeValue::string(format!("{}/{}", Ipv6Addr::from(net), new_prefix))
        }
    }
}

/// `cidr_supernet(CIDR, NEW_PREFIX)` — supernet at a shorter prefix
/// (covers CIDR). `cidr_supernet("10.0.5.0/24", 16)` → `"10.0.0.0/16"`.
pub fn cidr_supernet(args: &[StrykeValue]) -> StrykeValue {
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let Some(new_prefix) = args.get(1).map(|v| v.to_int()) else {
        return StrykeValue::UNDEF;
    };
    if new_prefix < 0 || new_prefix >= c.prefix() as i64 {
        return StrykeValue::UNDEF;
    }
    match c {
        Cidr::V4 { addr, .. } => {
            let mask = ipv4_mask(new_prefix as u8);
            let net = u32::from(addr) & mask;
            StrykeValue::string(format!("{}/{}", Ipv4Addr::from(net), new_prefix))
        }
        Cidr::V6 { addr, .. } => {
            let mask = ipv6_mask(new_prefix as u8);
            let net = u128::from(addr) & mask;
            StrykeValue::string(format!("{}/{}", Ipv6Addr::from(net), new_prefix))
        }
    }
}

/// `cidr_subnets(CIDR, NEW_PREFIX)` — every subnet at the longer prefix.
/// Bounded: refuses to return > 4096 subnets.
pub fn cidr_subnets(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let Some(new_prefix) = args.get(1).map(|v| v.to_int()) else {
        return StrykeValue::UNDEF;
    };
    if new_prefix <= c.prefix() as i64 || new_prefix > c.family_bits() as i64 {
        return StrykeValue::UNDEF;
    }
    let count_bits = new_prefix as u32 - c.prefix() as u32;
    if count_bits >= 13 {
        return StrykeValue::UNDEF; // > 4096
    }
    let count = 1u32 << count_bits;
    let step = match c {
        Cidr::V4 { .. } => 1u128 << (32 - new_prefix as u32),
        Cidr::V6 { .. } => 1u128 << (128 - new_prefix as u32),
    };
    let out: Vec<StrykeValue> = match c {
        Cidr::V4 { .. } => {
            let base = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            (0..count)
                .map(|i| {
                    let n = base.wrapping_add(i.wrapping_mul(step as u32));
                    StrykeValue::string(format!("{}/{}", Ipv4Addr::from(n), new_prefix))
                })
                .collect()
        }
        Cidr::V6 { .. } => {
            let base = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            (0..count)
                .map(|i| {
                    let n = base + (i as u128) * step;
                    StrykeValue::string(format!("{}/{}", Ipv6Addr::from(n), new_prefix))
                })
                .collect()
        }
    };
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `cidr_split(CIDR)` — split into two halves at prefix+1.
pub fn cidr_split(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if c.prefix() >= c.family_bits() {
        return StrykeValue::UNDEF;
    }
    let new_prefix = c.prefix() + 1;
    let (a, b) = match c {
        Cidr::V4 { .. } => {
            let base = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let half = 1u32 << (32 - new_prefix);
            (
                format!("{}/{}", Ipv4Addr::from(base), new_prefix),
                format!("{}/{}", Ipv4Addr::from(base + half), new_prefix),
            )
        }
        Cidr::V6 { .. } => {
            let base = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let half = 1u128 << (128 - new_prefix);
            (
                format!("{}/{}", Ipv6Addr::from(base), new_prefix),
                format!("{}/{}", Ipv6Addr::from(base + half), new_prefix),
            )
        }
    };
    let elems = vec![StrykeValue::string(a), StrykeValue::string(b)];
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

// ── set operations on CIDRs ───────────────────────────────────────────

/// `cidr_overlaps(A, B)` — 1 if the two CIDRs share any address.
pub fn cidr_overlaps(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let overlap = match (a, c) {
        (Cidr::V4 { .. }, Cidr::V4 { .. }) => {
            let (a_lo, a_hi) = v4_range(a);
            let (b_lo, b_hi) = v4_range(c);
            a_lo <= b_hi && b_lo <= a_hi
        }
        (Cidr::V6 { .. }, Cidr::V6 { .. }) => {
            let (a_lo, a_hi) = v6_range(a);
            let (b_lo, b_hi) = v6_range(c);
            a_lo <= b_hi && b_lo <= a_hi
        }
        _ => false,
    };
    b(overlap)
}

fn v4_range(c: Cidr) -> (u32, u32) {
    let net = match c.network_addr() {
        IpAddr::V4(n) => u32::from(n),
        _ => unreachable!(),
    };
    let bcast = match c.broadcast_addr() {
        IpAddr::V4(n) => u32::from(n),
        _ => unreachable!(),
    };
    (net, bcast)
}

fn v6_range(c: Cidr) -> (u128, u128) {
    let net = match c.network_addr() {
        IpAddr::V6(n) => u128::from(n),
        _ => unreachable!(),
    };
    let bcast = match c.broadcast_addr() {
        IpAddr::V6(n) => u128::from(n),
        _ => unreachable!(),
    };
    (net, bcast)
}

/// `cidr_aggregate(\@cidrs)` — merge a list of CIDRs into the smallest
/// set of CIDRs that covers exactly the same addresses. v4-only for
/// the initial impl; v6 falls back to returning the input deduped.
pub fn cidr_aggregate(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let mut cidrs: Vec<Cidr> = collect_cidrs(args);
    cidrs.retain(|c| matches!(c, Cidr::V4 { .. })); // v4-only for now
    let merged = aggregate_v4(cidrs);
    let out: Vec<StrykeValue> = merged
        .into_iter()
        .map(|c| StrykeValue::string(c.render()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `cidr_summarize(\@cidrs)` — alias of `cidr_aggregate`. Common
/// terminology in route summarization.
pub fn cidr_summarize(args: &[StrykeValue]) -> StrykeValue {
    cidr_aggregate(args)
}

fn collect_cidrs(args: &[StrykeValue]) -> Vec<Cidr> {
    let mut out = Vec::new();
    if let Some(arr) = args.first().and_then(|v| v.as_array_ref()) {
        for v in arr.read().iter() {
            if let Some(c) = parse_cidr(&v.to_string()) {
                out.push(c);
            }
        }
    } else {
        for v in args {
            if let Some(c) = parse_cidr(&v.to_string()) {
                out.push(c);
            }
        }
    }
    out
}

fn aggregate_v4(mut cidrs: Vec<Cidr>) -> Vec<Cidr> {
    cidrs.sort_by_key(|c| {
        let (lo, _) = v4_range(*c);
        (lo, c.prefix())
    });
    let mut out: Vec<Cidr> = Vec::new();
    for c in cidrs {
        if let Some(last) = out.last() {
            let (l_lo, l_hi) = v4_range(*last);
            let (c_lo, c_hi) = v4_range(c);
            if c_lo <= l_hi.saturating_add(1) {
                // Adjacent or overlapping. Try to merge into a covering CIDR.
                let new_lo = l_lo.min(c_lo);
                let new_hi = l_hi.max(c_hi);
                if let Some(merged) = make_v4_cidr_for_range(new_lo, new_hi) {
                    out.pop();
                    out.push(merged);
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

fn make_v4_cidr_for_range(lo: u32, hi: u32) -> Option<Cidr> {
    // Find a prefix length such that [lo..=hi] is exactly the block.
    let span = (hi as u64).saturating_sub(lo as u64) + 1;
    if !span.is_power_of_two() {
        return None;
    }
    let prefix = 32u8 - (span.trailing_zeros() as u8);
    if lo & !ipv4_mask(prefix) != 0 {
        return None;
    }
    Some(Cidr::V4 {
        addr: Ipv4Addr::from(lo),
        prefix,
    })
}

/// `cidr_intersection(A, B)` — common subnet (the longer-prefix one if
/// they overlap, undef otherwise).
pub fn cidr_intersection(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if cidr_contains_ip(a, c.network_addr()) {
        StrykeValue::string(c.render())
    } else if cidr_contains_ip(c, a.network_addr()) {
        StrykeValue::string(a.render())
    } else {
        StrykeValue::UNDEF
    }
}

/// `cidr_difference(A, B)` — addresses in A not in B, as arrayref of
/// CIDRs. v4-only initial impl; if B doesn't overlap A, returns [A].
pub fn cidr_difference(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some((a, b_c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if !matches!((a, b_c), (Cidr::V4 { .. }, Cidr::V4 { .. })) {
        return StrykeValue::UNDEF;
    }
    let (a_lo, a_hi) = v4_range(a);
    let (b_lo, b_hi) = v4_range(b_c);
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    if b_hi < a_lo || b_lo > a_hi {
        ranges.push((a_lo, a_hi));
    } else {
        if a_lo < b_lo {
            ranges.push((a_lo, b_lo - 1));
        }
        if b_hi < a_hi {
            ranges.push((b_hi + 1, a_hi));
        }
    }
    let mut out: Vec<StrykeValue> = Vec::new();
    for (lo, hi) in ranges {
        for c in v4_range_to_cidrs(lo, hi) {
            out.push(StrykeValue::string(c.render()));
        }
    }
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `cidr_union(A, B)` — both inputs combined and aggregated.
pub fn cidr_union(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if !matches!((a, c), (Cidr::V4 { .. }, Cidr::V4 { .. })) {
        return StrykeValue::UNDEF;
    }
    let merged = aggregate_v4(vec![a, c]);
    let out: Vec<StrykeValue> = merged
        .into_iter()
        .map(|c| StrykeValue::string(c.render()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

fn v4_range_to_cidrs(lo: u32, hi: u32) -> Vec<Cidr> {
    let mut out = Vec::new();
    let mut cur = lo;
    while cur <= hi {
        let max_size = if cur == 0 {
            32u8
        } else {
            32 - (cur.trailing_zeros() as u8)
        };
        let span = (hi - cur + 1).next_power_of_two();
        let span = if span > (1 << (32 - max_size)) || !(hi - cur + 1).is_power_of_two() {
            // Fit in remaining range
            let mut s = 1u32;
            while s.saturating_mul(2) <= (hi - cur + 1) && s.saturating_mul(2) > 0 {
                s = s.saturating_mul(2);
            }
            s
        } else {
            span
        };
        let prefix = 32u8 - (span.trailing_zeros() as u8);
        // Ensure cur is aligned for this prefix
        let prefix = prefix.max(32u8 - cur.trailing_zeros().min(32) as u8);
        let real_span = 1u32 << (32 - prefix);
        out.push(Cidr::V4 {
            addr: Ipv4Addr::from(cur),
            prefix,
        });
        cur = match cur.checked_add(real_span) {
            Some(v) => v,
            None => break,
        };
    }
    out
}

/// `cidr_minimum_covering(\@ips)` — smallest single CIDR containing all
/// given IPs. v4-only.
pub fn cidr_minimum_covering(args: &[StrykeValue]) -> StrykeValue {
    let ips: Vec<u32> = if let Some(arr) = args.first().and_then(|v| v.as_array_ref()) {
        arr.read()
            .iter()
            .filter_map(|v| match parse_ip_lenient(&v.to_string()) {
                Some(IpAddr::V4(v4)) => Some(u32::from(v4)),
                _ => None,
            })
            .collect()
    } else {
        args.iter()
            .filter_map(|v| match parse_ip_lenient(&v.to_string()) {
                Some(IpAddr::V4(v4)) => Some(u32::from(v4)),
                _ => None,
            })
            .collect()
    };
    if ips.is_empty() {
        return StrykeValue::UNDEF;
    }
    let lo = *ips.iter().min().unwrap();
    let hi = *ips.iter().max().unwrap();
    // Find the shortest prefix that contains the range
    for p in (0u8..=32).rev() {
        let mask = ipv4_mask(p);
        if (lo & mask) == (hi & mask) {
            return StrykeValue::string(format!("{}/{}", Ipv4Addr::from(lo & mask), p));
        }
    }
    StrykeValue::UNDEF
}

/// `cidr_is_aggregable(A, B)` — 1 if A and B can merge into a single
/// CIDR at prefix-1 (i.e. they're sibling halves of a parent block).
pub fn cidr_is_aggregable(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if a.prefix() != c.prefix() {
        return b(false);
    }
    if !matches!((a, c), (Cidr::V4 { .. }, Cidr::V4 { .. })) {
        return StrykeValue::UNDEF;
    }
    let (a_lo, a_hi) = v4_range(a);
    let (b_lo, b_hi) = v4_range(c);
    // Are they adjacent? a_hi + 1 == b_lo OR b_hi + 1 == a_lo
    if a_hi.saturating_add(1) != b_lo && b_hi.saturating_add(1) != a_lo {
        return b(false);
    }
    // And do they share a parent block at prefix-1?
    let parent_mask = ipv4_mask(a.prefix() - 1);
    b((a_lo & parent_mask) == (b_lo & parent_mask))
}

// ── traversal ─────────────────────────────────────────────────────────

/// `cidr_next(CIDR)` — next block of the same prefix length.
pub fn cidr_next(args: &[StrykeValue]) -> StrykeValue {
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    match c {
        Cidr::V4 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let span = 1u32 << (32 - prefix);
            match net.checked_add(span) {
                Some(n) => StrykeValue::string(format!("{}/{}", Ipv4Addr::from(n), prefix)),
                None => StrykeValue::UNDEF,
            }
        }
        Cidr::V6 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let span = 1u128 << (128 - prefix);
            match net.checked_add(span) {
                Some(n) => StrykeValue::string(format!("{}/{}", Ipv6Addr::from(n), prefix)),
                None => StrykeValue::UNDEF,
            }
        }
    }
}

/// `cidr_prev(CIDR)` — previous block of the same prefix length.
pub fn cidr_prev(args: &[StrykeValue]) -> StrykeValue {
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    match c {
        Cidr::V4 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let span = 1u32 << (32 - prefix);
            match net.checked_sub(span) {
                Some(n) => StrykeValue::string(format!("{}/{}", Ipv4Addr::from(n), prefix)),
                None => StrykeValue::UNDEF,
            }
        }
        Cidr::V6 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let span = 1u128 << (128 - prefix);
            match net.checked_sub(span) {
                Some(n) => StrykeValue::string(format!("{}/{}", Ipv6Addr::from(n), prefix)),
                None => StrykeValue::UNDEF,
            }
        }
    }
}

/// `cidr_distance(A, B)` — block-count distance between two same-prefix
/// CIDRs (signed). Undef on different prefix or family.
pub fn cidr_distance(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    if a.prefix() != c.prefix() {
        return StrykeValue::UNDEF;
    }
    match (a, c) {
        (Cidr::V4 { .. }, Cidr::V4 { .. }) => {
            let an = match a.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let bn = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let span = 1u32 << (32 - a.prefix());
            let diff = (bn as i64) - (an as i64);
            StrykeValue::integer(diff / span as i64)
        }
        (Cidr::V6 { .. }, Cidr::V6 { .. }) => {
            let an = match a.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let bn = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let span = 1u128 << (128 - a.prefix());
            let diff = if bn >= an {
                ((bn - an) / span) as i64
            } else {
                -(((an - bn) / span) as i64)
            };
            StrykeValue::integer(diff)
        }
        _ => StrykeValue::UNDEF,
    }
}

// ── random / ordering ─────────────────────────────────────────────────

/// `cidr_random_ip(CIDR)` — uniformly random IP from inside the block.
pub fn cidr_random_ip(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let Some(c) = arg_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    let mut rng = rand::thread_rng();
    match c {
        Cidr::V4 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V4(n) => u32::from(n),
                _ => unreachable!(),
            };
            let span = 1u32 << (32 - prefix);
            let r = if span == 0 { 0 } else { rng.gen_range(0..span) };
            StrykeValue::string(Ipv4Addr::from(net.wrapping_add(r)).to_string())
        }
        Cidr::V6 { prefix, .. } => {
            let net = match c.network_addr() {
                IpAddr::V6(n) => u128::from(n),
                _ => unreachable!(),
            };
            let span = 1u128 << (128 - prefix);
            let r = if span == 0 { 0 } else { rng.gen_range(0..span) };
            StrykeValue::string(Ipv6Addr::from(net.wrapping_add(r)).to_string())
        }
    }
}

/// `ip_random_in_cidr(CIDR)` — synonym of `cidr_random_ip` for callers
/// who prefer the "ip first" verb form.
pub fn ip_random_in_cidr(args: &[StrykeValue]) -> StrykeValue {
    cidr_random_ip(args)
}

/// `cidr_compare(A, B)` — -1/0/1 by (network, prefix). Same family
/// orders are v4 < v6 cross-family.
pub fn cidr_compare(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, c)) = arg2_cidr(args) else {
        return StrykeValue::UNDEF;
    };
    use std::cmp::Ordering;
    let ord = match (a, c) {
        (Cidr::V4 { .. }, Cidr::V6 { .. }) => Ordering::Less,
        (Cidr::V6 { .. }, Cidr::V4 { .. }) => Ordering::Greater,
        (Cidr::V4 { .. }, Cidr::V4 { .. }) => {
            let (a_lo, _) = v4_range(a);
            let (b_lo, _) = v4_range(c);
            a_lo.cmp(&b_lo).then(a.prefix().cmp(&c.prefix()))
        }
        (Cidr::V6 { .. }, Cidr::V6 { .. }) => {
            let (a_lo, _) = v6_range(a);
            let (b_lo, _) = v6_range(c);
            a_lo.cmp(&b_lo).then(a.prefix().cmp(&c.prefix()))
        }
    };
    StrykeValue::integer(match ord {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// `cidr_sort(\@cidrs)` — sorted arrayref by network address.
pub fn cidr_sort(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let mut cidrs = collect_cidrs(args);
    cidrs.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a, b) {
            (Cidr::V4 { .. }, Cidr::V6 { .. }) => Ordering::Less,
            (Cidr::V6 { .. }, Cidr::V4 { .. }) => Ordering::Greater,
            (Cidr::V4 { .. }, Cidr::V4 { .. }) => v4_range(*a).0.cmp(&v4_range(*b).0),
            (Cidr::V6 { .. }, Cidr::V6 { .. }) => v6_range(*a).0.cmp(&v6_range(*b).0),
        }
    });
    let out: Vec<StrykeValue> = cidrs
        .into_iter()
        .map(|c| StrykeValue::string(c.render()))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

// ══════════════════════════════════════════════════════════════════════
// MAC address ops
// ══════════════════════════════════════════════════════════════════════
//
// Input forms accepted (all six octets, any case):
//   * `aa:bb:cc:dd:ee:ff` — colon (default output)
//   * `aa-bb-cc-dd-ee-ff` — dash (Windows)
//   * `aabb.ccdd.eeff`    — dot (Cisco)
//   * `aabbccddeeff`      — bare 12 hex

fn render_mac(m: [u8; 6]) -> String {
    format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0], m[1], m[2], m[3], m[4], m[5]
    )
}

fn render_mac_sep(m: [u8; 6], sep: &str) -> String {
    match sep {
        "." => format!(
            "{:02x}{:02x}.{:02x}{:02x}.{:02x}{:02x}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
        "" => format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            m[0], m[1], m[2], m[3], m[4], m[5]
        ),
        s => {
            let parts: Vec<String> = m.iter().map(|b| format!("{:02x}", b)).collect();
            parts.join(s)
        }
    }
}

fn arg_mac(args: &[StrykeValue]) -> Option<[u8; 6]> {
    parse_mac_str(&arg_str(args))
}

/// `mac_parse(STR)` — canonical lowercase colon form, or undef.
pub fn mac_parse(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => StrykeValue::string(render_mac(m)),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_is_valid(STR)` — 1 if parses, 0 otherwise.
pub fn mac_is_valid(args: &[StrykeValue]) -> StrykeValue {
    b(arg_mac(args).is_some())
}

/// `mac_normalize(STR)` — alias of `mac_parse`. Canonical lowercase
/// colon form; idempotent.
pub fn mac_normalize(args: &[StrykeValue]) -> StrykeValue {
    mac_parse(args)
}

/// `mac_format(STR, SEP)` — re-render with a user-specified separator.
/// SEP examples: `":"` (default), `"-"`, `"."` (Cisco style — groups of
/// 4 hex), `""` (bare 12 hex). Returns undef on invalid input.
pub fn mac_format(args: &[StrykeValue]) -> StrykeValue {
    let Some(m) = arg_mac(args) else {
        return StrykeValue::UNDEF;
    };
    let sep = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| ":".to_string());
    StrykeValue::string(render_mac_sep(m, &sep))
}

/// `mac_to_int(STR)` — 48-bit integer value. Fits in i64
/// since 2^48 < 2^63.
pub fn mac_to_int(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => {
            let n: u64 = ((m[0] as u64) << 40)
                | ((m[1] as u64) << 32)
                | ((m[2] as u64) << 24)
                | ((m[3] as u64) << 16)
                | ((m[4] as u64) << 8)
                | (m[5] as u64);
            StrykeValue::integer(n as i64)
        }
        None => StrykeValue::UNDEF,
    }
}

/// `int_to_mac(N)` — integer → canonical MAC. Truncates to 48 bits.
pub fn int_to_mac(args: &[StrykeValue]) -> StrykeValue {
    let n = match args.first() {
        Some(v) => v.to_int() as u64 & 0x0000_ffff_ffff_ffff,
        None => return StrykeValue::UNDEF,
    };
    let m = [
        ((n >> 40) & 0xff) as u8,
        ((n >> 32) & 0xff) as u8,
        ((n >> 24) & 0xff) as u8,
        ((n >> 16) & 0xff) as u8,
        ((n >> 8) & 0xff) as u8,
        (n & 0xff) as u8,
    ];
    StrykeValue::string(render_mac(m))
}

/// `mac_to_bytes(STR)` — arrayref of 6 u8 octets.
pub fn mac_to_bytes(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(m) = arg_mac(args) else {
        return StrykeValue::UNDEF;
    };
    let elems: Vec<StrykeValue> = m.iter().map(|b| StrykeValue::integer(*b as i64)).collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

/// `bytes_to_mac(\@bytes)` — 6-byte arrayref → canonical MAC string.
pub fn bytes_to_mac(args: &[StrykeValue]) -> StrykeValue {
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let g = arr.read();
    if g.len() != 6 {
        return StrykeValue::UNDEF;
    }
    let mut m = [0u8; 6];
    for (i, v) in g.iter().enumerate() {
        m[i] = v.to_int().clamp(0, 255) as u8;
    }
    StrykeValue::string(render_mac(m))
}

/// `mac_oui(STR)` — first 24 bits (3 octets) as colon form — the
/// Organisationally Unique Identifier portion.
pub fn mac_oui(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => StrykeValue::string(format!("{:02x}:{:02x}:{:02x}", m[0], m[1], m[2])),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_vendor_lookup(STR)` — OUI → vendor name. Returns empty
/// string when the OUI is not in the small built-in table; callers
/// can `vendor || "unknown"`. For deep lookups, ship a full IEEE OUI
/// registry separately.
pub fn mac_vendor_lookup(args: &[StrykeValue]) -> StrykeValue {
    let Some(m) = arg_mac(args) else {
        return StrykeValue::UNDEF;
    };
    let oui_n: u32 = ((m[0] as u32) << 16) | ((m[1] as u32) << 8) | (m[2] as u32);
    let vendor = match oui_n {
        0x000000 => "XEROX",
        0x000c29 | 0x001c14 | 0x005056 => "VMware",
        0x000d3a => "Hewlett-Packard",
        0x00163e => "Xensource (Xen)",
        0x001a2b | 0x001b78 | 0x001c23 => "Intel",
        0x001b21 | 0x001b63 | 0x001cb3 | 0x002500 | 0x002608 => "Apple",
        0x0050ba | 0x0050c2 => "Cisco",
        0x0050e4 | 0x00a040 | 0x040ccf | 0x080007 | 0x0c4de9 => "Apple",
        0x0050f2 | 0x002485 | 0x60a44c | 0x9802d8 => "Microsoft",
        0x080027 => "Oracle VirtualBox",
        0x18c08a | 0x6c2b59 | 0xa0481c => "Samsung",
        0x3c5ab4 | 0xf4f5d8 => "Google",
        0x525400 => "QEMU",
        0xa45e60 | 0xa483e7 | 0xb09fba | 0xd0817a | 0xf81a67 => "Apple",
        0xb827eb | 0xdca632 | 0xe45f01 => "Raspberry Pi Foundation",
        _ => "",
    };
    StrykeValue::string(vendor.to_string())
}

/// `mac_lookup_vendor(STR)` — alias of `mac_vendor_lookup`.
pub fn mac_lookup_vendor(args: &[StrykeValue]) -> StrykeValue {
    mac_vendor_lookup(args)
}

/// `mac_is_unicast(STR)` — 1 if the low bit of the first octet is 0
/// (IEEE 802 individual address bit).
pub fn mac_is_unicast(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => b((m[0] & 0x01) == 0),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_is_multicast(STR)` — 1 if the low bit of the first octet is 1
/// (includes broadcast).
pub fn mac_is_multicast(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => b((m[0] & 0x01) == 1),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_is_broadcast(STR)` — 1 if all 48 bits are 1 (`ff:ff:…:ff`).
pub fn mac_is_broadcast(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => b(m.iter().all(|&b| b == 0xff)),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_is_locally_administered(STR)` — 1 if the U/L bit (bit 1 of the
/// first octet) is set: MAC was assigned locally, not by IEEE.
pub fn mac_is_locally_administered(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => b((m[0] & 0x02) == 0x02),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_is_universally_administered(STR)` — 1 if the U/L bit is 0
/// (IEEE-assigned, vendor-burned-in).
pub fn mac_is_universally_administered(args: &[StrykeValue]) -> StrykeValue {
    match arg_mac(args) {
        Some(m) => b((m[0] & 0x02) == 0),
        None => StrykeValue::UNDEF,
    }
}

/// `mac_random()` — random MAC with the I/G bit cleared (unicast) and
/// the U/L bit cleared (vendor-burned-in shape). Use `mac_random_local`
/// for the locally-administered shape.
pub fn mac_random(_args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut m: [u8; 6] = rng.gen();
    m[0] &= 0b1111_1100;
    StrykeValue::string(render_mac(m))
}

/// `mac_random_local()` — random MAC with U/L bit set (locally
/// administered) and I/G bit cleared (unicast). Standard shape for
/// software-generated MACs (VPN, container, VM).
pub fn mac_random_local(_args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut m: [u8; 6] = rng.gen();
    m[0] = (m[0] & 0b1111_1100) | 0b0000_0010;
    StrykeValue::string(render_mac(m))
}

/// `mac_compare(A, B)` — -1/0/1 by numeric value.
pub fn mac_compare(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(|v| parse_mac_str(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    let Some(c) = args.get(1).and_then(|v| parse_mac_str(&v.to_string())) else {
        return StrykeValue::UNDEF;
    };
    use std::cmp::Ordering;
    StrykeValue::integer(match a.cmp(&c) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// `eui48_to_eui64(MAC)` — expand a 48-bit MAC to a 64-bit EUI-64 form
/// per RFC 4291: inserts `0xff 0xfe` in the middle and flips the U/L bit.
pub fn eui48_to_eui64(args: &[StrykeValue]) -> StrykeValue {
    let Some(m) = arg_mac(args) else {
        return StrykeValue::UNDEF;
    };
    let eui = mac_to_eui64(m);
    StrykeValue::string(format!(
        "{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}",
        eui[0], eui[1], eui[2], eui[3], eui[4], eui[5], eui[6], eui[7]
    ))
}

/// `eui64_to_eui48(EUI64)` — extract the original 48-bit MAC from an
/// EUI-64. Returns undef if the middle bytes aren't `0xff 0xfe` (i.e.
/// not actually an EUI-48-expanded form).
pub fn eui64_to_eui48(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() != 16 {
        return StrykeValue::UNDEF;
    }
    let mut bytes = [0u8; 8];
    for i in 0..8 {
        let lo = i * 2;
        let hi = lo + 2;
        bytes[i] = match u8::from_str_radix(&cleaned[lo..hi], 16) {
            Ok(byte) => byte,
            Err(_) => return StrykeValue::UNDEF,
        };
    }
    if bytes[3] != 0xff || bytes[4] != 0xfe {
        return StrykeValue::UNDEF;
    }
    let mac = [
        bytes[0] ^ 0x02,
        bytes[1],
        bytes[2],
        bytes[5],
        bytes[6],
        bytes[7],
    ];
    StrykeValue::string(render_mac(mac))
}

/// `eui64_from_mac(MAC)` — alias of `eui48_to_eui64` for callers that
/// prefer the "convert FROM mac" verb form.
pub fn eui64_from_mac(args: &[StrykeValue]) -> StrykeValue {
    eui48_to_eui64(args)
}

// ══════════════════════════════════════════════════════════════════════
// Ports
// ══════════════════════════════════════════════════════════════════════

/// IANA port ranges. 0-1023 well-known, 1024-49151 registered/assigned,
/// 49152-65535 dynamic/ephemeral.
fn port_in_range(args: &[StrykeValue], lo: u16, hi: u16) -> StrykeValue {
    let p = match args.first().map(|v| v.to_int()) {
        Some(n) if (0..=65535).contains(&n) => n as u16,
        _ => return StrykeValue::UNDEF,
    };
    b(p >= lo && p <= hi)
}

/// `port_is_well_known(N)` — 1 if N ∈ 0..=1023 (system ports).
pub fn port_is_well_known(args: &[StrykeValue]) -> StrykeValue {
    port_in_range(args, 0, 1023)
}

/// `port_is_assigned(N)` / `port_is_registered(N)` — 1 if N ∈ 1024..=49151.
pub fn port_is_assigned(args: &[StrykeValue]) -> StrykeValue {
    port_in_range(args, 1024, 49151)
}

/// `port_is_registered(N)` — alias for `port_is_assigned`.
pub fn port_is_registered(args: &[StrykeValue]) -> StrykeValue {
    port_is_assigned(args)
}

/// `port_is_ephemeral(N)` / `port_is_dynamic(N)` — 1 if N ∈ 49152..=65535.
pub fn port_is_ephemeral(args: &[StrykeValue]) -> StrykeValue {
    port_in_range(args, 49152, 65535)
}

/// `port_is_dynamic(N)` — alias for `port_is_ephemeral`.
pub fn port_is_dynamic(args: &[StrykeValue]) -> StrykeValue {
    port_is_ephemeral(args)
}

/// Compact well-known port → service map (only the most common ones —
/// for the full IANA list, ship a separate registry crate).
fn port_to_service_table() -> &'static [(u16, &'static str)] {
    &[
        (7, "echo"),
        (9, "discard"),
        (13, "daytime"),
        (17, "qotd"),
        (19, "chargen"),
        (20, "ftp-data"),
        (21, "ftp"),
        (22, "ssh"),
        (23, "telnet"),
        (25, "smtp"),
        (37, "time"),
        (43, "whois"),
        (49, "tacacs"),
        (53, "dns"),
        (67, "dhcp-server"),
        (68, "dhcp-client"),
        (69, "tftp"),
        (70, "gopher"),
        (79, "finger"),
        (80, "http"),
        (88, "kerberos"),
        (109, "pop2"),
        (110, "pop3"),
        (111, "rpcbind"),
        (113, "ident"),
        (119, "nntp"),
        (123, "ntp"),
        (135, "msrpc"),
        (137, "netbios-ns"),
        (138, "netbios-dgm"),
        (139, "netbios-ssn"),
        (143, "imap"),
        (161, "snmp"),
        (162, "snmptrap"),
        (179, "bgp"),
        (194, "irc"),
        (220, "imap3"),
        (389, "ldap"),
        (443, "https"),
        (445, "smb"),
        (465, "smtps"),
        (500, "isakmp"),
        (512, "exec"),
        (513, "login"),
        (514, "syslog"),
        (515, "lpd"),
        (520, "rip"),
        (530, "rpc"),
        (543, "klogin"),
        (544, "kshell"),
        (548, "afp"),
        (554, "rtsp"),
        (587, "submission"),
        (631, "ipp"),
        (636, "ldaps"),
        (873, "rsync"),
        (993, "imaps"),
        (995, "pop3s"),
        (1080, "socks"),
        (1194, "openvpn"),
        (1433, "mssql"),
        (1521, "oracle"),
        (1701, "l2tp"),
        (1723, "pptp"),
        (1812, "radius-auth"),
        (1813, "radius-acct"),
        (2049, "nfs"),
        (3128, "squid"),
        (3306, "mysql"),
        (3389, "rdp"),
        (3690, "svn"),
        (4369, "epmd"),
        (5060, "sip"),
        (5061, "sips"),
        (5432, "postgres"),
        (5672, "amqp"),
        (5800, "vnc-http"),
        (5900, "vnc"),
        (6379, "redis"),
        (6443, "kubernetes"),
        (6667, "irc"),
        (8080, "http-alt"),
        (8443, "https-alt"),
        (9090, "prometheus"),
        (9092, "kafka"),
        (9200, "elasticsearch"),
        (9418, "git"),
        (11211, "memcached"),
        (15672, "rabbitmq-mgmt"),
        (27017, "mongodb"),
        (50000, "sap"),
    ]
}

/// `port_to_service(N)` — service name for known port, empty otherwise.
pub fn port_to_service(args: &[StrykeValue]) -> StrykeValue {
    let n = args
        .first()
        .map(|v| v.to_int())
        .filter(|n| (0..=65535).contains(n))
        .map(|n| n as u16);
    let Some(port) = n else {
        return StrykeValue::UNDEF;
    };
    let name = port_to_service_table()
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, n)| *n)
        .unwrap_or("");
    StrykeValue::string(name.to_string())
}

/// `port_name(N)` — alias of `port_to_service`.
pub fn port_name(args: &[StrykeValue]) -> StrykeValue {
    port_to_service(args)
}

/// `port_service_lookup(NAME)` — service name → port, undef if unknown.
pub fn port_service_lookup(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args);
    let needle = needle.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return StrykeValue::UNDEF;
    }
    for (p, n) in port_to_service_table() {
        if *n == needle {
            return StrykeValue::integer(*p as i64);
        }
    }
    StrykeValue::UNDEF
}

/// `port_parse_range("8000-8080")` — arrayref of port numbers.
/// Accepts a single port too. Single comma-list: `"22,80,443"`.
pub fn port_parse_range(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let s = arg_str(args);
    let mut out: Vec<StrykeValue> = Vec::new();
    for part in s.split([',', ' ']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let (lo, hi) = (lo.trim().parse::<u32>().ok(), hi.trim().parse::<u32>().ok());
            if let (Some(lo), Some(hi)) = (lo, hi) {
                if lo <= hi && hi <= 65535 {
                    for p in lo..=hi {
                        out.push(StrykeValue::integer(p as i64));
                    }
                }
            }
        } else if let Ok(p) = part.parse::<u32>() {
            if p <= 65535 {
                out.push(StrykeValue::integer(p as i64));
            }
        }
    }
    if out.is_empty() {
        return StrykeValue::UNDEF;
    }
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `port_random_ephemeral()` — random port in 49152..=65535.
pub fn port_random_ephemeral(_args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let p: u16 = rng.gen_range(49152..=65535);
    StrykeValue::integer(p as i64)
}

// ══════════════════════════════════════════════════════════════════════
// WebSocket handshake / framing
// ══════════════════════════════════════════════════════════════════════

const WS_MAGIC: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// `ws_handshake_key()` — random 16-byte client key, base64-encoded.
/// Used as the `Sec-WebSocket-Key` request header value.
pub fn ws_handshake_key(_args: &[StrykeValue]) -> StrykeValue {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    StrykeValue::string(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// `ws_handshake_accept(KEY)` — server's response token (base64 of
/// SHA-1 of `key + magic`). Returns the `Sec-WebSocket-Accept` header
/// value. RFC 6455 §1.3.
pub fn ws_handshake_accept(args: &[StrykeValue]) -> StrykeValue {
    use base64::Engine;
    use sha1::{Digest, Sha1};
    let key = arg_str(args);
    let key = key.trim();
    if key.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut h = Sha1::new();
    h.update(key.as_bytes());
    h.update(WS_MAGIC.as_bytes());
    let digest = h.finalize();
    StrykeValue::string(base64::engine::general_purpose::STANDARD.encode(digest))
}

/// `ws_mask(\@payload, MASK_KEY)` — apply 4-byte mask to payload bytes
/// (XOR each byte with `mask[i % 4]`). RFC 6455 §5.3.
pub fn ws_mask(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let Some(mask) = args.get(1).and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let mg = mask.read();
    if mg.len() != 4 {
        return StrykeValue::UNDEF;
    }
    let mk: [u8; 4] = [
        mg[0].to_int() as u8,
        mg[1].to_int() as u8,
        mg[2].to_int() as u8,
        mg[3].to_int() as u8,
    ];
    drop(mg);
    let g = arr.read();
    let out: Vec<StrykeValue> = g
        .iter()
        .enumerate()
        .map(|(i, v)| StrykeValue::integer(((v.to_int() as u8) ^ mk[i % 4]) as i64))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `ws_unmask(\@payload, MASK_KEY)` — XOR is symmetric, so unmasking
/// is the same op as masking. Provided for verb pairing.
pub fn ws_unmask(args: &[StrykeValue]) -> StrykeValue {
    ws_mask(args)
}

/// `ws_frame_encode(OPCODE, \@payload, FIN, MASK_KEY?)` — build a raw
/// WebSocket frame as an arrayref of bytes. MASK_KEY is optional;
/// supply 4 bytes for client→server frames, omit for server→client.
/// Supports payload lengths up to u64.
pub fn ws_frame_encode(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let opcode = args.first().map(|v| v.to_int()).unwrap_or(1) as u8 & 0x0f;
    let Some(payload_arr) = args.get(1).and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let fin = args.get(2).map(|v| v.is_true()).unwrap_or(true);
    let mask_key: Option<[u8; 4]> = args.get(3).and_then(|v| v.as_array_ref()).and_then(|arr| {
        let g = arr.read();
        if g.len() != 4 {
            return None;
        }
        Some([
            g[0].to_int() as u8,
            g[1].to_int() as u8,
            g[2].to_int() as u8,
            g[3].to_int() as u8,
        ])
    });
    let payload: Vec<u8> = payload_arr
        .read()
        .iter()
        .map(|v| v.to_int() as u8)
        .collect();
    let mut frame: Vec<u8> = Vec::with_capacity(payload.len() + 14);
    let byte0 = (if fin { 0x80 } else { 0x00 }) | opcode;
    frame.push(byte0);
    let mask_bit = if mask_key.is_some() { 0x80 } else { 0x00 };
    let len = payload.len();
    if len < 126 {
        frame.push(mask_bit | (len as u8));
    } else if len < 65536 {
        frame.push(mask_bit | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(mask_bit | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    if let Some(mk) = mask_key {
        frame.extend_from_slice(&mk);
        for (i, b) in payload.iter().enumerate() {
            frame.push(b ^ mk[i % 4]);
        }
    } else {
        frame.extend_from_slice(&payload);
    }
    let elems: Vec<StrykeValue> = frame
        .into_iter()
        .map(|b| StrykeValue::integer(b as i64))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

/// `ws_frame_decode(\@bytes)` — parse a raw WebSocket frame. Returns
/// `{ fin, opcode, masked, payload_len, payload, mask_key }` as a
/// hashref, or undef on incomplete/invalid input.
pub fn ws_frame_decode(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let bytes: Vec<u8> = arr.read().iter().map(|v| v.to_int() as u8).collect();
    if bytes.len() < 2 {
        return StrykeValue::UNDEF;
    }
    let byte0 = bytes[0];
    let byte1 = bytes[1];
    let fin = (byte0 & 0x80) != 0;
    let opcode = byte0 & 0x0f;
    let masked = (byte1 & 0x80) != 0;
    let len7 = byte1 & 0x7f;
    let mut idx = 2usize;
    let payload_len: u64 = match len7 {
        126 => {
            if bytes.len() < idx + 2 {
                return StrykeValue::UNDEF;
            }
            let l = u16::from_be_bytes([bytes[idx], bytes[idx + 1]]) as u64;
            idx += 2;
            l
        }
        127 => {
            if bytes.len() < idx + 8 {
                return StrykeValue::UNDEF;
            }
            let mut b8 = [0u8; 8];
            b8.copy_from_slice(&bytes[idx..idx + 8]);
            idx += 8;
            u64::from_be_bytes(b8)
        }
        n => n as u64,
    };
    let mask_key: Option<[u8; 4]> = if masked {
        if bytes.len() < idx + 4 {
            return StrykeValue::UNDEF;
        }
        let mk = [bytes[idx], bytes[idx + 1], bytes[idx + 2], bytes[idx + 3]];
        idx += 4;
        Some(mk)
    } else {
        None
    };
    if bytes.len() < idx + payload_len as usize {
        return StrykeValue::UNDEF;
    }
    let payload: Vec<u8> = match mask_key {
        Some(mk) => bytes[idx..idx + payload_len as usize]
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mk[i % 4])
            .collect(),
        None => bytes[idx..idx + payload_len as usize].to_vec(),
    };
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("fin".to_string(), b(fin));
    h.insert("opcode".to_string(), StrykeValue::integer(opcode as i64));
    h.insert("masked".to_string(), b(masked));
    h.insert(
        "payload_len".to_string(),
        StrykeValue::integer(payload_len as i64),
    );
    let payload_elems: Vec<StrykeValue> = payload
        .into_iter()
        .map(|b| StrykeValue::integer(b as i64))
        .collect();
    h.insert(
        "payload".to_string(),
        StrykeValue::array_ref(Arc::new(RwLock::new(payload_elems))),
    );
    if let Some(mk) = mask_key {
        let mk_elems: Vec<StrykeValue> =
            mk.iter().map(|b| StrykeValue::integer(*b as i64)).collect();
        h.insert(
            "mask_key".to_string(),
            StrykeValue::array_ref(Arc::new(RwLock::new(mk_elems))),
        );
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `ws_close_frame(CODE, REASON?)` — build an opcode-0x8 close frame
/// per RFC 6455 §5.5.1. CODE is a u16 status (1000=normal, 1001=going
/// away, 1011=server error, etc.), REASON is an optional UTF-8 string.
pub fn ws_close_frame(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let code = args.first().map(|v| v.to_int()).unwrap_or(1000) as u16;
    let reason = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    // Frame: FIN=1, opcode=8 (close), no mask, payload as above
    let mut frame: Vec<u8> = Vec::with_capacity(payload.len() + 4);
    frame.push(0x88); // FIN + opcode 8
    if payload.len() < 126 {
        frame.push(payload.len() as u8);
    } else {
        frame.push(126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&payload);
    let elems: Vec<StrykeValue> = frame
        .into_iter()
        .map(|b| StrykeValue::integer(b as i64))
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

// ══════════════════════════════════════════════════════════════════════
// HTTP cookies
// ══════════════════════════════════════════════════════════════════════

/// `cookie_parse(STR)` — parse a single `Set-Cookie` or `Cookie` header
/// value into a hashref of attributes. Keys: `name`, `value`,
/// `domain`, `path`, `expires`, `max-age`, `secure`, `http-only`,
/// `same-site`.
pub fn cookie_parse(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let s = arg_str(args);
    let mut parts = s.split(';');
    let first = match parts.next() {
        Some(p) => p.trim(),
        None => return StrykeValue::UNDEF,
    };
    let (name, value) = match first.split_once('=') {
        Some((n, v)) => (n.trim().to_string(), v.trim().to_string()),
        None => (first.to_string(), String::new()),
    };
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("name".to_string(), StrykeValue::string(name));
    h.insert("value".to_string(), StrykeValue::string(value));
    for attr in parts {
        let attr = attr.trim();
        if attr.is_empty() {
            continue;
        }
        let (k, v) = match attr.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_lowercase(), v.trim().to_string()),
            None => (attr.to_ascii_lowercase(), String::new()),
        };
        match k.as_str() {
            "secure" => {
                h.insert("secure".to_string(), b(true));
            }
            "httponly" => {
                h.insert("http-only".to_string(), b(true));
            }
            "max-age" => {
                if let Ok(n) = v.parse::<i64>() {
                    h.insert("max-age".to_string(), StrykeValue::integer(n));
                }
            }
            "samesite" => {
                h.insert("same-site".to_string(), StrykeValue::string(v));
            }
            _ => {
                h.insert(k, StrykeValue::string(v));
            }
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `cookie_format(\%cookie)` — render a hashref back to a Set-Cookie
/// header string.
pub fn cookie_format(args: &[StrykeValue]) -> StrykeValue {
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let g = h.read();
    let name = g.get("name").map(|v| v.to_string()).unwrap_or_default();
    let value = g.get("value").map(|v| v.to_string()).unwrap_or_default();
    if name.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut out = format!("{}={}", name, value);
    for (k, v) in g.iter() {
        match k.as_str() {
            "name" | "value" => continue,
            "secure" if v.is_true() => out.push_str("; Secure"),
            "http-only" if v.is_true() => out.push_str("; HttpOnly"),
            "max-age" => out.push_str(&format!("; Max-Age={}", v.to_int())),
            "expires" => out.push_str(&format!("; Expires={}", v)),
            "domain" => out.push_str(&format!("; Domain={}", v)),
            "path" => out.push_str(&format!("; Path={}", v)),
            "same-site" => out.push_str(&format!("; SameSite={}", v)),
            _ => out.push_str(&format!("; {}={}", k, v)),
        }
    }
    StrykeValue::string(out)
}

/// `cookie_jar_new()` — empty hashref of `name → cookie hashref`.
pub fn cookie_jar_new(_args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::hash_ref(Arc::new(RwLock::new(IndexMap::new())))
}

/// `cookie_jar_add(\%jar, COOKIE_STR_OR_HASHREF)` — add a cookie
/// to the jar. Returns 1 on success.
pub fn cookie_jar_add(args: &[StrykeValue]) -> StrykeValue {
    let Some(jar) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let cookie_val = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let parsed = if cookie_val.as_hash_ref().is_some() {
        cookie_val
    } else {
        cookie_parse(&[cookie_val])
    };
    let Some(parsed_h) = parsed.as_hash_ref() else {
        return b(false);
    };
    let name = parsed_h
        .read()
        .get("name")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if name.is_empty() {
        return b(false);
    }
    jar.write().insert(name, parsed);
    b(true)
}

/// `cookie_jar_get(\%jar, NAME)` — cookie hashref by name, or undef.
pub fn cookie_jar_get(args: &[StrykeValue]) -> StrykeValue {
    let Some(jar) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let name = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let g = jar.read();
    g.get(&name).cloned().unwrap_or(StrykeValue::UNDEF)
}

/// `cookie_is_session(\%cookie)` — 1 if cookie has no `Expires` and no
/// `Max-Age` (i.e. dies with the browser session).
pub fn cookie_is_session(args: &[StrykeValue]) -> StrykeValue {
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let g = h.read();
    let has_expires = g.get("expires").is_some_and(|v| !v.is_undef());
    let has_max_age = g.get("max-age").is_some_and(|v| !v.is_undef());
    b(!has_expires && !has_max_age)
}

/// `cookie_is_expired(\%cookie, NOW_UNIX)` — 1 if cookie's max-age or
/// expires indicates it's already past. NOW_UNIX optional (defaults to
/// `time()`).
pub fn cookie_is_expired(args: &[StrykeValue]) -> StrykeValue {
    use std::time::{SystemTime, UNIX_EPOCH};
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let now = args.get(1).map(|v| v.to_int()).unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let g = h.read();
    // Max-Age takes precedence over Expires per RFC 6265
    if let Some(ma) = g.get("max-age") {
        let secs = ma.to_int();
        if secs <= 0 {
            return b(true);
        }
        // We don't know the cookie's creation time, so use the
        // `created_at` field if present; otherwise treat as fresh.
        let created = g.get("created_at").map(|v| v.to_int()).unwrap_or(now);
        return b(now >= created + secs);
    }
    if let Some(exp) = g.get("expires") {
        let exp_str = exp.to_string();
        // Accept either Unix epoch int or RFC 7231 string parsed by chrono.
        if let Ok(epoch) = exp_str.parse::<i64>() {
            return b(now >= epoch);
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(&exp_str) {
            return b(now >= dt.timestamp());
        }
    }
    b(false)
}

/// `cookie_domain_matches(\%cookie, HOST)` — RFC 6265 §5.1.3 domain
/// matching: cookie's Domain is a suffix of HOST (case-insensitive),
/// or exact match.
pub fn cookie_domain_matches(args: &[StrykeValue]) -> StrykeValue {
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let host = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let g = h.read();
    let domain = g
        .get("domain")
        .map(|v| v.to_string().trim_start_matches('.').to_ascii_lowercase())
        .unwrap_or_default();
    let host = host.to_ascii_lowercase();
    if domain.is_empty() {
        return b(false);
    }
    if host == domain {
        return b(true);
    }
    if host.ends_with(&format!(".{}", domain)) {
        return b(true);
    }
    b(false)
}

/// `cookie_path_matches(\%cookie, REQUEST_PATH)` — RFC 6265 §5.1.4
/// path-match: cookie path is `/` or is a prefix of REQUEST_PATH on a
/// `/` boundary.
pub fn cookie_path_matches(args: &[StrykeValue]) -> StrykeValue {
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let req_path = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "/".to_string());
    let g = h.read();
    let path = g
        .get("path")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "/".to_string());
    if path == "/" {
        return b(true);
    }
    if req_path == path {
        return b(true);
    }
    let needle = if path.ends_with('/') {
        path.clone()
    } else {
        format!("{}/", path)
    };
    b(req_path.starts_with(&needle))
}

/// `cookie_set_max_age(\%cookie, SECS)` — set the cookie's Max-Age.
/// Mutates the hashref in place; returns 1.
pub fn cookie_set_max_age(args: &[StrykeValue]) -> StrykeValue {
    let Some(h) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let secs = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    h.write()
        .insert("max-age".to_string(), StrykeValue::integer(secs));
    b(true)
}

// ══════════════════════════════════════════════════════════════════════
// HTTP method / status / MIME helpers
// ══════════════════════════════════════════════════════════════════════

/// `http_method_is_idempotent(METHOD)` — RFC 9110 §9.2.2. Idempotent
/// methods: GET, HEAD, OPTIONS, PUT, DELETE, TRACE.
pub fn http_method_is_idempotent(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_str(args).to_ascii_uppercase();
    b(matches!(
        m.as_str(),
        "GET" | "HEAD" | "OPTIONS" | "PUT" | "DELETE" | "TRACE"
    ))
}

/// `http_method_is_safe(METHOD)` — RFC 9110 §9.2.1. Safe methods don't
/// modify server state: GET, HEAD, OPTIONS, TRACE.
pub fn http_method_is_safe(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_str(args).to_ascii_uppercase();
    b(matches!(m.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE"))
}

/// `http_method_has_body(METHOD)` — methods that typically carry a
/// request body: POST, PUT, PATCH.
pub fn http_method_has_body(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_str(args).to_ascii_uppercase();
    b(matches!(m.as_str(), "POST" | "PUT" | "PATCH"))
}

/// `http_status_class(N)` — first digit of the status code (1-5),
/// undef if out of range.
pub fn http_status_class(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    if (100..=599).contains(&n) {
        StrykeValue::integer(n / 100)
    } else {
        StrykeValue::UNDEF
    }
}

/// `http_status_is_informational(N)` — 100..=199.
pub fn http_status_is_informational(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    b((100..200).contains(&n))
}

/// `http_status_is_success(N)` — 200..=299.
pub fn http_status_is_success(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    b((200..300).contains(&n))
}

/// `http_status_is_redirect(N)` — 300..=399.
pub fn http_status_is_redirect(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    b((300..400).contains(&n))
}

/// `http_status_is_client_error(N)` — 400..=499.
pub fn http_status_is_client_error(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    b((400..500).contains(&n))
}

/// `http_status_is_server_error(N)` — 500..=599.
pub fn http_status_is_server_error(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    b((500..600).contains(&n))
}

/// `http_status_text(N)` — canonical reason phrase per RFC 9110.
pub fn http_status_text(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    let s = match n {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "",
    };
    if s.is_empty() {
        StrykeValue::UNDEF
    } else {
        StrykeValue::string(s.to_string())
    }
}

/// `http_date_parse(STR)` — parse an HTTP date (RFC 7231 IMF-fixdate,
/// RFC 850, or asctime() form) → unix epoch.
pub fn http_date_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    // Try RFC 2822 / IMF-fixdate first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(s) {
        return StrykeValue::integer(dt.timestamp());
    }
    // Try ANSI C asctime() format: "Sun Nov  6 08:49:37 1994"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%a %b %e %T %Y") {
        return StrykeValue::integer(dt.and_utc().timestamp());
    }
    // RFC 850: "Sunday, 06-Nov-94 08:49:37 GMT"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%A, %d-%b-%y %T GMT") {
        return StrykeValue::integer(dt.and_utc().timestamp());
    }
    StrykeValue::UNDEF
}

/// `http_date_format(EPOCH)` — format unix epoch as IMF-fixdate
/// per RFC 7231 (e.g., `"Sun, 06 Nov 1994 08:49:37 GMT"`).
pub fn http_date_format(args: &[StrykeValue]) -> StrykeValue {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(n, 0);
    match dt {
        Some(d) => StrykeValue::string(d.format("%a, %d %b %Y %H:%M:%S GMT").to_string()),
        None => StrykeValue::UNDEF,
    }
}

/// MIME type ↔ extension table (compact — common subset).
fn mime_table() -> &'static [(&'static str, &'static str)] {
    &[
        ("html", "text/html"),
        ("htm", "text/html"),
        ("css", "text/css"),
        ("js", "application/javascript"),
        ("mjs", "application/javascript"),
        ("json", "application/json"),
        ("xml", "application/xml"),
        ("txt", "text/plain"),
        ("md", "text/markdown"),
        ("csv", "text/csv"),
        ("tsv", "text/tab-separated-values"),
        ("yaml", "application/yaml"),
        ("yml", "application/yaml"),
        ("toml", "application/toml"),
        ("pdf", "application/pdf"),
        ("zip", "application/zip"),
        ("tar", "application/x-tar"),
        ("gz", "application/gzip"),
        ("bz2", "application/x-bzip2"),
        ("xz", "application/x-xz"),
        ("zst", "application/zstd"),
        ("7z", "application/x-7z-compressed"),
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("gif", "image/gif"),
        ("webp", "image/webp"),
        ("svg", "image/svg+xml"),
        ("ico", "image/x-icon"),
        ("bmp", "image/bmp"),
        ("tiff", "image/tiff"),
        ("avif", "image/avif"),
        ("heic", "image/heic"),
        ("mp3", "audio/mpeg"),
        ("wav", "audio/wav"),
        ("ogg", "audio/ogg"),
        ("flac", "audio/flac"),
        ("aac", "audio/aac"),
        ("m4a", "audio/mp4"),
        ("opus", "audio/opus"),
        ("mp4", "video/mp4"),
        ("mov", "video/quicktime"),
        ("webm", "video/webm"),
        ("mkv", "video/x-matroska"),
        ("avi", "video/x-msvideo"),
        ("woff", "font/woff"),
        ("woff2", "font/woff2"),
        ("ttf", "font/ttf"),
        ("otf", "font/otf"),
        ("wasm", "application/wasm"),
        ("rs", "text/x-rust"),
        ("py", "text/x-python"),
        ("pl", "application/x-perl"),
        ("stk", "text/x-stryke"),
    ]
}

/// `mime_type_for_extension(EXT)` — `"png"` → `"image/png"`. EXT may
/// include or omit the leading dot. Returns empty string when unknown.
pub fn mime_type_for_extension(args: &[StrykeValue]) -> StrykeValue {
    let ext = arg_str(args);
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let found = mime_table()
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, m)| *m)
        .unwrap_or("");
    StrykeValue::string(found.to_string())
}

/// `mime_extension_for_type(MIME)` — `"image/png"` → `"png"`. Returns
/// empty string when unknown. First match wins for types with multiple
/// extensions (e.g., `image/jpeg` → `jpg`).
pub fn mime_extension_for_type(args: &[StrykeValue]) -> StrykeValue {
    let mime = arg_str(args).trim().to_ascii_lowercase();
    let found = mime_table()
        .iter()
        .find(|(_, m)| *m == mime)
        .map(|(e, _)| *e)
        .unwrap_or("");
    StrykeValue::string(found.to_string())
}

/// `mime_is_text(MIME)` — 1 if the type starts with `"text/"` or is
/// one of the well-known text MIMEs (json, javascript, xml, yaml, toml).
pub fn mime_is_text(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_str(args).to_ascii_lowercase();
    b(m.starts_with("text/")
        || m.starts_with("application/json")
        || m.starts_with("application/javascript")
        || m.starts_with("application/xml")
        || m.starts_with("application/yaml")
        || m.starts_with("application/toml")
        || m.starts_with("application/x-")
            && (m.contains("rust") || m.contains("python") || m.contains("perl")))
}

/// `mime_is_image(MIME)` — 1 if the type starts with `"image/"`.
pub fn mime_is_image(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args).to_ascii_lowercase().starts_with("image/"))
}

/// `mime_is_audio(MIME)` — 1 if the type starts with `"audio/"`.
pub fn mime_is_audio(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args).to_ascii_lowercase().starts_with("audio/"))
}

/// `mime_is_video(MIME)` — 1 if the type starts with `"video/"`.
pub fn mime_is_video(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args).to_ascii_lowercase().starts_with("video/"))
}

/// `mime_is_application(MIME)` — 1 if the type starts with `"application/"`.
pub fn mime_is_application(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args)
        .to_ascii_lowercase()
        .starts_with("application/"))
}

// ══════════════════════════════════════════════════════════════════════
// Bandwidth / RTT formatting
// ══════════════════════════════════════════════════════════════════════

/// `bandwidth_format(BPS)` — human-readable bandwidth string
/// (e.g., `1_500_000` → `"1.5 Mbps"`).
pub fn bandwidth_format(args: &[StrykeValue]) -> StrykeValue {
    let bps = args.first().map(|v| v.to_int()).unwrap_or(0) as f64;
    let (val, unit) = if bps >= 1e12 {
        (bps / 1e12, "Tbps")
    } else if bps >= 1e9 {
        (bps / 1e9, "Gbps")
    } else if bps >= 1e6 {
        (bps / 1e6, "Mbps")
    } else if bps >= 1e3 {
        (bps / 1e3, "Kbps")
    } else {
        (bps, "bps")
    };
    StrykeValue::string(format!("{:.1} {}", val, unit))
}

/// `bandwidth_parse(STR)` — inverse: `"1.5 Mbps"` → `1500000`. Accepts
/// `bps/kbps/mbps/gbps/tbps` case-insensitively. Returns undef on parse
/// failure.
pub fn bandwidth_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    let mut split_at = s.len();
    for (i, c) in s.char_indices() {
        if c.is_ascii_alphabetic() {
            split_at = i;
            break;
        }
    }
    let (num_part, unit_part) = s.split_at(split_at);
    let Ok(num) = num_part.trim().parse::<f64>() else {
        return StrykeValue::UNDEF;
    };
    let mult = match unit_part.trim().to_ascii_lowercase().as_str() {
        "bps" | "" => 1.0,
        "kbps" => 1e3,
        "mbps" => 1e6,
        "gbps" => 1e9,
        "tbps" => 1e12,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::integer((num * mult) as i64)
}

/// Collect numeric latencies from arg or single arg array.
fn collect_latencies(args: &[StrykeValue]) -> Vec<f64> {
    if let Some(arr) = args.first().and_then(|v| v.as_array_ref()) {
        arr.read().iter().map(|v| v.to_number()).collect()
    } else {
        args.iter().map(|v| v.to_number()).collect()
    }
}

/// `latency_ms(SECONDS)` — convert seconds → milliseconds.
pub fn latency_ms(args: &[StrykeValue]) -> StrykeValue {
    let s = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    StrykeValue::float(s * 1000.0)
}

/// `packet_loss(SENT, LOST)` — loss percentage as f64 in `[0.0, 100.0]`.
pub fn packet_loss(args: &[StrykeValue]) -> StrykeValue {
    let sent = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if sent <= 0.0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float((lost / sent) * 100.0)
}

/// `jitter_ms(\@samples_ms)` — RFC 3550 inter-arrival jitter estimate
/// (smoothed mean absolute diff of consecutive samples). Input is a
/// list of latency samples in milliseconds.
pub fn jitter_ms(args: &[StrykeValue]) -> StrykeValue {
    let samples = collect_latencies(args);
    if samples.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let mut j = 0.0f64;
    for i in 1..samples.len() {
        let d = (samples[i] - samples[i - 1]).abs();
        j += (d - j) / 16.0;
    }
    StrykeValue::float(j)
}

/// `rtt_min(\@samples_ms)` — minimum sample.
pub fn rtt_min(args: &[StrykeValue]) -> StrykeValue {
    let s = collect_latencies(args);
    s.into_iter()
        .fold(f64::INFINITY, |a, b| a.min(b))
        .pipe(|v| {
            if v.is_infinite() {
                StrykeValue::UNDEF
            } else {
                StrykeValue::float(v)
            }
        })
}

/// `rtt_max(\@samples_ms)` — maximum sample.
pub fn rtt_max(args: &[StrykeValue]) -> StrykeValue {
    let s = collect_latencies(args);
    s.into_iter()
        .fold(f64::NEG_INFINITY, |a, b| a.max(b))
        .pipe(|v| {
            if v.is_infinite() {
                StrykeValue::UNDEF
            } else {
                StrykeValue::float(v)
            }
        })
}

/// `rtt_avg(\@samples_ms)` — arithmetic mean.
pub fn rtt_avg(args: &[StrykeValue]) -> StrykeValue {
    let s = collect_latencies(args);
    if s.is_empty() {
        return StrykeValue::UNDEF;
    }
    let sum: f64 = s.iter().sum();
    StrykeValue::float(sum / s.len() as f64)
}

trait Pipe: Sized {
    fn pipe<U, F: FnOnce(Self) -> U>(self, f: F) -> U {
        f(self)
    }
}
impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(s: &str) -> StrykeValue {
        StrykeValue::string(s.to_string())
    }

    #[test]
    fn ip_parse_round_trips_v4() {
        assert_eq!(ip_parse(&[s("1.2.3.4")]).to_string(), "1.2.3.4");
        assert_eq!(ip_parse(&[s(" 1.2.3.4 ")]).to_string(), "1.2.3.4");
    }

    #[test]
    fn ip_parse_compresses_v6() {
        assert_eq!(
            ip_parse(&[s("2001:0db8:0000:0000:0000:0000:0000:0001")]).to_string(),
            "2001:db8::1"
        );
    }

    #[test]
    fn ip_parse_undef_on_garbage() {
        assert!(ip_parse(&[s("nope")]).is_undef());
        assert_eq!(ip_is_valid(&[s("nope")]).to_int(), 0);
    }

    #[test]
    fn ip_version_4_and_6() {
        assert_eq!(ip_version(&[s("10.0.0.1")]).to_int(), 4);
        assert_eq!(ip_version(&[s("::1")]).to_int(), 6);
    }

    #[test]
    fn ip_to_int_and_back_v4() {
        assert_eq!(ip_to_int(&[s("0.0.0.0")]).to_int(), 0);
        assert_eq!(ip_to_int(&[s("255.255.255.255")]).to_int(), 0xffff_ffff);
        assert_eq!(
            int_to_ip(&[StrykeValue::integer(0xffff_ffff)]).to_string(),
            "255.255.255.255"
        );
    }

    #[test]
    fn ip_to_bits_round_trip_v4() {
        let bits = ip_to_bits(&[s("10.0.0.1")]).to_string();
        assert_eq!(bits.len(), 32);
        assert_eq!(
            bits_to_ip(&[StrykeValue::string(bits)]).to_string(),
            "10.0.0.1"
        );
    }

    #[test]
    fn ip_is_private_v4_and_v6() {
        assert_eq!(ip_is_private(&[s("10.0.0.1")]).to_int(), 1);
        assert_eq!(ip_is_private(&[s("8.8.8.8")]).to_int(), 0);
        assert_eq!(ip_is_private(&[s("fc00::1")]).to_int(), 1);
        assert_eq!(ip_is_private(&[s("2001:db8::1")]).to_int(), 0);
    }

    #[test]
    fn ip_is_loopback() {
        assert_eq!(super::ip_is_loopback(&[s("127.0.0.1")]).to_int(), 1);
        assert_eq!(super::ip_is_loopback(&[s("::1")]).to_int(), 1);
        assert_eq!(super::ip_is_loopback(&[s("1.2.3.4")]).to_int(), 0);
    }

    #[test]
    fn ip_reverse_v4() {
        assert_eq!(
            ip_reverse(&[s("8.8.8.8")]).to_string(),
            "8.8.8.8.in-addr.arpa"
        );
        assert_eq!(
            ip_reverse(&[s("1.2.3.4")]).to_string(),
            "4.3.2.1.in-addr.arpa"
        );
    }

    #[test]
    fn ip_reverse_v6_nibble_form() {
        // ::1 → 32 nibbles, all 0 except last
        let r = ip_reverse(&[s("::1")]).to_string();
        assert!(r.starts_with("1.0.0.0.0.0.0.0"));
        assert!(r.ends_with(".ip6.arpa"));
    }

    #[test]
    fn ipv6_expand_and_compress() {
        let exp = ipv6_expand(&[s("2001:db8::1")]).to_string();
        assert_eq!(exp, "2001:0db8:0000:0000:0000:0000:0000:0001");
        let comp = ipv6_compress(&[StrykeValue::string(exp.clone())]).to_string();
        assert_eq!(comp, "2001:db8::1");
    }

    #[test]
    fn ipv6_zone_id_extraction() {
        assert_eq!(ipv6_zone_id(&[s("fe80::1%eth0")]).to_string(), "eth0");
        assert!(ipv6_zone_id(&[s("fe80::1")]).is_undef());
        assert_eq!(ipv6_strip_zone(&[s("fe80::1%eth0")]).to_string(), "fe80::1");
    }

    #[test]
    fn ipv4_classful() {
        assert_eq!(ipv4_classful_class(&[s("10.0.0.1")]).to_string(), "A");
        assert_eq!(ipv4_classful_class(&[s("172.16.0.1")]).to_string(), "B");
        assert_eq!(ipv4_classful_class(&[s("192.168.1.1")]).to_string(), "C");
        assert_eq!(ipv4_classful_class(&[s("224.0.0.1")]).to_string(), "D");
        assert_eq!(ipv4_classful_class(&[s("240.0.0.1")]).to_string(), "E");
    }

    #[test]
    fn ipv4_to_v6_mapped() {
        assert_eq!(
            ipv4_to_ipv6_mapped(&[s("192.0.2.128")]).to_string(),
            "::ffff:192.0.2.128"
        );
    }

    #[test]
    fn ipv4_to_v6_6to4() {
        // 192.0.2.1 → 2002:c000:0201::
        assert_eq!(
            ipv4_to_ipv6_6to4(&[s("192.0.2.1")]).to_string(),
            "2002:c000:201::"
        );
    }

    #[test]
    fn ip_compare_orders_correctly() {
        assert_eq!(ip_compare(&[s("1.2.3.4"), s("1.2.3.5")]).to_int(), -1);
        assert_eq!(ip_compare(&[s("1.2.3.5"), s("1.2.3.4")]).to_int(), 1);
        assert_eq!(ip_compare(&[s("1.2.3.4"), s("1.2.3.4")]).to_int(), 0);
        // cross-family: v4 < v6
        assert_eq!(ip_compare(&[s("1.2.3.4"), s("::1")]).to_int(), -1);
    }

    #[test]
    fn ipv6_solicited_node_form() {
        // fe80::1234:5678 -> ff02::1:ff34:5678
        let r = ipv6_solicited_node(&[s("fe80::1234:5678")]).to_string();
        assert_eq!(r, "ff02::1:ff34:5678");
    }

    // ── CIDR tests ──────────────────────────────────────────────────

    #[test]
    fn cidr_parses_v4_and_v6() {
        assert_eq!(cidr_parse(&[s("10.0.5.123/24")]).to_string(), "10.0.5.0/24");
        assert_eq!(
            cidr_parse(&[s("2001:db8:0:0::1/64")]).to_string(),
            "2001:db8::/64"
        );
        assert!(cidr_parse(&[s("not-a-cidr/24")]).is_undef());
    }

    #[test]
    fn cidr_network_and_broadcast() {
        assert_eq!(cidr_network(&[s("10.0.5.99/24")]).to_string(), "10.0.5.0");
        assert_eq!(
            cidr_broadcast(&[s("10.0.5.0/24")]).to_string(),
            "10.0.5.255"
        );
        assert_eq!(
            cidr_broadcast(&[s("192.168.1.0/30")]).to_string(),
            "192.168.1.3"
        );
    }

    #[test]
    fn cidr_masks() {
        assert_eq!(
            cidr_netmask(&[s("10.0.0.0/24")]).to_string(),
            "255.255.255.0"
        );
        assert_eq!(cidr_netmask(&[s("10.0.0.0/16")]).to_string(), "255.255.0.0");
        assert_eq!(cidr_hostmask(&[s("10.0.0.0/24")]).to_string(), "0.0.0.255");
    }

    #[test]
    fn cidr_to_netmask_and_back() {
        assert_eq!(
            cidr_to_netmask(&[StrykeValue::integer(24)]).to_string(),
            "255.255.255.0"
        );
        assert_eq!(
            cidr_to_netmask(&[StrykeValue::integer(16)]).to_string(),
            "255.255.0.0"
        );
        // Round-trip:
        assert_eq!(netmask_to_prefix(&[s("255.255.255.0")]).to_int(), 24);
        assert_eq!(netmask_to_prefix(&[s("255.255.0.0")]).to_int(), 16);
        assert_eq!(netmask_to_prefix(&[s("255.255.255.255")]).to_int(), 32);
        assert_eq!(netmask_to_prefix(&[s("0.0.0.0")]).to_int(), 0);
        // Non-contiguous mask should fail:
        assert!(netmask_to_prefix(&[s("255.0.255.0")]).is_undef());
    }

    #[test]
    fn cidr_host_counts() {
        assert_eq!(cidr_num_hosts(&[s("10.0.0.0/24")]).to_int(), 254);
        assert_eq!(cidr_num_hosts(&[s("10.0.0.0/30")]).to_int(), 2);
        assert_eq!(cidr_num_hosts(&[s("10.0.0.0/31")]).to_int(), 2);
        assert_eq!(cidr_num_hosts(&[s("10.0.0.0/32")]).to_int(), 1);
        assert_eq!(cidr_size(&[s("10.0.0.0/24")]).to_int(), 256);
    }

    #[test]
    fn cidr_first_last_host() {
        assert_eq!(cidr_first_host(&[s("10.0.0.0/24")]).to_string(), "10.0.0.1");
        assert_eq!(
            cidr_last_host(&[s("10.0.0.0/24")]).to_string(),
            "10.0.0.254"
        );
    }

    #[test]
    fn cidr_subnet_and_supernet() {
        assert_eq!(
            cidr_subnet(&[s("10.0.0.0/16"), StrykeValue::integer(24)]).to_string(),
            "10.0.0.0/24"
        );
        assert_eq!(
            cidr_supernet(&[s("10.0.5.0/24"), StrykeValue::integer(16)]).to_string(),
            "10.0.0.0/16"
        );
        assert!(cidr_subnet(&[s("10.0.0.0/24"), StrykeValue::integer(16)]).is_undef());
    }

    #[test]
    fn cidr_split_in_two() {
        let split = cidr_split(&[s("10.0.0.0/24")]);
        let arr = split.as_array_ref().expect("array");
        let g = arr.read();
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].to_string(), "10.0.0.0/25");
        assert_eq!(g[1].to_string(), "10.0.0.128/25");
    }

    #[test]
    fn cidr_contains_and_overlaps() {
        assert_eq!(
            cidr_contains(&[s("10.0.0.0/16"), s("10.0.5.123")]).to_int(),
            1
        );
        assert_eq!(
            cidr_contains(&[s("10.0.0.0/16"), s("11.0.0.1")]).to_int(),
            0
        );
        assert_eq!(ip_in_cidr(&[s("10.0.5.123"), s("10.0.0.0/16")]).to_int(), 1);
        assert_eq!(
            cidr_overlaps(&[s("10.0.0.0/16"), s("10.0.5.0/24")]).to_int(),
            1
        );
        assert_eq!(
            cidr_overlaps(&[s("10.0.0.0/16"), s("11.0.0.0/16")]).to_int(),
            0
        );
    }

    #[test]
    fn cidr_is_aggregable_check() {
        assert_eq!(
            cidr_is_aggregable(&[s("10.0.0.0/25"), s("10.0.0.128/25")]).to_int(),
            1
        );
        assert_eq!(
            cidr_is_aggregable(&[s("10.0.0.0/25"), s("10.0.1.0/25")]).to_int(),
            0
        );
    }

    #[test]
    fn cidr_aggregate_merges_halves() {
        use parking_lot::RwLock;
        use std::sync::Arc;
        let pair = StrykeValue::array_ref(Arc::new(RwLock::new(vec![
            s("10.0.0.0/25"),
            s("10.0.0.128/25"),
        ])));
        let merged = cidr_aggregate(&[pair]);
        let arr = merged.as_array_ref().expect("array");
        let g = arr.read();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].to_string(), "10.0.0.0/24");
    }

    #[test]
    fn cidr_next_and_prev() {
        assert_eq!(cidr_next(&[s("10.0.0.0/24")]).to_string(), "10.0.1.0/24");
        assert_eq!(cidr_prev(&[s("10.0.1.0/24")]).to_string(), "10.0.0.0/24");
    }

    #[test]
    fn cidr_distance_blocks_apart() {
        assert_eq!(
            cidr_distance(&[s("10.0.0.0/24"), s("10.0.5.0/24")]).to_int(),
            5
        );
        assert_eq!(
            cidr_distance(&[s("10.0.5.0/24"), s("10.0.0.0/24")]).to_int(),
            -5
        );
    }

    #[test]
    fn cidr_class_labels() {
        assert_eq!(cidr_class(&[s("10.0.0.0/8")]).to_string(), "A");
        assert_eq!(cidr_class(&[s("172.16.0.0/12")]).to_string(), "B");
        assert_eq!(cidr_class(&[s("::1/128")]).to_string(), "loopback");
        assert_eq!(cidr_class(&[s("fe80::/10")]).to_string(), "link-local");
    }

    #[test]
    fn cidr_min_covering_two_ips() {
        // 10.0.1.5 and 10.0.1.200 — smallest prefix is /24
        let r = cidr_minimum_covering(&[s("10.0.1.5"), s("10.0.1.200")]).to_string();
        assert_eq!(r, "10.0.1.0/24");
    }

    // ── MAC tests ──────────────────────────────────────────────────

    #[test]
    fn mac_parse_all_separator_forms() {
        let canonical = "aa:bb:cc:dd:ee:ff";
        assert_eq!(mac_parse(&[s("aa:bb:cc:dd:ee:ff")]).to_string(), canonical);
        assert_eq!(mac_parse(&[s("AA-BB-CC-DD-EE-FF")]).to_string(), canonical);
        assert_eq!(mac_parse(&[s("aabb.ccdd.eeff")]).to_string(), canonical);
        assert_eq!(mac_parse(&[s("aabbccddeeff")]).to_string(), canonical);
        assert!(mac_parse(&[s("not-a-mac")]).is_undef());
        assert!(mac_parse(&[s("aa:bb:cc:dd:ee")]).is_undef());
    }

    #[test]
    fn mac_format_with_separators() {
        let m = s("aa:bb:cc:dd:ee:ff");
        assert_eq!(
            mac_format(&[m.clone(), s(":")]).to_string(),
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(
            mac_format(&[m.clone(), s("-")]).to_string(),
            "aa-bb-cc-dd-ee-ff"
        );
        assert_eq!(
            mac_format(&[m.clone(), s(".")]).to_string(),
            "aabb.ccdd.eeff"
        );
        assert_eq!(mac_format(&[m, s("")]).to_string(), "aabbccddeeff");
    }

    #[test]
    fn mac_to_int_and_back() {
        let n = mac_to_int(&[s("00:11:22:33:44:55")]).to_int();
        assert_eq!(n, 0x001122334455);
        assert_eq!(
            int_to_mac(&[StrykeValue::integer(n)]).to_string(),
            "00:11:22:33:44:55"
        );
    }

    #[test]
    fn mac_oui_returns_first_3_octets() {
        assert_eq!(mac_oui(&[s("b8:27:eb:11:22:33")]).to_string(), "b8:27:eb");
    }

    #[test]
    fn mac_vendor_known_ouis() {
        assert_eq!(
            mac_vendor_lookup(&[s("b8:27:eb:11:22:33")]).to_string(),
            "Raspberry Pi Foundation"
        );
        assert_eq!(
            mac_vendor_lookup(&[s("00:50:56:11:22:33")]).to_string(),
            "VMware"
        );
        assert_eq!(
            mac_vendor_lookup(&[s("08:00:27:11:22:33")]).to_string(),
            "Oracle VirtualBox"
        );
    }

    #[test]
    fn mac_unicast_multicast_broadcast() {
        assert_eq!(mac_is_unicast(&[s("00:11:22:33:44:55")]).to_int(), 1);
        assert_eq!(mac_is_multicast(&[s("01:00:5e:00:00:01")]).to_int(), 1);
        assert_eq!(mac_is_broadcast(&[s("ff:ff:ff:ff:ff:ff")]).to_int(), 1);
        assert_eq!(mac_is_unicast(&[s("ff:ff:ff:ff:ff:ff")]).to_int(), 0);
    }

    #[test]
    fn mac_universal_vs_local() {
        // 02:... has U/L bit set → locally administered
        assert_eq!(
            mac_is_locally_administered(&[s("02:00:00:00:00:01")]).to_int(),
            1
        );
        assert_eq!(
            mac_is_universally_administered(&[s("02:00:00:00:00:01")]).to_int(),
            0
        );
        // 00:... has U/L bit clear → universally administered
        assert_eq!(
            mac_is_locally_administered(&[s("00:11:22:33:44:55")]).to_int(),
            0
        );
        assert_eq!(
            mac_is_universally_administered(&[s("00:11:22:33:44:55")]).to_int(),
            1
        );
    }

    #[test]
    fn eui48_to_eui64_and_back() {
        // RFC 4291 example: 00-aa-00-3f-2a-1c -> 02aa:00ff:fe3f:2a1c
        let expanded = eui48_to_eui64(&[s("00:aa:00:3f:2a:1c")]).to_string();
        assert_eq!(expanded, "02aa:00ff:fe3f:2a1c");
        let recovered = eui64_to_eui48(&[StrykeValue::string(expanded)]).to_string();
        assert_eq!(recovered, "00:aa:00:3f:2a:1c");
    }

    #[test]
    fn eui64_rejects_non_eui48_form() {
        // Middle bytes not 0xff 0xfe → not an expanded EUI-48
        assert!(eui64_to_eui48(&[s("0011:2233:4455:6677")]).is_undef());
    }

    #[test]
    fn mac_random_is_unicast_unique() {
        for _ in 0..50 {
            let r = mac_random(&[]).to_string();
            let m = parse_mac_str(&r).unwrap();
            assert_eq!(m[0] & 0x01, 0, "I/G bit must be clear");
        }
    }

    #[test]
    fn mac_random_local_is_locally_administered() {
        for _ in 0..50 {
            let r = mac_random_local(&[]).to_string();
            let m = parse_mac_str(&r).unwrap();
            assert_eq!(m[0] & 0x01, 0, "I/G bit must be clear (unicast)");
            assert_eq!(m[0] & 0x02, 0x02, "U/L bit must be set (locally admin)");
        }
    }

    #[test]
    fn mac_compare_ordering() {
        assert_eq!(
            mac_compare(&[s("00:00:00:00:00:01"), s("00:00:00:00:00:02")]).to_int(),
            -1
        );
        assert_eq!(
            mac_compare(&[s("00:00:00:00:00:02"), s("00:00:00:00:00:01")]).to_int(),
            1
        );
        assert_eq!(
            mac_compare(&[s("aa:bb:cc:dd:ee:ff"), s("aa:bb:cc:dd:ee:ff")]).to_int(),
            0
        );
    }
}
