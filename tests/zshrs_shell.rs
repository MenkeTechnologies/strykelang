//! Integration tests for zshrs shell features

use std::process::Command;

fn run_zshrs(script: &str) -> String {
    let output = Command::new("cargo")
        .args(["run", "-q", "--bin", "zshrs", "--", "-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn test_word_splitting_array() {
    let output = run_zshrs(r#"arr=(one two three); for x in ${arr[@]}; do echo "$x"; done"#);
    assert!(output.contains("one"));
    assert!(output.contains("two"));
    assert!(output.contains("three"));
}

#[test]
fn test_word_splitting_variable() {
    let output = run_zshrs(r#"words="a b c"; for w in $words; do echo "$w"; done"#);
    assert!(output.contains("a"));
    assert!(output.contains("b"));
    assert!(output.contains("c"));
}

#[test]
fn test_glob_expansion() {
    let output = run_zshrs(r#"echo *.rs"#);
    assert!(output.contains("build.rs"));
}

#[test]
fn test_brace_expansion_list() {
    let output = run_zshrs(r#"echo {a,b,c}"#);
    assert_eq!(output.trim(), "a b c");
}

#[test]
fn test_brace_expansion_sequence() {
    let output = run_zshrs(r#"echo {1..5}"#);
    assert_eq!(output.trim(), "1 2 3 4 5");
}

#[test]
fn test_brace_expansion_with_prefix() {
    let output = run_zshrs(r#"echo file{1,2,3}.txt"#);
    assert_eq!(output.trim(), "file1.txt file2.txt file3.txt");
}

#[test]
fn test_heredoc() {
    let output = run_zshrs(
        r#"cat <<EOF
hello
world
EOF"#,
    );
    assert!(output.contains("hello"));
    assert!(output.contains("world"));
}

#[test]
fn test_heredoc_variable_expansion() {
    let output = run_zshrs(
        r#"x=test; cat <<EOF
value is $x
EOF"#,
    );
    assert!(output.contains("value is test"));
}

#[test]
fn test_herestring() {
    let output = run_zshrs(r#"cat <<< "hello world""#);
    assert!(output.contains("hello world"));
}

#[test]
fn test_alias() {
    let output = run_zshrs(r#"alias ll="ls -la"; alias"#);
    assert!(output.contains("alias ll='ls -la'"));
}

#[test]
fn test_unalias() {
    let output = run_zshrs(r#"alias ll="ls -la"; unalias ll; alias"#);
    assert!(!output.contains("ll="));
}

#[test]
fn test_type_builtin() {
    let output = run_zshrs(r#"type echo"#);
    assert!(output.contains("shell builtin"));
}

#[test]
fn test_let_arithmetic() {
    let output = run_zshrs(r#"let x=5+3; echo $x"#);
    assert_eq!(output.trim(), "8");
}

#[test]
fn test_arithmetic_command() {
    let output = run_zshrs(r#"((x = 5 + 3)); echo $x"#);
    assert_eq!(output.trim(), "8");
}

#[test]
fn test_arithmetic_comparison() {
    let output = run_zshrs(r#"((x = 5 < 10)); echo $x"#);
    assert_eq!(output.trim(), "1");
}

#[test]
fn test_arithmetic_increment() {
    let output = run_zshrs(r#"((x = 5)); ((x++)); echo $x"#);
    assert_eq!(output.trim(), "6");
}

#[test]
fn test_for_arith_loop() {
    let output = run_zshrs(r#"for ((i=0; i<5; i++)); do echo $i; done"#);
    assert!(output.contains("0"));
    assert!(output.contains("1"));
    assert!(output.contains("2"));
    assert!(output.contains("3"));
    assert!(output.contains("4"));
}

#[test]
fn test_conditional_numeric_lt() {
    let output = run_zshrs(r#"[[ 5 -lt 10 ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_conditional_numeric_gt() {
    let output = run_zshrs(r#"[[ 10 -gt 5 ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_conditional_string_equal() {
    let output = run_zshrs(r#"[[ "hello" == "hello" ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_conditional_string_not_equal() {
    let output = run_zshrs(r#"[[ "hello" != "world" ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_array_length() {
    let output = run_zshrs(r#"arr=(a b c d e); echo ${#arr[@]}"#);
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_array_element_access() {
    let output = run_zshrs(r#"arr=(one two three); echo ${arr[2]}"#);
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_parameter_expansion_default() {
    let output = run_zshrs(r#"echo ${unset:-default}"#);
    assert_eq!(output.trim(), "default");
}

#[test]
fn test_parameter_expansion_substring() {
    let output = run_zshrs(r#"x=hello; echo ${x:1:3}"#);
    assert_eq!(output.trim(), "ell");
}

#[test]
fn test_set_positional_params() {
    let output = run_zshrs(r#"set -- a b c; echo $1 $2 $3"#);
    assert_eq!(output.trim(), "a b c");
}
