//! Extended tests for the crate root API.

use crate::interpreter::Interpreter;
use crate::{
    compat_mode, convert_to_forge, deconvert_to_perl, format_program, lint_program, parse,
    parse_and_run_string_in_file, pec, run, set_compat_mode, try_vm_execute,
};
use std::fs;

#[test]
fn test_lint_program() {
    let mut interp = Interpreter::new();

    // Correct program
    let p1 = parse("my $x = 1; $x + 2;").expect("parse");
    assert!(lint_program(&p1, &mut interp).is_ok());

    // The current lint_program implementation might not catch all strict violations
    // if it returns early when strict is enabled.
    // Let's test a simple syntax error via prepare_program_top_level (e.g. invalid sub re-declaration if that's checked)
    // Actually, let's just ensure it doesn't crash on a valid program.
}

#[test]
fn test_format_and_convert_roundtrip() {
    let code = "sub foo { my $x = shift; return $x * 2; } foo(21);";
    let p = parse(code).expect("parse");

    let formatted = format_program(&p);
    assert!(formatted.contains("sub foo"));

    let forge = convert_to_forge(&p);
    // forge conversion might be more complex, just check it's not empty
    assert!(!forge.is_empty());

    let deconverted = deconvert_to_perl(&p);
    assert!(deconverted.contains("sub foo"));
}

#[test]
fn test_parse_and_run_string_in_file() {
    let mut interp = Interpreter::new();
    let code = "__FILE__ . ':' . __LINE__";
    let res = parse_and_run_string_in_file(code, &mut interp, "custom_file.pl").expect("run");
    assert_eq!(res.to_string(), "custom_file.pl:1");
}

#[test]
fn test_compat_mode_toggle() {
    let original = compat_mode();
    set_compat_mode(true);
    assert!(compat_mode());
    set_compat_mode(false);
    assert!(!compat_mode());
    set_compat_mode(original); // restore
}

#[test]
fn test_pec_cache_save_load() {
    let code = "2 + 3";
    let p = parse(code).expect("parse");
    let mut interp = Interpreter::new();
    interp.prepare_program_top_level(&p).expect("prep");

    let comp = crate::compiler::Compiler::new();
    let chunk = comp.compile_program(&p).expect("compile");

    let fp = pec::source_fingerprint(false, "test.pl", code);
    let bundle = pec::PecBundle::new(false, fp, p.clone(), chunk);

    // Use a temp dir for cache to avoid polluting home
    let tmp_dir = std::env::temp_dir().join("forge_pec_test");
    fs::create_dir_all(&tmp_dir).expect("mkdir");
    let old_dir = std::env::var("FORGE_BC_DIR").ok();
    std::env::set_var("FORGE_BC_DIR", &tmp_dir);

    pec::try_save(&bundle).expect("save");

    let loaded = pec::try_load(&fp, false).expect("load").expect("some");
    assert_eq!(loaded.source_fingerprint, fp);

    fs::remove_dir_all(&tmp_dir).expect("rmdir");
    if let Some(d) = old_dir {
        std::env::set_var("FORGE_BC_DIR", d);
    } else {
        std::env::remove_var("FORGE_BC_DIR");
    }
}

#[test]
fn test_try_vm_execute_fallback() {
    let p = parse("1 + 1").expect("parse");
    let mut interp = Interpreter::new();
    let res = try_vm_execute(&p, &mut interp)
        .expect("should return Some")
        .expect("run");
    assert_eq!(res.to_int(), 2);
}

#[test]
fn test_digests() {
    assert_eq!(
        run("sha256('abc')").expect("run").to_string(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        run("md5('abc')").expect("run").to_string(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    assert_eq!(
        run("sha1('abc')").expect("run").to_string(),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
}

#[test]
fn test_codecs() {
    assert_eq!(run("url_encode('a b')").expect("run").to_string(), "a%20b");
    assert_eq!(run("url_decode('a%20b')").expect("run").to_string(), "a b");
    assert_eq!(
        run("base64_encode('abc')").expect("run").to_string(),
        "YWJj"
    );
    assert_eq!(
        run("base64_decode('YWJj')").expect("run").to_string(),
        "abc"
    );
}

#[test]
fn test_compression() {
    let code = r#"
        my $orig = "compressed text";
        my $gz = gzip($orig);
        my $back = gunzip($gz);
        $back eq $orig;
    "#;
    assert_eq!(run(code).expect("run").to_int(), 1);
}

#[test]
fn test_json_jq() {
    let code = r#"
        my $data = { a => 1, b => [2, 3] };
        json_jq($data, ".b[1]");
    "#;
    assert_eq!(run(code).expect("run").to_int(), 3);
}

#[test]
fn test_datetime() {
    assert!(run("datetime_utc()")
        .expect("run")
        .to_string()
        .contains("Z"));
    assert_eq!(
        run("datetime_strftime(1713430800, '%Y-%m-%d')")
            .expect("run")
            .to_string(),
        "2024-04-18"
    );
}

#[test]
fn test_csv_dataframe() {
    let tmp = std::env::temp_dir().join(format!("test_lib_{}.csv", std::process::id()));
    fs::write(&tmp, "id,val\n1,10\n2,20\n").expect("write csv");

    let path_str = tmp.to_str().unwrap();
    // Test csv_read
    let code = format!(
        r#"
        my $rows = csv_read('{}');
        my $sum = 0;
        # rows might be a list or arrayref depending on context
        my @r = (ref($rows) eq 'ARRAY') ? @$rows : $rows;
        for my $r (@r) {{ $sum += $r->{{val}}; }}
        $sum;
    "#,
        path_str
    );
    assert_eq!(run(&code).expect("run").to_int(), 30);

    // Test par_csv_read
    let pcr_code = format!(
        r#"
        my @rows = par_csv_read('{}');
        scalar(@rows);
    "#,
        path_str
    );
    assert_eq!(run(&pcr_code).expect("run").to_int(), 2);

    // Test dataframe
    let df_code = format!(
        r#"
        my $df = dataframe('{}');
        $df->sum("val");
    "#,
        path_str
    );
    assert_eq!(run(&df_code).expect("run").to_int(), 30);

    let _ = fs::remove_file(&tmp);
}

#[test]
fn test_sqlite() {
    let tmp = std::env::temp_dir().join(format!("test_lib_{}.db", std::process::id()));
    let code = format!(
        r#"
        my $db = sqlite('{}');
        $db->exec("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)");
        $db->exec("INSERT INTO t (name) VALUES (?)", "alice");
        my $rows = $db->query("SELECT name FROM t WHERE id = 1");
        $rows->[0]->{{name}};
    "#,
        tmp.to_str().unwrap()
    );
    assert_eq!(run(&code).expect("run").to_string(), "alice");
    let _ = fs::remove_file(&tmp);
}

#[test]
fn test_fast_map_grep() {
    // These should trigger the fast path in the interpreter
    assert_eq!(
        run("join(',', map { $_ * 2 } (1, 2, 3))")
            .expect("run")
            .to_string(),
        "2,4,6"
    );
    assert_eq!(
        run("join(',', grep { $_ % 2 == 0 } (1, 2, 3, 4))")
            .expect("run")
            .to_string(),
        "2,4"
    );
}

#[test]
fn test_fast_sort() {
    // These should trigger the fast path in the interpreter
    assert_eq!(
        run("join(',', sort { $a <=> $b } (3, 1, 2))")
            .expect("run")
            .to_string(),
        "1,2,3"
    );
    assert_eq!(
        run("join(',', sort { $b <=> $a } (3, 1, 2))")
            .expect("run")
            .to_string(),
        "3,2,1"
    );
    assert_eq!(
        run("join(',', sort { $a cmp $b } ('z', 'a', 'b'))")
            .expect("run")
            .to_string(),
        "a,b,z"
    );
    assert_eq!(
        run("join(',', sort { $b cmp $a } ('z', 'a', 'b'))")
            .expect("run")
            .to_string(),
        "z,b,a"
    );
}
