//! Tests for the new builtin functions added in v0.1.45–0.1.46:
//! partition, frequencies, min_by, max_by, zip_with, interleave,
//! read_lines, append_file, tempfile, tempdir, read_json, write_json,
//! glob_match, which_all.

use crate::common::*;
use std::fs;

// ── partition ────────────────────────────────────────────────────────

#[test]
fn partition_splits_even_odd() {
    // partition returns [\@true, \@false] — two array refs
    assert_eq!(
        eval_string(r#"my ($yes, $no) = partition { $_ % 2 == 0 } 1,2,3,4,5; "@$yes""#),
        "2 4",
    );
}

#[test]
fn partition_false_bucket() {
    assert_eq!(
        eval_string(r#"my ($yes, $no) = partition { $_ % 2 == 0 } 1,2,3,4,5; "@$no""#),
        "1 3 5",
    );
}

#[test]
fn partition_all_true() {
    assert_eq!(
        eval_string(r#"my ($y, $n) = partition { 1 } "a","b","c"; scalar @$y"#),
        "3",
    );
}

#[test]
fn partition_all_false() {
    assert_eq!(
        eval_string(r#"my ($y, $n) = partition { 0 } "a","b"; scalar @$n"#),
        "2",
    );
}

#[test]
fn partition_single_item() {
    // partition with a single-element list
    assert_eq!(
        eval_int(r#"my @a = (42); my ($y, $n) = partition { $_ > 10 } @a; scalar @$y"#),
        1,
    );
}

#[test]
fn partition_with_array_variable() {
    assert_eq!(
        eval_string(
            r#"my @a = (10, 15, 20, 25); my ($big, $small) = partition { $_ >= 20 } @a; "@$big""#
        ),
        "20 25",
    );
}

#[test]
fn partition_pipe_forward() {
    assert_eq!(
        eval_string(r#"my ($y, $n) = (1,2,3,4) |> partition { $_ > 2 }; "@$y""#),
        "3 4",
    );
}

// ── frequencies ─────────────────────────────────────────────────────

#[test]
fn frequencies_basic_counts() {
    assert_eq!(
        eval_int(r#"my $f = frequencies("a","b","a","c","b","a"); $f->{a}"#),
        3,
    );
}

#[test]
fn frequencies_single_element() {
    assert_eq!(eval_int(r#"my $f = frequencies("x"); $f->{x}"#), 1,);
}

#[test]
fn frequencies_from_array() {
    assert_eq!(
        eval_int(r#"my @w = ("foo","bar","foo"); my $f = frequencies @w; $f->{foo}"#),
        2,
    );
}

#[test]
fn frequencies_numeric_keys() {
    assert_eq!(eval_int(r#"my $f = frequencies(1,2,2,3,3,3); $f->{3}"#), 3,);
}

#[test]
fn frequencies_returns_hashref() {
    assert_eq!(eval_string(r#"ref(frequencies("a","b"))"#), "HASH",);
}

#[test]
fn frequencies_pipe_forward() {
    assert_eq!(
        eval_int(r#"my $f = ("x","y","x") |> frequencies; $f->{x}"#),
        2,
    );
}

#[test]
fn frequencies_empty_list() {
    assert_eq!(eval_int(r#"my $f = frequencies(); scalar keys %$f"#), 0,);
}

// ── min_by / max_by ─────────────────────────────────────────────────

#[test]
fn min_by_finds_shortest_string() {
    assert_eq!(
        eval_string(r#"min_by { length $_ } "apple", "fig", "banana""#),
        "fig",
    );
}

#[test]
fn max_by_finds_longest_string() {
    assert_eq!(
        eval_string(r#"max_by { length $_ } "apple", "fig", "banana""#),
        "banana",
    );
}

#[test]
fn min_by_numeric_key() {
    assert_eq!(eval_int(r#"min_by { $_ * $_ } -3, 1, -2, 4"#), 1,);
}

#[test]
fn max_by_numeric_key() {
    assert_eq!(eval_int(r#"max_by { $_ * $_ } -3, 1, -2, 4"#), 4,);
}

#[test]
fn min_by_single_element() {
    assert_eq!(eval_int(r#"min_by { $_ } 42"#), 42,);
}

#[test]
fn max_by_single_element() {
    assert_eq!(eval_int(r#"max_by { $_ } 99"#), 99,);
}

#[test]
fn min_by_with_array() {
    assert_eq!(
        eval_string(r#"my @words = ("hi","hello","hey"); min_by { length $_ } @words"#),
        "hi",
    );
}

#[test]
fn max_by_pipe_forward() {
    assert_eq!(
        eval_string(r#"("cat","elephant","dog") |> max_by { length $_ }"#),
        "elephant",
    );
}

#[test]
fn min_by_pipe_forward() {
    assert_eq!(
        eval_string(r#"("cat","elephant","dog") |> min_by { length $_ }"#),
        "cat",
    );
}

// ── zip_with ────────────────────────────────────────────────────────

#[test]
fn zip_with_addition() {
    assert_eq!(
        eval_string(r#"my @r = zip_with { $_[0] + $_[1] } [1,2,3], [10,20,30]; "@r""#),
        "11 22 33",
    );
}

#[test]
fn zip_with_string_concat() {
    assert_eq!(
        eval_string(r#"my @r = zip_with { $_[0] . $_[1] } ["a","b"], ["x","y"]; "@r""#),
        "ax by",
    );
}

#[test]
fn zip_with_binding_refs() {
    assert_eq!(
        eval_string(
            r#"my @a = (1,2,3); my @b = (10,20,30); my @r = zip_with { $_[0] + $_[1] } \@a, \@b; "@r""#
        ),
        "11 22 33",
    );
}

#[test]
fn zip_with_unequal_lengths() {
    // zip_with uses longest list, filling missing with undef (0 for addition)
    assert_eq!(
        eval_string(r#"my @r = zip_with { $_[0] + $_[1] } [1,2], [10,20,30]; "@r""#),
        "11 22 30",
    );
}

#[test]
fn zip_with_multiply() {
    assert_eq!(
        eval_string(r#"my @r = zip_with { $_[0] * $_[1] } [2,3,4], [5,6,7]; "@r""#),
        "10 18 28",
    );
}

// ── interleave ──────────────────────────────────────────────────────

#[test]
fn interleave_two_arrays() {
    assert_eq!(
        eval_string(r#"my @r = interleave [1,2,3], [10,20,30]; "@r""#),
        "1 10 2 20 3 30",
    );
}

#[test]
fn interleave_three_arrays() {
    assert_eq!(
        eval_string(r#"my @r = interleave ["a","b"], ["x","y"], ["1","2"]; "@r""#),
        "a x 1 b y 2",
    );
}

#[test]
fn interleave_unequal_lengths() {
    assert_eq!(
        eval_string(r#"my @r = interleave [1,2,3], [10]; "@r""#),
        "1 10 2 3",
    );
}

#[test]
fn interleave_single_array() {
    assert_eq!(eval_string(r#"my @r = interleave [1,2,3]; "@r""#), "1 2 3",);
}

#[test]
fn interleave_pipe_forward() {
    assert_eq!(
        eval_string(r#"my @r = ([1,2], [3,4]) |> interleave; "@r""#),
        "1 3 2 4",
    );
}

// ── read_lines ──────────────────────────────────────────────────────

#[test]
fn read_lines_reads_file_into_array() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_rl_{}.txt", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
    let code = format!(r#"my @l = read_lines("{ps}"); "@l""#);
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, "alpha beta gamma");
}

#[test]
fn read_lines_scalar_context_count() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_rlc_{}.txt", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let code = format!(r#"my @l = read_lines("{ps}"); scalar @l"#);
    let got = eval_int(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, 3);
}

#[test]
fn read_lines_no_trailing_newline() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_rlnt_{}.txt", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    fs::write(&path, "first\nsecond").unwrap();
    let code = format!(r#"my @l = read_lines("{ps}"); scalar @l"#);
    let got = eval_int(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, 2);
}

// ── append_file ─────────────────────────────────────────────────────

#[test]
fn append_file_creates_and_appends() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_af_{}.txt", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"append_file("{ps}", "hello\n"); append_file("{ps}", "world\n"); my @l = read_lines("{ps}"); "@l""#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, "hello world");
}

// ── tempfile / tempdir ──────────────────────────────────────────────

#[test]
fn tempfile_returns_valid_path() {
    let got = eval_string(r#"my $f = tempfile(); -e $f ? "exists" : "missing""#);
    assert_eq!(got, "exists");
}

#[test]
fn tempfile_with_suffix() {
    let got = eval_string(r#"my $f = tempfile(".log"); $f =~ /\.log$/ ? "ok" : "bad""#);
    assert_eq!(got, "ok");
}

#[test]
fn tempdir_returns_valid_directory() {
    let got = eval_string(r#"my $d = tempdir(); -d $d ? "isdir" : "notdir""#);
    assert_eq!(got, "isdir");
}

// ── read_json / write_json ──────────────────────────────────────────

#[test]
fn write_json_read_json_roundtrip_hash() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_json_{}.json", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(
        r#"write_json("{ps}", {{ name => "Alice", age => 30 }}); my $d = read_json("{ps}"); $d->{{name}}"#
    );
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, "Alice");
}

#[test]
fn write_json_read_json_roundtrip_array() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_json2_{}.json", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    let code = format!(r#"write_json("{ps}", [1,2,3]); my $d = read_json("{ps}"); $d->[1]"#);
    let got = eval_int(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, 2);
}

#[test]
fn read_json_nested_hash() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stryke_test_json3_{}.json", std::process::id()));
    let ps = path.to_string_lossy().replace('\\', "/");
    fs::write(&path, r#"{"user":{"name":"Bob","scores":[10,20]}}"#).unwrap();
    let code = format!(r#"my $d = read_json("{ps}"); $d->{{user}}->{{name}}"#);
    let got = eval_string(&code);
    let _ = fs::remove_file(&path);
    assert_eq!(got, "Bob");
}

// ── glob_match ──────────────────────────────────────────────────────

#[test]
fn glob_match_star_dot_rs() {
    assert_eq!(eval_int(r#"glob_match("*.rs", "main.rs")"#), 1,);
}

#[test]
fn glob_match_no_match() {
    assert_eq!(eval_int(r#"glob_match("*.txt", "main.rs")"#), 0,);
}

#[test]
fn glob_match_question_mark() {
    assert_eq!(eval_int(r#"glob_match("f?o", "foo")"#), 1,);
}

#[test]
fn glob_match_question_mark_no_match() {
    assert_eq!(eval_int(r#"glob_match("f?o", "fooo")"#), 0,);
}

#[test]
fn glob_match_double_star() {
    assert_eq!(
        eval_int(r#"glob_match("src/**/*.rs", "src/parser/mod.rs")"#),
        1,
    );
}

#[test]
fn glob_match_exact() {
    assert_eq!(eval_int(r#"glob_match("hello", "hello")"#), 1,);
}

#[test]
fn glob_match_prefix_star() {
    assert_eq!(eval_int(r#"glob_match("*.txt", "readme.txt")"#), 1,);
}

// ── which_all ───────────────────────────────────────────────────────

#[test]
fn which_all_returns_array() {
    // sh should be findable on any Unix
    let got = eval_string(r#"my @w = which_all("sh"); scalar @w > 0 ? "found" : "empty""#);
    assert_eq!(got, "found");
}

#[test]
fn which_all_nonexistent_returns_empty() {
    assert_eq!(
        eval_int(r#"my @w = which_all("__stryke_nonexistent_cmd_xyz__"); scalar @w"#),
        0,
    );
}

#[test]
fn which_all_paths_are_executable() {
    // every path which_all returns should actually exist
    assert_eq!(
        eval_string(
            r#"my @w = which_all("sh"); my $ok = 1; for (@w) { $ok = 0 unless -e $_ } $ok ? "ok" : "bad""#
        ),
        "ok",
    );
}

// ── case conversions ────────────────────────────────────────────────

#[test]
fn snake_case_converts_camel_and_spaces() {
    assert_eq!(eval_string(r#"snake_case "HelloWorld""#), "hello_world");
    assert_eq!(eval_string(r#"snake_case "hello-world""#), "hello_world");
    assert_eq!(eval_string(r#"snake_case "hello world""#), "hello_world");
}

#[test]
fn camel_case_converts_snake_and_kebab() {
    assert_eq!(eval_string(r#"camel_case "hello_world""#), "helloWorld");
    assert_eq!(eval_string(r#"camel_case "hello-world""#), "helloWorld");
}

#[test]
fn kebab_case_converts_camel_and_snake() {
    assert_eq!(eval_string(r#"kebab_case "HelloWorld""#), "hello-world");
    assert_eq!(eval_string(r#"kebab_case "hello_world""#), "hello-world");
}

#[test]
fn case_builtins_with_iterator() {
    assert_eq!(
        eval_string(r#"range(1, 3) |> maps { "Item$_" } |> snake_case |> join ",""#),
        "item1,item2,item3"
    );
}

// ── compact ──────────────────────────────────────────────────────────

#[test]
fn compact_removes_undef_and_empty_strings() {
    assert_eq!(
        eval_string(r#"join ",", compact("a", undef, "b", "", "c")"#),
        "a,b,c"
    );
}

#[test]
fn compact_with_iterator() {
    assert_eq!(
        eval_string(r#"range(1, 5) |> maps { $_ % 2 == 0 ? $_ : undef } |> compact |> join ",""#),
        "2,4"
    );
}

// ── enumerate ────────────────────────────────────────────────────────

#[test]
fn enumerate_yields_index_value_pairs() {
    assert_eq!(
        eval_string(
            r#"range(0, 2) |> maps { chr(ord("a") + $_) } |> enumerate |> map { "$_->[0]:$_->[1]" } |> join ",""#
        ),
        "0:a,1:b,2:c"
    );
}

// ── chunk ────────────────────────────────────────────────────────────

#[test]
fn chunk_groups_elements() {
    assert_eq!(
        eval_string(r#"range(1, 5) |> chunk 2 |> map { join "-", @$_ } |> join ",""#),
        "1-2,3-4,5"
    );
}

// ── dedup ────────────────────────────────────────────────────────────

#[test]
fn dedup_removes_consecutive_duplicates() {
    assert_eq!(
        eval_string(r#"dedup(1, 1, 2, 3, 3, 3, 1) |> join ",""#),
        "1,2,3,1"
    );
}

// ── top ──────────────────────────────────────────────────────────────

#[test]
fn top_returns_highest_frequency_items() {
    assert_eq!(
        eval_string(
            r#"my $f = { a => 5, b => 10, c => 1 }; my $t = top 2, $f; join ",", keys %$t"#
        ),
        "b,a"
    );
}

#[test]
fn top_with_frequencies_pipe() {
    assert_eq!(
        eval_string(
            r#"my $t = ("x", "y", "x", "z", "x", "y") |> frequencies |> top 2; join ",", keys %$t"#
        ),
        "x,y"
    );
}
