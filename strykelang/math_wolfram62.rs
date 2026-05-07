// Batch 62 — archive/encoding format primitives: TAR/ZIP/GZIP/LZ4/Zstd/Brotli,
// Base32/58/85, quoted-printable, uuencode, modhex, percent-encode, punycode,
// IDN, MessagePack, CBOR. Each fn implements a per-spec piece (header bytes,
// alphabet lookup, framing, varint/group-of-5 encoder, etc.).

fn b62_to_bytes(v: &PerlValue) -> Vec<u8> {
    arg_to_vec(v).iter().map(|x| x.to_number() as u8).collect()
}

/// TAR header checksum: per POSIX 1003.1-1990, sum of all 512 bytes treating
/// the checksum field itself as ASCII spaces. Args: array of 512 bytes.
fn builtin_tar_header_checksum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bytes = b62_to_bytes(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut sum = 0_i64;
    for (i, b) in bytes.iter().enumerate().take(512) {
        sum += if (148..156).contains(&i) { 32 } else { *b as i64 };
    }
    Ok(PerlValue::integer(sum))
}

/// TAR pad to 512: returns number of padding bytes needed for given length.
fn builtin_tar_pad_512(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    Ok(PerlValue::integer((512 - n.rem_euclid(512)).rem_euclid(512)))
}

/// TAR member record total size = 512 (header) + ⌈data/512⌉·512.
fn builtin_tar_member_record(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data_len = i1(args).max(0);
    let blocks = (data_len + 511) / 512;
    Ok(PerlValue::integer(512 + 512 * blocks))
}

/// ZIP local-file-header size: 30 + filename_len + extra_len. Magic 0x04034b50.
fn builtin_zip_local_header(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let name_len = i1(args).max(0);
    let extra_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(PerlValue::integer(30 + name_len + extra_len))
}

/// ZIP central-directory entry size: 46 + name + extra + comment.
fn builtin_zip_central_dir(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let name_len = i1(args).max(0);
    let extra_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let comment_len = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(PerlValue::integer(46 + name_len + extra_len + comment_len))
}

/// ZIP end-of-central-directory record size: 22 + comment_len.
fn builtin_zip_eocd(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let comment_len = i1(args).max(0);
    Ok(PerlValue::integer(22 + comment_len))
}

/// GZIP member step: header (10 bytes) + optional extras + body + trailer (8).
fn builtin_gzip_member_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let body_len = f1(args);
    let header = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(PerlValue::float(header + body_len + 8.0))
}

/// GZIP CRC32 init = 0xFFFFFFFF (then negated for output).
fn builtin_gzip_crc32_init(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(0xFFFFFFFF_i64))
}

/// GZIP ISIZE field: original-size mod 2³² (little-endian).
fn builtin_gzip_isize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args) as u64;
    Ok(PerlValue::integer((n & 0xFFFF_FFFF) as i64))
}

/// Deflate dynamic-Huffman block: returns code-length-table size for given
/// HCLEN (number of code-length codes following the header). Per RFC 1951,
/// HCLEN ranges 4..19, alphabet length = HCLEN + 4.
fn builtin_deflate_dynamic_huffman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let hclen = i1(args).clamp(0, 15);
    Ok(PerlValue::integer(hclen + 4))
}

/// Deflate static block: alphabet sizes (literal=288, distance=30) per RFC 1951.
fn builtin_deflate_static_block(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kind = i1(args);
    Ok(PerlValue::integer(match kind { 0 => 288, _ => 30 }))
}

/// LZ4 block step: encode N bytes as a token (high nibble = literal length,
/// low = match length). Returns the token byte for given (literal_len, match_len).
fn builtin_lz4_block_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lit = i1(args).clamp(0, 15) as u8;
    let m = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0).min(15);
    Ok(PerlValue::integer(((lit << 4) | m) as i64))
}

/// LZ4 match offset: little-endian 16-bit offset relative to current position.
/// Args: distance back. Returns (lo, hi) packed as lo*256 + hi.
fn builtin_lz4_match_offset(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = i1(args) as u16;
    let lo = (d & 0xFF) as i64;
    let hi = ((d >> 8) & 0xFF) as i64;
    Ok(PerlValue::integer(lo * 256 + hi))
}

/// Zstd frame header: magic 0x28 0xB5 0x2F 0xFD (4 bytes) + frame header descriptor.
/// Returns frame_content_size_flag from FHD byte (top 2 bits).
fn builtin_zstd_frame_header(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fhd = i1(args) as u8;
    Ok(PerlValue::integer(((fhd >> 6) & 0x03) as i64))
}

/// Brotli Huffman code table size at given precode: N = (4 + alphabet_size).
fn builtin_brotli_huffman_table(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alphabet_size = i1(args).max(1);
    Ok(PerlValue::integer(4 + alphabet_size))
}

/// Brotli meta-block: 4 bytes header, then literal/copy commands.
fn builtin_brotli_meta_block(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let payload_len = f1(args);
    Ok(PerlValue::float(4.0 + payload_len))
}

/// LZMA range coder step: range *= prob; if bit=1 add (range_total - range).
fn builtin_lzma_range_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let range = f1(args);
    let prob = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let bit = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let new_range = range * prob;
    Ok(PerlValue::float(if bit == 0 { new_range } else { range - new_range }))
}

/// Quoted-Printable encode: encode byte as "=HH" if byte > 126 or (byte < 32
/// and byte ≠ 9 and byte ≠ 32). Returns the 3-byte encoding packed as
/// 0x3D·0x10000 + hi·0x100 + lo.
fn builtin_quoted_printable_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = (i1(args) & 0xFF) as u8;
    let needs_quote = b > 126 || (b < 32 && b != 9 && b != 32);
    if !needs_quote { return Ok(PerlValue::integer(b as i64)); }
    let hex = b"0123456789ABCDEF";
    let hi = hex[(b >> 4) as usize] as i64;
    let lo = hex[(b & 0x0F) as usize] as i64;
    Ok(PerlValue::integer(0x3D * 0x10000 + hi * 0x100 + lo))
}

/// uuencode step: encode 3 bytes → 4 chars by 6-bit groups + 0x20.
fn builtin_uuencode_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = b62_to_bytes(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = b.len().min(3);
    let mut out = [0_u8; 4];
    if n >= 1 { out[0] = ((b[0] >> 2) & 0x3F) + 0x20; }
    let b1 = if n >= 1 { b[0] } else { 0 };
    let b2 = if n >= 2 { b[1] } else { 0 };
    let b3 = if n >= 3 { b[2] } else { 0 };
    out[1] = (((b1 << 4) | (b2 >> 4)) & 0x3F) + 0x20;
    out[2] = (((b2 << 2) | (b3 >> 6)) & 0x3F) + 0x20;
    out[3] = (b3 & 0x3F) + 0x20;
    let mut acc = 0_i64;
    for &c in &out { acc = (acc << 8) | c as i64; }
    Ok(PerlValue::integer(acc))
}

/// ModHex (YubiKey alphabet "cbdefghijklnrtuv") encode of a hex nibble.
fn builtin_modhex_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = (i1(args) & 0x0F) as usize;
    const MODHEX: [u8; 16] = *b"cbdefghijklnrtuv";
    Ok(PerlValue::integer(MODHEX[n] as i64))
}

/// Percent-encode any byte. Reserved set per RFC 3986: any byte except
/// unreserved [A-Za-z0-9-._~] is encoded.
fn builtin_percent_encode_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = (i1(args) & 0xFF) as u8;
    let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
    if unreserved { return Ok(PerlValue::integer(b as i64)); }
    let hex = b"0123456789ABCDEF";
    Ok(PerlValue::integer(0x25 * 0x10000
        + (hex[(b >> 4) as usize] as i64) * 0x100
        + hex[(b & 0x0F) as usize] as i64))
}

/// Punycode adapt(): per RFC 3492 §6.1, used between adjustments. Args: delta,
/// num_points, first_time.
fn builtin_punycode_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut delta = i1(args);
    let num_points = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let first_time = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) != 0;
    let base: i64 = 36; let tmin: i64 = 1; let tmax: i64 = 26;
    delta /= if first_time { 700 } else { 2 };
    delta += delta / num_points;
    let mut k = 0_i64;
    while delta > ((base - tmin) * tmax) / 2 {
        delta /= base - tmin;
        k += base;
    }
    Ok(PerlValue::integer(k + (((base - tmin + 1) * delta) / (delta + 38))))
}

/// IDN ASCII conversion: domain label "xn--..." prefix marker. Returns 1 if
/// prefix present, else 0. Args: first 4 bytes of label.
fn builtin_idn_to_ascii(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bytes = b62_to_bytes(args.first().unwrap_or(&PerlValue::array(vec![])));
    let prefix = bytes.iter().take(4).copied().collect::<Vec<u8>>();
    Ok(PerlValue::integer(if prefix == b"xn--" { 1 } else { 0 }))
}

/// IDN to Unicode: counterpart — strips xn-- prefix length.
fn builtin_idn_to_unicode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let total_len = i1(args);
    Ok(PerlValue::integer((total_len - 4).max(0)))
}

/// MessagePack pack uint: positive fixint < 128 = single byte; 0xCC for u8,
/// 0xCD u16, 0xCE u32, 0xCF u64. Returns first prefix byte and total size packed
/// as prefix*100 + size.
fn builtin_msgpack_pack_int(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let (prefix, size) = if (0..128).contains(&n) { (n as u8, 1) }
        else if n >= 0 && n <= 0xFF { (0xCC, 2) }
        else if n >= 0 && n <= 0xFFFF { (0xCD, 3) }
        else if n >= 0 && n <= 0xFFFF_FFFF { (0xCE, 5) }
        else { (0xCF, 9) };
    Ok(PerlValue::integer((prefix as i64) * 100 + size))
}

/// MessagePack pack str: fixstr (len < 32) = 0xA0 | len; else 0xD9..0xDB.
fn builtin_msgpack_pack_str(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let len = i1(args);
    let prefix = if len < 32 { 0xA0 | len as u8 }
        else if len <= 0xFF { 0xD9 }
        else if len <= 0xFFFF { 0xDA }
        else { 0xDB };
    Ok(PerlValue::integer(prefix as i64))
}

/// CBOR encode unsigned integer header byte + length: major type 0, additional
/// info encodes the value or length-of-length per RFC 8949.
fn builtin_cbor_encode_uint(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    let header = if n < 24 { n as u8 }
        else if n <= 0xFF { 0x18 }
        else if n <= 0xFFFF { 0x19 }
        else if n <= 0xFFFF_FFFF { 0x1A }
        else { 0x1B };
    Ok(PerlValue::integer(header as i64))
}

/// CBOR encode text-string header: major 3, additional info per length.
fn builtin_cbor_encode_str(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let len = i1(args).max(0);
    let header = if len < 24 { 0x60 | len as u8 }
        else if len <= 0xFF { 0x78 }
        else if len <= 0xFFFF { 0x79 }
        else if len <= 0xFFFF_FFFF { 0x7A }
        else { 0x7B };
    Ok(PerlValue::integer(header as i64))
}


