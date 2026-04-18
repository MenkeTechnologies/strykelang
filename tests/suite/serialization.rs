//! Tests for serialization builtins: to_html, to_markdown, to_json, to_csv,
//! to_yaml, to_toml, to_xml, xopen, and their aliases.

use crate::common::*;

// ── to_html ─────────────────────────────────────────────────────────────

#[test]
fn to_html_scalar_produces_styled_div() {
    let html = eval_string(r#"th("hello")"#);
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("<div class=\"scalar\">hello</div>"));
    assert!(html.contains("--cyan:#05d9e8"));
}

#[test]
fn to_html_alias_th() {
    let html = eval_string(r#"th("test")"#);
    assert!(html.contains("<div class=\"scalar\">test</div>"));
}

#[test]
fn to_html_hash_renders_table() {
    let html = eval_string(r#"th({name => "Alice", age => 30})"#);
    assert!(html.contains("<table>"));
    assert!(html.contains("<th>name</th>"));
    assert!(html.contains("<td>Alice</td>"));
    assert!(html.contains("<td>30</td>"));
}

#[test]
fn to_html_aoh_renders_full_table() {
    let html = eval_string(
        r#"my @rows = ({name => "Alice", age => 30}, {name => "Bob", age => 25});
           @rows |> th"#,
    );
    assert!(html.contains("<thead><tr><th>name</th><th>age</th></tr></thead>"));
    assert!(html.contains("<td>Alice</td>"));
    assert!(html.contains("<td>Bob</td>"));
    assert!(html.contains("<td>30</td>"));
    assert!(html.contains("<td>25</td>"));
}

#[test]
fn to_html_plain_array_renders_list() {
    let html = eval_string(r#"qw(alpha beta gamma) |> th"#);
    assert!(html.contains("<ul>"));
    assert!(html.contains("<li>alpha</li>"));
    assert!(html.contains("<li>beta</li>"));
    assert!(html.contains("<li>gamma</li>"));
}

#[test]
fn to_html_undef_renders_null_span() {
    let html = eval_string(r#"th(undef)"#);
    assert!(html.contains("<span class=\"null\">undef</span>"));
}

#[test]
fn to_html_empty_array() {
    // empty array in pipeline becomes undef via normalize_serialize_root
    let html = eval_string(r#"th(undef)"#);
    assert!(html.contains("<span class=\"null\">undef</span>"));
}

#[test]
fn to_html_empty_hash() {
    let html = eval_string(r#"th({})"#);
    assert!(html.contains("<span class=\"null\">{}</span>"));
}

#[test]
fn to_html_escapes_special_chars() {
    let html = eval_string(r#"th("<script>alert(1)</script>")"#);
    assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert"));
}

#[test]
fn to_html_nested_hash_in_array() {
    let html = eval_string(r#"qw(a b) |> map { my $h = {val => $_}; $h } |> th"#);
    assert!(html.contains("<th>val</th>"));
    assert!(html.contains("<td>a</td>"));
    assert!(html.contains("<td>b</td>"));
}

#[test]
fn to_html_pipeline_to_file_returns_path() {
    let html =
        eval_string(r#"my $path = th("test") |> to_file("/tmp/perlrs_test_th.html"); $path"#);
    assert_eq!(html, "/tmp/perlrs_test_th.html");
}

#[test]
fn to_html_aoh_union_keys() {
    // Rows with different key sets — should union all keys
    let html = eval_string(r#"my @r = ({a => 1, b => 2}, {b => 3, c => 4}); @r |> th"#);
    assert!(html.contains("<th>a</th>"));
    assert!(html.contains("<th>b</th>"));
    assert!(html.contains("<th>c</th>"));
}

// ── to_markdown ─────────────────────────────────────────────────────────

#[test]
fn to_markdown_scalar() {
    assert_eq!(eval_string(r#"tmd("hello")"#), "hello\n");
}

#[test]
fn to_markdown_alias_tmd() {
    assert_eq!(eval_string(r#"tmd("x")"#), "x\n");
}

#[test]
fn to_markdown_alias_to_md() {
    assert_eq!(eval_string(r#"to_md("y")"#), "y\n");
}

#[test]
fn to_markdown_hash_renders_key_value_table() {
    let md = eval_string(r#"tmd({name => "Alice", age => 30})"#);
    assert!(md.contains("| Key | Value |"));
    assert!(md.contains("| --- | --- |"));
    assert!(md.contains("| name | Alice |"));
    assert!(md.contains("| age | 30 |"));
}

#[test]
fn to_markdown_aoh_renders_table() {
    let md = eval_string(
        r#"my @r = ({name => "Alice", age => 30}, {name => "Bob", age => 25}); @r |> tmd"#,
    );
    assert!(md.contains("| name | age |"));
    assert!(md.contains("| --- | --- |"));
    assert!(md.contains("| Alice | 30 |"));
    assert!(md.contains("| Bob | 25 |"));
}

#[test]
fn to_markdown_plain_array_renders_bullets() {
    let md = eval_string(r#"qw(x y z) |> tmd"#);
    assert!(md.contains("- x\n"));
    assert!(md.contains("- y\n"));
    assert!(md.contains("- z\n"));
}

#[test]
fn to_markdown_undef() {
    assert_eq!(eval_string(r#"tmd(undef)"#), "*undef*\n");
}

#[test]
fn to_markdown_empty_array() {
    // empty array in pipeline becomes undef via normalize_serialize_root
    assert_eq!(eval_string(r#"tmd(undef)"#), "*undef*\n");
}

#[test]
fn to_markdown_empty_hash() {
    assert_eq!(eval_string(r#"tmd({})"#), "*{}*\n");
}

#[test]
fn to_markdown_escapes_pipe_chars() {
    let md = eval_string(r#"tmd({key => "a|b"})"#);
    assert!(md.contains(r"a\|b"));
}

#[test]
fn to_markdown_escapes_markdown_metacharacters_in_table() {
    // Escaping happens in table cells (md_cell/md_escape_into), not raw scalars
    let md = eval_string(r#"tmd({key => "*bold*"})"#);
    assert!(md.contains(r"\*bold\*"));
}

#[test]
fn to_markdown_aoh_union_keys() {
    let md = eval_string(r#"my @r = ({a => 1}, {b => 2}); @r |> tmd"#);
    assert!(md.contains("| a | b |"));
}

#[test]
fn to_markdown_numeric_values_unquoted() {
    let md = eval_string(r#"tmd({count => 42})"#);
    assert!(md.contains("| 42 |"));
}

// ── to_json ─────────────────────────────────────────────────────────────

#[test]
fn to_json_scalar_string() {
    assert_eq!(eval_string(r#"tj("hello")"#), r#""hello""#);
}

#[test]
fn to_json_integer() {
    assert_eq!(eval_string(r#"tj(42)"#), "42");
}

#[test]
fn to_json_hash() {
    let j = eval_string(r#"tj({a => 1})"#);
    assert!(j.contains(r#""a":1"#));
}

#[test]
fn to_json_array_pipeline() {
    assert_eq!(eval_string(r#"(1, 2, 3) |> tj"#), "[1,2,3]");
}

#[test]
fn to_json_undef_is_null() {
    assert_eq!(eval_string(r#"tj(undef)"#), "null");
}

#[test]
fn to_json_escapes_quotes() {
    let j = eval_string(r#"tj("he said \"hi\"")"#);
    assert!(j.contains(r#"\""#));
}

#[test]
fn to_json_nested_structure() {
    let j = eval_string(r#"tj({a => [1, 2], b => {c => 3}})"#);
    assert!(j.contains("[1,2]"));
    assert!(j.contains(r#""c":3"#));
}

// ── to_csv ──────────────────────────────────────────────────────────────

#[test]
fn to_csv_aoh_produces_header_and_rows() {
    let csv = eval_string(
        r#"my @r = ({name => "Alice", age => 30}, {name => "Bob", age => 25}); @r |> tc"#,
    );
    assert!(csv.contains("name"));
    assert!(csv.contains("age"));
    assert!(csv.contains("Alice"));
    assert!(csv.contains("Bob"));
}

#[test]
fn to_csv_single_hash_key_value() {
    let csv = eval_string(r#"tc({a => 1, b => 2})"#);
    assert!(csv.contains("key,value"));
}

#[test]
fn to_csv_empty_is_empty_string() {
    assert_eq!(eval_string(r#"tc(undef)"#), "");
}

// ── to_yaml ─────────────────────────────────────────────────────────────

#[test]
fn to_yaml_hash_produces_yaml() {
    let y = eval_string(r#"ty({name => "test", val => 42})"#);
    assert!(y.contains("---"));
    assert!(y.contains("name: test"));
    assert!(y.contains("val: 42"));
}

#[test]
fn to_yaml_array_produces_dashes() {
    let y = eval_string(r#"(1, 2, 3) |> ty"#);
    assert!(y.contains("- 1"));
    assert!(y.contains("- 2"));
    assert!(y.contains("- 3"));
}

#[test]
fn to_yaml_undef_is_tilde() {
    assert!(eval_string(r#"ty(undef)"#).contains("~"));
}

// ── to_toml ─────────────────────────────────────────────────────────────

#[test]
fn to_toml_flat_hash() {
    let t = eval_string(r#"tt({debug => 1, name => "app"})"#);
    assert!(t.contains("debug = 1"));
    assert!(t.contains(r#"name = "app""#));
}

#[test]
fn to_toml_nested_hash_becomes_section() {
    let t = eval_string(r#"tt({db => {host => "localhost"}})"#);
    assert!(t.contains("[db]"));
    assert!(t.contains(r#"host = "localhost""#));
}

// ── to_xml ──────────────────────────────────────────────────────────────

#[test]
fn to_xml_produces_xml_header() {
    let x = eval_string(r#"tx({a => 1})"#);
    assert!(x.contains(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    assert!(x.contains("<root>"));
    assert!(x.contains("<a>1</a>"));
}

#[test]
fn to_xml_escapes_ampersand() {
    let x = eval_string(r#"tx({val => "a&b"})"#);
    assert!(x.contains("a&amp;b"));
}

#[test]
fn to_xml_undef_self_closes() {
    let x = eval_string(r#"tx(undef)"#);
    assert!(x.contains("<root/>"));
}

// ── xopen ───────────────────────────────────────────────────────────────

#[test]
fn xopen_returns_path_unchanged() {
    // Write a file first, then xopen returns the path for pipeline chaining
    let path = eval_string(
        r#"th("test") |> to_file("/tmp/perlrs_xopen_ret.html");
           xopen("/tmp/perlrs_xopen_ret.html")"#,
    );
    assert_eq!(path, "/tmp/perlrs_xopen_ret.html");
}

#[test]
fn xopen_alias_xo() {
    let path = eval_string(
        r#"th("test") |> to_file("/tmp/perlrs_xo_ret.html");
           xo("/tmp/perlrs_xo_ret.html")"#,
    );
    assert_eq!(path, "/tmp/perlrs_xo_ret.html");
}

#[test]
fn xopen_in_pipeline_chains() {
    // to_file returns path, xopen opens it and passes path through
    let result = eval_string(r#"th("test") |> to_file("/tmp/perlrs_xopen_chain.html") |> xopen"#);
    assert_eq!(result, "/tmp/perlrs_xopen_chain.html");
}

// ── to_file returns path ────────────────────────────────────────────────

#[test]
fn to_file_returns_path_not_content() {
    // Use content with newlines so to_file's heuristic correctly identifies it as content
    let result = eval_string(r#""line1\nline2" |> to_file("/tmp/perlrs_tf_test.txt")"#);
    assert_eq!(result, "/tmp/perlrs_tf_test.txt");
}

// ── thread syntax with map +{} ──────────────────────────────────────────

#[test]
fn thread_map_plus_hashref_produces_aoh() {
    assert_eq!(
        eval_string(r#"t qw(a b c) map +{name => $_} tj"#),
        r#"[{"name":"a"},{"name":"b"},{"name":"c"}]"#,
    );
}

#[test]
fn thread_map_plus_hashref_with_computed_values() {
    let j = eval_string(r#"t (1, 2, 3) map +{val => $_, sq => $_ * $_} tj"#);
    assert!(j.contains(r#""val":1"#));
    assert!(j.contains(r#""sq":4"#));
    assert!(j.contains(r#""sq":9"#));
}

#[test]
fn thread_map_plus_hashref_to_html() {
    let html = eval_string(r#"t qw(x y) map +{item => $_} th"#);
    assert!(html.contains("<th>item</th>"));
    assert!(html.contains("<td>x</td>"));
    assert!(html.contains("<td>y</td>"));
}

#[test]
fn thread_map_plus_hashref_to_markdown() {
    let md = eval_string(r#"t qw(x y) map +{item => $_} tmd"#);
    assert!(md.contains("| item |"));
    assert!(md.contains("| x |"));
    assert!(md.contains("| y |"));
}

// ── pipe-forward with serializers ───────────────────────────────────────

#[test]
fn pipe_map_plus_hashref_to_json() {
    let j = eval_string(r#"qw(a b) |> map +{name => $_, len => length} |> tj"#);
    assert!(j.contains(r#""name":"a""#));
    assert!(j.contains(r#""name":"b""#));
    assert!(j.contains(r#""len":1"#));
}

#[test]
fn pipe_map_plus_hashref_to_html() {
    let html = eval_string(r#"qw(a b) |> map +{name => $_} |> th"#);
    assert!(html.contains("<th>name</th>"));
    assert!(html.contains("<td>a</td>"));
}

#[test]
fn pipe_map_plus_hashref_to_markdown() {
    let md = eval_string(r#"qw(a b) |> map +{name => $_} |> tmd"#);
    assert!(md.contains("| name |"));
    assert!(md.contains("| a |"));
}

// ── format_bytes in pipeline ────────────────────────────────────────────

#[test]
fn format_bytes_in_to_html_pipeline() {
    let html = eval_string(r#"(1536, 2048) |> map +{size => format_bytes($_)} |> th"#);
    assert!(html.contains("1.50 KB"));
    assert!(html.contains("2.00 KB"));
}

#[test]
fn format_bytes_in_to_markdown_pipeline() {
    let md = eval_string(r#"(1048576, 2097152) |> map +{size => format_bytes($_)} |> tmd"#);
    assert!(md.contains("1.00 MB"));
    assert!(md.contains("2.00 MB"));
}
