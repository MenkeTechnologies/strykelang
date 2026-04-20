//! Extra tests for AOT (Ahead-of-Time) building and trailer handling.

use crate::aot::{AOT_MAGIC, AOT_VERSION, TRAILER_LEN};

#[test]
fn test_aot_constants() {
    assert_eq!(AOT_MAGIC, b"STRK_AOT");
    assert_eq!(AOT_VERSION, 1);
    assert_eq!(TRAILER_LEN, 32);
}

#[test]
fn test_aot_invalid_trailers() {
    use crate::aot::try_load_embedded;
    use std::fs;

    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke-aot-invalid-{}", rand::random::<u32>()));

    // 1. File exactly TRAILER_LEN but wrong magic
    let mut data = vec![0u8; 32];
    data[24..32].copy_from_slice(b"WRONGMAG");
    fs::write(&path, &data).unwrap();
    assert!(try_load_embedded(&path).is_none());

    // 2. File with correct magic but wrong version
    data[24..32].copy_from_slice(b"STRK_AOT");
    data[16..20].copy_from_slice(&999u32.to_le_bytes());
    fs::write(&path, &data).unwrap();
    assert!(try_load_embedded(&path).is_none());

    // 3. Correct magic, correct version, but compressed_len is zero
    data[16..20].copy_from_slice(&1u32.to_le_bytes());
    data[0..8].copy_from_slice(&0u64.to_le_bytes());
    fs::write(&path, &data).unwrap();
    assert!(try_load_embedded(&path).is_none());

    // 4. Correct magic, correct version, but compressed_len is too large
    data[0..8].copy_from_slice(&1000u64.to_le_bytes());
    fs::write(&path, &data).unwrap();
    assert!(try_load_embedded(&path).is_none());

    let _ = fs::remove_file(&path);
}

#[test]
fn test_aot_payload_corrupted_zstd() {
    use crate::aot::{try_load_embedded, AOT_MAGIC, AOT_VERSION};
    use std::fs;
    use std::io::Write;

    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke-aot-corrupt-{}", rand::random::<u32>()));

    // Write some "binary" data
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(b"binary data").unwrap();

    // Write corrupted zstd data (not valid zstd)
    let corrupt_zstd = b"not zstd at all";
    f.write_all(corrupt_zstd).unwrap();

    // Write trailer pointing to it
    let mut trailer = [0u8; 32];
    trailer[0..8].copy_from_slice(&(corrupt_zstd.len() as u64).to_le_bytes());
    trailer[8..16].copy_from_slice(&100u64.to_le_bytes()); // uncompressed_len
    trailer[16..20].copy_from_slice(&AOT_VERSION.to_le_bytes());
    trailer[24..32].copy_from_slice(AOT_MAGIC);
    f.write_all(&trailer).unwrap();
    drop(f);

    assert!(try_load_embedded(&path).is_none());
    let _ = fs::remove_file(&path);
}

#[test]
fn test_aot_build_invalid_script() {
    use crate::aot::build;
    use std::fs;

    let dir = std::env::temp_dir();
    let script = dir.join("invalid.pl");
    let out = dir.join("out.exe");

    fs::write(&script, "sub { !!! syntax error !!! }").unwrap();

    let res = build(&script, &out);
    assert!(res.is_err());
    let err = res.unwrap_err();
    println!("AOT build error: {}", err);
    // PerlError Display usually doesn't say "Syntax" in the string if it's formatted for parity.
    // It might just be "Expected ..., got ... at -e line 1."
    assert!(err.contains("at invalid.pl line 1"));

    let _ = fs::remove_file(&script);
    let _ = fs::remove_file(&out);
}
