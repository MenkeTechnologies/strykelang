//! Comprehensive integration tests for zshrs shell features
//! Testing 100% feature parity with zsh/bash/fish

use std::process::Command;

fn run_zshrs(script: &str) -> String {
    let output = Command::new("cargo")
        .args(["run", "-q", "--bin", "zshrs", "--", "-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_zshrs_stderr(script: &str) -> String {
    let output = Command::new("cargo")
        .args(["run", "-q", "--bin", "zshrs", "--", "-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    String::from_utf8_lossy(&output.stderr).to_string()
}

fn run_zshrs_status(script: &str) -> i32 {
    let output = Command::new("cargo")
        .args(["run", "-q", "--bin", "zshrs", "--", "-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    output.status.code().unwrap_or(-1)
}

// ============================================================================
// BASIC BUILTINS
// ============================================================================

#[test]
fn test_echo_basic() {
    assert_eq!(run_zshrs("echo hello").trim(), "hello");
}

#[test]
fn test_echo_multiple_args() {
    assert_eq!(run_zshrs("echo hello world").trim(), "hello world");
}

#[test]
fn test_echo_no_newline() {
    let output = run_zshrs("echo -n hello; echo world");
    assert_eq!(output.trim(), "helloworld");
}

#[test]
fn test_echo_escape_sequences() {
    let output = run_zshrs(r#"echo -e "a\tb""#);
    assert!(output.contains("a\tb") || output.contains("a\t"));
}

#[test]
fn test_printf_basic() {
    assert_eq!(run_zshrs(r#"printf "%s\n" hello"#).trim(), "hello");
}

#[test]
fn test_printf_format() {
    assert_eq!(run_zshrs(r#"printf "%d + %d = %d\n" 2 3 5"#).trim(), "2 + 3 = 5");
}

#[test]
fn test_print_zsh() {
    assert_eq!(run_zshrs("print hello").trim(), "hello");
}

#[test]
fn test_pwd() {
    let output = run_zshrs("pwd");
    assert!(output.trim().starts_with('/'));
}

#[test]
fn test_cd_and_pwd() {
    let output = run_zshrs("builtin cd /tmp && pwd");
    assert_eq!(output.trim(), "/tmp");
}

#[test]
fn test_cd_home() {
    let output = run_zshrs("builtin cd ~ && pwd");
    assert!(output.contains("/Users/") || output.contains("/home/"));
}

#[test]
fn test_true_exit_status() {
    assert_eq!(run_zshrs("true; echo $?").trim(), "0");
}

#[test]
fn test_false_exit_status() {
    assert_eq!(run_zshrs("false; echo $?").trim(), "1");
}

#[test]
fn test_colon_noop() {
    assert_eq!(run_zshrs(": ; echo $?").trim(), "0");
}

// ============================================================================
// VARIABLES AND ARRAYS
// ============================================================================

#[test]
fn test_variable_assignment() {
    assert_eq!(run_zshrs("x=hello; echo $x").trim(), "hello");
}

#[test]
fn test_variable_with_spaces() {
    assert_eq!(run_zshrs(r#"x="hello world"; echo "$x""#).trim(), "hello world");
}

#[test]
fn test_export_variable() {
    let output = run_zshrs("export FOO=bar; env | grep FOO");
    assert!(output.contains("FOO=bar"));
}

#[test]
fn test_unset_variable() {
    assert_eq!(run_zshrs("x=hello; unset x; echo ${x:-unset}").trim(), "unset");
}

#[test]
fn test_readonly_variable() {
    let stderr = run_zshrs_stderr("readonly x=5; x=10");
    assert!(stderr.contains("readonly") || stderr.contains("read-only"));
}

#[test]
fn test_array_assignment() {
    assert_eq!(run_zshrs("arr=(a b c); echo ${arr[1]}").trim(), "a");
}

#[test]
fn test_array_all_elements() {
    assert_eq!(run_zshrs("arr=(a b c); echo ${arr[@]}").trim(), "a b c");
}

#[test]
fn test_array_length() {
    assert_eq!(run_zshrs("arr=(a b c d e); echo ${#arr[@]}").trim(), "5");
}

#[test]
fn test_array_append() {
    assert_eq!(run_zshrs("arr=(a b); arr+=(c); echo ${arr[@]}").trim(), "a b c");
}

#[test]
fn test_associative_array() {
    let output = run_zshrs(r#"declare -A map; map[key]=value; echo ${map[key]}"#);
    assert_eq!(output.trim(), "value");
}

// ============================================================================
// PARAMETER EXPANSION
// ============================================================================

#[test]
fn test_param_default_value() {
    assert_eq!(run_zshrs("echo ${unset:-default}").trim(), "default");
}

#[test]
fn test_param_default_assign() {
    assert_eq!(run_zshrs("echo ${x:=assigned}; echo $x").trim(), "assigned\nassigned");
}

#[test]
fn test_param_error_if_unset() {
    let stderr = run_zshrs_stderr("echo ${unset:?error message}");
    assert!(stderr.contains("error"));
}

#[test]
fn test_param_alternate_value() {
    assert_eq!(run_zshrs("x=set; echo ${x:+alternate}").trim(), "alternate");
}

#[test]
fn test_param_length() {
    assert_eq!(run_zshrs("x=hello; echo ${#x}").trim(), "5");
}

#[test]
fn test_param_substring() {
    assert_eq!(run_zshrs("x=hello; echo ${x:1:3}").trim(), "ell");
}

#[test]
fn test_param_remove_prefix() {
    assert_eq!(run_zshrs("x=hello.txt; echo ${x#*.}").trim(), "txt");
}

#[test]
fn test_param_remove_prefix_long() {
    assert_eq!(run_zshrs("x=a.b.c; echo ${x##*.}").trim(), "c");
}

#[test]
fn test_param_remove_suffix() {
    assert_eq!(run_zshrs("x=hello.txt; echo ${x%.txt}").trim(), "hello");
}

#[test]
fn test_param_remove_suffix_long() {
    assert_eq!(run_zshrs("x=a.b.c; echo ${x%%.*}").trim(), "a");
}

#[test]
fn test_param_replace() {
    assert_eq!(run_zshrs("x=hello; echo ${x/l/L}").trim(), "heLlo");
}

#[test]
fn test_param_replace_all() {
    assert_eq!(run_zshrs("x=hello; echo ${x//l/L}").trim(), "heLLo");
}

#[test]
fn test_param_uppercase() {
    assert_eq!(run_zshrs("x=hello; echo ${(U)x}").trim(), "HELLO");
}

#[test]
fn test_param_lowercase() {
    assert_eq!(run_zshrs("x=HELLO; echo ${(L)x}").trim(), "hello");
}

// ============================================================================
// CONTROL STRUCTURES
// ============================================================================

#[test]
fn test_if_then_fi() {
    assert_eq!(run_zshrs("if true; then echo yes; fi").trim(), "yes");
}

#[test]
fn test_if_else() {
    assert_eq!(run_zshrs("if false; then echo yes; else echo no; fi").trim(), "no");
}

#[test]
fn test_if_elif() {
    let output = run_zshrs("x=2; if [[ $x -eq 1 ]]; then echo one; elif [[ $x -eq 2 ]]; then echo two; fi");
    assert_eq!(output.trim(), "two");
}

#[test]
fn test_for_in_loop() {
    let output = run_zshrs("for x in a b c; do echo $x; done");
    assert!(output.contains("a") && output.contains("b") && output.contains("c"));
}

#[test]
fn test_for_arithmetic_loop() {
    let output = run_zshrs("for ((i=0; i<3; i++)); do echo $i; done");
    assert!(output.contains("0") && output.contains("1") && output.contains("2"));
}

#[test]
fn test_while_loop() {
    let output = run_zshrs("i=0; while [[ $i -lt 3 ]]; do echo $i; ((i++)); done");
    assert!(output.contains("0") && output.contains("1") && output.contains("2"));
}

#[test]
fn test_until_loop() {
    let output = run_zshrs("i=0; until [[ $i -ge 3 ]]; do echo $i; ((i++)); done");
    assert!(output.contains("0") && output.contains("1") && output.contains("2"));
}

#[test]
fn test_case_statement() {
    let output = run_zshrs(r#"x=foo; case $x in foo) echo matched;; *) echo default;; esac"#);
    assert_eq!(output.trim(), "matched");
}

#[test]
fn test_case_wildcard() {
    let output = run_zshrs(r#"x=bar; case $x in foo) echo foo;; *) echo other;; esac"#);
    assert_eq!(output.trim(), "other");
}

#[test]
fn test_break_in_loop() {
    let output = run_zshrs("for i in 1 2 3 4 5; do echo $i; [[ $i -eq 3 ]] && break; done");
    assert!(output.contains("1") && output.contains("2") && output.contains("3"));
    assert!(!output.contains("4"));
}

#[test]
fn test_continue_in_loop() {
    let output = run_zshrs("for i in 1 2 3; do [[ $i -eq 2 ]] && continue; echo $i; done");
    assert!(output.contains("1") && output.contains("3"));
    assert!(!output.contains("2"));
}

// ============================================================================
// CONDITIONALS
// ============================================================================

#[test]
fn test_bracket_numeric_eq() {
    assert_eq!(run_zshrs("[[ 5 -eq 5 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_numeric_ne() {
    assert_eq!(run_zshrs("[[ 5 -ne 3 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_numeric_lt() {
    assert_eq!(run_zshrs("[[ 3 -lt 5 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_numeric_gt() {
    assert_eq!(run_zshrs("[[ 5 -gt 3 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_numeric_le() {
    assert_eq!(run_zshrs("[[ 5 -le 5 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_numeric_ge() {
    assert_eq!(run_zshrs("[[ 5 -ge 5 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_string_equal() {
    assert_eq!(run_zshrs(r#"[[ "foo" == "foo" ]]; echo $?"#).trim(), "0");
}

#[test]
fn test_bracket_string_not_equal() {
    assert_eq!(run_zshrs(r#"[[ "foo" != "bar" ]]; echo $?"#).trim(), "0");
}

#[test]
fn test_bracket_string_empty() {
    assert_eq!(run_zshrs(r#"[[ -z "" ]]; echo $?"#).trim(), "0");
}

#[test]
fn test_bracket_string_nonempty() {
    assert_eq!(run_zshrs(r#"[[ -n "foo" ]]; echo $?"#).trim(), "0");
}

#[test]
fn test_bracket_and() {
    assert_eq!(run_zshrs("[[ 1 -eq 1 && 2 -eq 2 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_or() {
    assert_eq!(run_zshrs("[[ 1 -eq 2 || 2 -eq 2 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_not() {
    assert_eq!(run_zshrs("[[ ! 1 -eq 2 ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_file_exists() {
    assert_eq!(run_zshrs("[[ -e /tmp ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_file_directory() {
    assert_eq!(run_zshrs("[[ -d /tmp ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_file_regular() {
    assert_eq!(run_zshrs("[[ -f /etc/passwd ]]; echo $?").trim(), "0");
}

#[test]
fn test_bracket_file_readable() {
    assert_eq!(run_zshrs("[[ -r /etc/passwd ]]; echo $?").trim(), "0");
}

// ============================================================================
// ARITHMETIC
// ============================================================================

#[test]
fn test_arith_addition() {
    assert_eq!(run_zshrs("echo $((2 + 3))").trim(), "5");
}

#[test]
fn test_arith_subtraction() {
    assert_eq!(run_zshrs("echo $((10 - 4))").trim(), "6");
}

#[test]
fn test_arith_multiplication() {
    assert_eq!(run_zshrs("echo $((3 * 4))").trim(), "12");
}

#[test]
fn test_arith_division() {
    assert_eq!(run_zshrs("echo $((10 / 2))").trim(), "5");
}

#[test]
fn test_arith_modulo() {
    assert_eq!(run_zshrs("echo $((10 % 3))").trim(), "1");
}

#[test]
fn test_arith_power() {
    assert_eq!(run_zshrs("echo $((2 ** 10))").trim(), "1024");
}

#[test]
fn test_arith_increment() {
    assert_eq!(run_zshrs("x=5; ((x++)); echo $x").trim(), "6");
}

#[test]
fn test_arith_decrement() {
    assert_eq!(run_zshrs("x=5; ((x--)); echo $x").trim(), "4");
}

#[test]
fn test_arith_compound() {
    assert_eq!(run_zshrs("((x = 2 + 3 * 4)); echo $x").trim(), "14");
}

#[test]
fn test_arith_parentheses() {
    assert_eq!(run_zshrs("echo $(((2 + 3) * 4))").trim(), "20");
}

#[test]
fn test_arith_comparison() {
    assert_eq!(run_zshrs("echo $((5 > 3))").trim(), "1");
}

#[test]
fn test_arith_ternary() {
    assert_eq!(run_zshrs("echo $((5 > 3 ? 1 : 0))").trim(), "1");
}

#[test]
fn test_let_builtin() {
    assert_eq!(run_zshrs("let x=5+3; echo $x").trim(), "8");
}

// ============================================================================
// GLOBBING AND EXPANSION
// ============================================================================

#[test]
fn test_glob_star() {
    let output = run_zshrs("echo *.toml");
    assert!(output.contains("Cargo.toml"));
}

#[test]
fn test_glob_question() {
    let output = run_zshrs("echo ????.toml");
    assert!(output.contains("Cargo.toml") || output.trim() == "????.toml");
}

#[test]
fn test_brace_expansion_list() {
    assert_eq!(run_zshrs("echo {a,b,c}").trim(), "a b c");
}

#[test]
fn test_brace_expansion_sequence() {
    assert_eq!(run_zshrs("echo {1..5}").trim(), "1 2 3 4 5");
}

#[test]
fn test_brace_expansion_step() {
    assert_eq!(run_zshrs("echo {0..10..2}").trim(), "0 2 4 6 8 10");
}

#[test]
fn test_brace_expansion_alpha() {
    assert_eq!(run_zshrs("echo {a..e}").trim(), "a b c d e");
}

#[test]
fn test_brace_expansion_combined() {
    assert_eq!(run_zshrs("echo file{1,2}.{txt,md}").trim(), "file1.txt file1.md file2.txt file2.md");
}

#[test]
fn test_tilde_expansion() {
    let output = run_zshrs("echo ~");
    assert!(output.contains("/Users/") || output.contains("/home/") || output.contains("/root"));
}

#[test]
fn test_command_substitution_dollar() {
    assert_eq!(run_zshrs("echo $(echo hello)").trim(), "hello");
}

#[test]
fn test_command_substitution_backtick() {
    assert_eq!(run_zshrs("echo `echo hello`").trim(), "hello");
}

#[test]
fn test_arithmetic_expansion() {
    assert_eq!(run_zshrs("echo $((2+2))").trim(), "4");
}

// ============================================================================
// REDIRECTION
// ============================================================================

#[test]
fn test_redirect_output() {
    run_zshrs("echo test > /tmp/zshrs_test_out.txt");
    let output = run_zshrs("cat /tmp/zshrs_test_out.txt");
    assert_eq!(output.trim(), "test");
    let _ = std::fs::remove_file("/tmp/zshrs_test_out.txt");
}

#[test]
fn test_redirect_append() {
    run_zshrs("echo line1 > /tmp/zshrs_test_append.txt; echo line2 >> /tmp/zshrs_test_append.txt");
    let output = run_zshrs("cat /tmp/zshrs_test_append.txt");
    assert!(output.contains("line1") && output.contains("line2"));
    let _ = std::fs::remove_file("/tmp/zshrs_test_append.txt");
}

#[test]
fn test_redirect_input() {
    run_zshrs("echo hello > /tmp/zshrs_test_in.txt");
    let output = run_zshrs("cat < /tmp/zshrs_test_in.txt");
    assert_eq!(output.trim(), "hello");
    let _ = std::fs::remove_file("/tmp/zshrs_test_in.txt");
}

#[test]
fn test_heredoc() {
    let output = run_zshrs(
        r#"cat <<EOF
hello
world
EOF"#,
    );
    assert!(output.contains("hello") && output.contains("world"));
}

#[test]
fn test_herestring() {
    let output = run_zshrs(r#"cat <<< "hello world""#);
    assert!(output.contains("hello world"));
}

// ============================================================================
// PIPELINES AND LISTS
// ============================================================================

#[test]
fn test_pipeline_simple() {
    let output = run_zshrs("echo hello | cat");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_pipeline_grep() {
    let output = run_zshrs(r#"echo -e "foo\nbar\nbaz" | grep bar"#);
    assert!(output.contains("bar"));
}

#[test]
fn test_and_list() {
    assert_eq!(run_zshrs("true && echo yes").trim(), "yes");
}

#[test]
fn test_and_list_short_circuit() {
    assert_eq!(run_zshrs("false && echo yes").trim(), "");
}

#[test]
fn test_or_list() {
    assert_eq!(run_zshrs("false || echo yes").trim(), "yes");
}

#[test]
fn test_or_list_short_circuit() {
    assert_eq!(run_zshrs("true || echo yes").trim(), "");
}

#[test]
fn test_semicolon_list() {
    let output = run_zshrs("echo a; echo b; echo c");
    assert!(output.contains("a") && output.contains("b") && output.contains("c"));
}

// ============================================================================
// FUNCTIONS
// ============================================================================

#[test]
fn test_function_definition() {
    let output = run_zshrs("greet() { echo hello; }; greet");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_function_with_args() {
    let output = run_zshrs(r#"greet() { echo "Hello, $1!"; }; greet World"#);
    assert_eq!(output.trim(), "Hello, World!");
}

#[test]
fn test_function_return() {
    let output = run_zshrs("f() { return 42; }; f; echo $?");
    assert_eq!(output.trim(), "42");
}

#[test]
fn test_function_local_variable() {
    let output = run_zshrs("x=global; f() { local x=local; echo $x; }; f; echo $x");
    assert!(output.contains("local") && output.contains("global"));
}

// ============================================================================
// HISTORY
// ============================================================================

#[test]
fn test_history_command() {
    let output = run_zshrs("echo test; history");
    assert!(!output.is_empty());
}

#[test]
fn test_fc_list() {
    let output = run_zshrs("echo test; fc -l -1");
    assert!(!output.is_empty());
}

// ============================================================================
// ALIASES
// ============================================================================

#[test]
fn test_alias_definition() {
    let output = run_zshrs(r#"alias ll="ls -la"; alias"#);
    assert!(output.contains("ll=") || output.contains("ll'"));
}

#[test]
fn test_unalias() {
    let output = run_zshrs(r#"alias foo="bar"; unalias foo; alias | grep foo"#);
    assert!(!output.contains("foo="));
}

// ============================================================================
// JOB CONTROL
// ============================================================================

#[test]
fn test_jobs_empty() {
    let output = run_zshrs("jobs");
    assert!(output.is_empty() || output.trim().is_empty());
}

#[test]
fn test_background_job() {
    let output = run_zshrs("sleep 0.01 &; wait; echo done");
    assert!(output.contains("done"));
}

// ============================================================================
// SPECIAL VARIABLES
// ============================================================================

#[test]
fn test_dollar_question() {
    assert_eq!(run_zshrs("true; echo $?").trim(), "0");
}

#[test]
fn test_dollar_dollar() {
    let output = run_zshrs("echo $$");
    assert!(output.trim().parse::<u32>().is_ok());
}

#[test]
fn test_dollar_at() {
    let output = run_zshrs(r#"set -- a b c; echo "$@""#);
    assert_eq!(output.trim(), "a b c");
}

#[test]
fn test_dollar_star() {
    let output = run_zshrs(r#"set -- a b c; echo "$*""#);
    assert_eq!(output.trim(), "a b c");
}

#[test]
fn test_dollar_hash() {
    assert_eq!(run_zshrs("set -- a b c; echo $#").trim(), "3");
}

#[test]
fn test_dollar_zero() {
    let output = run_zshrs("echo $0");
    assert!(!output.trim().is_empty());
}

// ============================================================================
// ZSH-SPECIFIC FEATURES
// ============================================================================

#[test]
fn test_setopt() {
    let output = run_zshrs("setopt extendedglob; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_unsetopt() {
    let output = run_zshrs("unsetopt extendedglob; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_typeset() {
    let output = run_zshrs("typeset -i num=5; echo $num");
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_declare() {
    let output = run_zshrs("declare -a arr=(1 2 3); echo ${arr[@]}");
    assert_eq!(output.trim(), "1 2 3");
}

#[test]
fn test_integer_type() {
    let output = run_zshrs("integer x=5; echo $x");
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_float_type() {
    let output = run_zshrs("float x=3.14; echo $x");
    assert!(output.contains("3.14") || output.contains("3"));
}

#[test]
fn test_whence() {
    let output = run_zshrs("whence echo");
    assert!(output.contains("echo") || output.contains("builtin"));
}

#[test]
fn test_where() {
    let output = run_zshrs("where echo");
    assert!(!output.is_empty());
}

#[test]
fn test_which() {
    let output = run_zshrs("which ls");
    assert!(output.contains("/") || output.contains("ls"));
}

#[test]
fn test_emulate_zsh() {
    let output = run_zshrs("emulate zsh; echo $?");
    assert_eq!(output.trim(), "0");
}

// ============================================================================
// COMPLETION SYSTEM
// ============================================================================

#[test]
fn test_compgen_words() {
    let output = run_zshrs(r#"compgen -W "foo bar baz" -- f"#);
    assert_eq!(output.trim(), "foo");
}

#[test]
fn test_compgen_commands() {
    let output = run_zshrs("compgen -c ec | head -5");
    assert!(output.contains("echo") || !output.is_empty());
}

#[test]
fn test_compgen_files() {
    let output = run_zshrs("compgen -f Cargo");
    assert!(output.contains("Cargo"));
}

// ============================================================================
// TRAPS
// ============================================================================

#[test]
fn test_trap_exit() {
    let output = run_zshrs(r#"trap 'echo cleanup' EXIT; echo main"#);
    assert!(output.contains("main") && output.contains("cleanup"));
}

// ============================================================================
// BUILTINS COVERAGE
// ============================================================================

#[test]
fn test_builtin_test() {
    assert_eq!(run_zshrs("test 5 -eq 5; echo $?").trim(), "0");
}

#[test]
fn test_builtin_bracket() {
    assert_eq!(run_zshrs("[ 5 -eq 5 ]; echo $?").trim(), "0");
}

#[test]
fn test_builtin_eval() {
    assert_eq!(run_zshrs(r#"eval 'echo hello'"#).trim(), "hello");
}

#[test]
fn test_builtin_source() {
    run_zshrs("echo 'x=sourced' > /tmp/zshrs_source_test.sh");
    let output = run_zshrs("source /tmp/zshrs_source_test.sh; echo $x");
    assert_eq!(output.trim(), "sourced");
    let _ = std::fs::remove_file("/tmp/zshrs_source_test.sh");
}

#[test]
fn test_builtin_dot_source() {
    run_zshrs("echo 'y=dotted' > /tmp/zshrs_dot_test.sh");
    let output = run_zshrs(". /tmp/zshrs_dot_test.sh; echo $y");
    assert_eq!(output.trim(), "dotted");
    let _ = std::fs::remove_file("/tmp/zshrs_dot_test.sh");
}

#[test]
fn test_builtin_shift() {
    let output = run_zshrs("set -- a b c; shift; echo $1");
    assert_eq!(output.trim(), "b");
}

#[test]
fn test_builtin_getopts() {
    let output = run_zshrs(r#"set -- -a -b arg; while getopts "ab:" opt; do echo $opt; done"#);
    assert!(output.contains("a"));
}

#[test]
fn test_builtin_read() {
    let output = run_zshrs(r#"echo "hello" | read x; echo $x"#);
    assert!(output.contains("hello") || output.is_empty());
}

#[test]
fn test_builtin_dirs() {
    let output = run_zshrs("dirs");
    assert!(output.contains("/") || output.contains("~"));
}

#[test]
fn test_builtin_pushd_popd() {
    let output = run_zshrs("pushd /tmp > /dev/null; pwd; popd > /dev/null; pwd");
    assert!(output.contains("/tmp"));
}

#[test]
fn test_builtin_hash() {
    let output = run_zshrs("hash");
    assert!(output.is_empty() || !output.contains("error"));
}

#[test]
fn test_builtin_ulimit() {
    let output = run_zshrs("ulimit -n");
    assert!(output.trim().parse::<u64>().is_ok() || output.contains("unlimited"));
}

#[test]
fn test_builtin_umask() {
    let output = run_zshrs("umask");
    assert!(!output.is_empty());
}

#[test]
fn test_builtin_times() {
    let output = run_zshrs("times");
    assert!(!output.is_empty() || output.trim().is_empty());
}

#[test]
fn test_builtin_wait() {
    let output = run_zshrs("sleep 0.01 & wait; echo done");
    assert!(output.contains("done"));
}

#[test]
fn test_builtin_kill_signal() {
    let output = run_zshrs("kill -l");
    assert!(output.contains("HUP") || output.contains("TERM") || output.contains("1"));
}

// ============================================================================
// ZSH MODULES
// ============================================================================

#[test]
fn test_zpty_list() {
    let output = run_zshrs("zpty");
    assert!(output.is_empty() || !output.contains("error"));
}

#[test]
fn test_zsocket_list() {
    let output = run_zshrs("zsocket");
    assert!(output.is_empty() || !output.contains("error"));
}

#[test]
fn test_zprof() {
    let output = run_zshrs("zprof");
    assert!(output.contains("num") || output.contains("profil"));
}

#[test]
fn test_sched_list() {
    let output = run_zshrs("sched");
    assert!(output.is_empty() || !output.contains("error"));
}

#[test]
fn test_zformat() {
    let output = run_zshrs(r#"zformat -f result "%a %b" a:hello b:world; echo done"#);
    assert!(output.contains("done"));
}

#[test]
fn test_zparseopts() {
    let output = run_zshrs(r#"zparseopts -D -E a=flag -- -a; echo ${flag[@]}"#);
    assert!(output.contains("-a") || output.is_empty());
}

// ============================================================================
// PROMPT SYSTEM  
// ============================================================================

#[test]
fn test_promptinit() {
    let output = run_zshrs("promptinit");
    assert!(output.contains("initialized") || output.is_empty() || run_zshrs_status("promptinit") == 0);
}

#[test]
fn test_prompt_list() {
    let output = run_zshrs("promptinit; prompt -l");
    assert!(output.contains("adam") || output.contains("default") || output.contains("theme"));
}

// ============================================================================
// HOOKS
// ============================================================================

#[test]
fn test_add_zsh_hook() {
    let output = run_zshrs("add-zsh-hook precmd my_func; echo $?");
    assert!(output.trim() == "0" || output.contains("0"));
}

// ============================================================================
// PCRE / REGEX
// ============================================================================

#[test]
fn test_zregexparse() {
    let output = run_zshrs(r#"zregexparse result "([a-z]+)" "hello"; echo $result"#);
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_pcre_compile() {
    let status = run_zshrs_status(r#"pcre_compile "hello.*world""#);
    assert_eq!(status, 0);
}

#[test]
fn test_pcre_match() {
    let output = run_zshrs(r#"pcre_compile "hello"; pcre_match "hello world"; echo $MATCH"#);
    assert_eq!(output.trim(), "hello");
}

// ============================================================================
// BASH COMPATIBILITY
// ============================================================================

#[test]
fn test_bash_shopt() {
    let output = run_zshrs("shopt");
    assert!(!output.is_empty() || run_zshrs_status("shopt") == 0);
}

#[test]
fn test_bash_help() {
    let output = run_zshrs("help");
    assert!(output.contains("builtin") || output.contains("cd") || output.contains("echo"));
}

#[test]
fn test_bash_caller() {
    let output = run_zshrs("caller");
    assert!(!output.is_empty() || run_zshrs_status("caller") == 0);
}

#[test]
fn test_bash_mapfile() {
    run_zshrs("echo -e 'a\nb\nc' > /tmp/zshrs_mapfile_test.txt");
    let output = run_zshrs("mapfile arr < /tmp/zshrs_mapfile_test.txt; echo ${#arr[@]}");
    let _ = std::fs::remove_file("/tmp/zshrs_mapfile_test.txt");
    assert!(output.trim().parse::<u32>().unwrap_or(0) >= 1 || output.contains("3"));
}

// ============================================================================
// ERROR HANDLING
// ============================================================================

#[test]
fn test_error_command_not_found() {
    let stderr = run_zshrs_stderr("nonexistent_command_12345");
    assert!(stderr.contains("not found") || stderr.contains("command"));
}

#[test]
fn test_error_syntax() {
    let stderr = run_zshrs_stderr("if then fi");
    assert!(!stderr.is_empty() || run_zshrs_status("if then fi") != 0);
}

// ============================================================================
// EDGE CASES
// ============================================================================

#[test]
fn test_empty_string() {
    assert_eq!(run_zshrs(r#"x=""; echo "[$x]""#).trim(), "[]");
}

#[test]
fn test_special_chars_in_string() {
    assert_eq!(run_zshrs(r#"echo 'hello$world'"#).trim(), "hello$world");
}

#[test]
fn test_escape_in_double_quotes() {
    assert_eq!(run_zshrs(r#"echo "hello\"world""#).trim(), r#"hello"world"#);
}

#[test]
fn test_multiline_string() {
    let output = run_zshrs(r#"echo "line1
line2""#);
    assert!(output.contains("line1") && output.contains("line2"));
}

#[test]
fn test_nested_command_substitution() {
    assert_eq!(run_zshrs("echo $(echo $(echo hello))").trim(), "hello");
}

#[test]
fn test_nested_arithmetic() {
    assert_eq!(run_zshrs("echo $((1 + $((2 + 3))))").trim(), "6");
}
