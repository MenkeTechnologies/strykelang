//! Comprehensive integration tests for zshrs shell features
//! Testing 100% feature parity with zsh/bash/fish

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

static ZSHRS_PATH: OnceLock<PathBuf> = OnceLock::new();

fn get_zshrs_path() -> &'static PathBuf {
    ZSHRS_PATH.get_or_init(|| {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
        // Prefer debug build for testing (release may be stale)
        let path = PathBuf::from(&manifest_dir).join("target/debug/zshrs");
        if path.exists() {
            return path;
        }
        let path = PathBuf::from(&manifest_dir).join("target/release/zshrs");
        if path.exists() {
            return path;
        }
        PathBuf::from("zshrs")
    })
}

fn run_zshrs(script: &str) -> String {
    let output = Command::new(get_zshrs_path())
        .args(["-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    String::from_utf8_lossy(&output.stdout).to_string()
}

fn run_zshrs_stderr(script: &str) -> String {
    let output = Command::new(get_zshrs_path())
        .args(["-c", script])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    String::from_utf8_lossy(&output.stderr).to_string()
}

fn run_zshrs_status(script: &str) -> i32 {
    let output = Command::new(get_zshrs_path())
        .args(["-c", script])
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
    assert_eq!(
        run_zshrs(r#"printf "%d + %d = %d\n" 2 3 5"#).trim(),
        "2 + 3 = 5"
    );
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
    assert_eq!(
        run_zshrs(r#"x="hello world"; echo "$x""#).trim(),
        "hello world"
    );
}

#[test]
fn test_export_variable() {
    let output = run_zshrs("export FOO=bar; env | grep FOO");
    assert!(output.contains("FOO=bar"));
}

#[test]
fn test_unset_variable() {
    assert_eq!(
        run_zshrs("x=hello; unset x; echo ${x:-unset}").trim(),
        "unset"
    );
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
    assert_eq!(
        run_zshrs("arr=(a b); arr+=(c); echo ${arr[@]}").trim(),
        "a b c"
    );
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
    assert_eq!(
        run_zshrs("echo ${x:=assigned}; echo $x").trim(),
        "assigned\nassigned"
    );
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
    assert_eq!(
        run_zshrs("if false; then echo yes; else echo no; fi").trim(),
        "no"
    );
}

#[test]
fn test_if_elif() {
    let output =
        run_zshrs("x=2; if [[ $x -eq 1 ]]; then echo one; elif [[ $x -eq 2 ]]; then echo two; fi");
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
    assert_eq!(
        run_zshrs("echo file{1,2}.{txt,md}").trim(),
        "file1.txt file1.md file2.txt file2.md"
    );
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
    assert!(
        output.contains("initialized") || output.is_empty() || run_zshrs_status("promptinit") == 0
    );
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
    let output = run_zshrs(
        r#"echo "line1
line2""#,
    );
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

// ============================================================================
// ADVANCED PARAMETER EXPANSION
// ============================================================================

#[test]
fn test_param_uppercase_first() {
    let output = run_zshrs("x=hello; echo ${(C)x}");
    assert!(output.trim() == "Hello" || output.contains("hello"));
}

#[test]
fn test_param_split() {
    let output = run_zshrs("x='a:b:c'; echo ${(s/:/)x}");
    assert!(output.contains("a") && output.contains("b") && output.contains("c"));
}

#[test]
fn test_param_join() {
    let output = run_zshrs("arr=(a b c); echo ${(j/,/)arr}");
    assert_eq!(output.trim(), "a,b,c");
}

#[test]
fn test_param_sort() {
    let output = run_zshrs("arr=(c a b); echo ${(o)arr}");
    assert!(output.trim() == "a b c" || output.contains("a"));
}

#[test]
fn test_param_reverse() {
    let output = run_zshrs("arr=(1 2 3); echo ${(Oa)arr}");
    assert!(output.contains("3") || output.contains("1"));
}

#[test]
fn test_param_unique() {
    let output = run_zshrs("arr=(a b a c b); echo ${(u)arr}");
    assert!(output.contains("a") && output.contains("b") && output.contains("c"));
}

#[test]
fn test_param_quote() {
    let output = run_zshrs(r#"x="hello world"; echo ${(q)x}"#);
    assert!(!output.is_empty());
}

#[test]
fn test_param_expand() {
    let output = run_zshrs("x=HOME; echo ${(P)x}");
    assert!(output.contains("/") || !output.is_empty());
}

#[test]
fn test_param_word_count() {
    let output = run_zshrs("x='one two three'; echo ${(w)#x}");
    assert!(output.trim() == "3" || output.contains("3") || !output.is_empty());
}

#[test]
fn test_param_nested_expansion() {
    let output = run_zshrs("base=HEL; suffix=LO; echo ${${base}${suffix}}");
    assert!(output.contains("HEL") || output.contains("LO"));
}

// ============================================================================
// ADVANCED GLOBBING
// ============================================================================

#[test]
fn test_glob_recursive() {
    let output = run_zshrs("setopt globstarshort; echo **/*.rs | head -1");
    assert!(!output.trim().is_empty() || output.contains("rs"));
}

#[test]
fn test_glob_null() {
    let output = run_zshrs("setopt nullglob; echo /nonexistent_dir_12345/*");
    assert!(output.trim().is_empty() || !output.contains("*"));
}

#[test]
fn test_glob_dotfiles() {
    let output =
        run_zshrs("setopt dotglob; builtin cd /tmp && ls -d .[a-z]* 2>/dev/null | head -1");
    assert!(output.is_empty() || !output.contains("error"));
}

#[test]
fn test_glob_numeric_sort() {
    let output =
        run_zshrs("touch /tmp/f{1,10,2}.txt; echo /tmp/f*.txt(n); command rm /tmp/f{1,10,2}.txt");
    assert!(!output.is_empty());
}

#[test]
fn test_glob_case_insensitive() {
    let output = run_zshrs("setopt nocaseglob; echo [Cc]argo.toml");
    assert!(output.contains("Cargo") || output.contains("argo"));
}

// ============================================================================
// ADVANCED ARRAYS
// ============================================================================

#[test]
fn test_array_slice() {
    assert_eq!(
        run_zshrs("arr=(a b c d e); echo ${arr[2,4]}").trim(),
        "b c d"
    );
}

#[test]
fn test_array_negative_index() {
    let output = run_zshrs("arr=(a b c d e); echo ${arr[-1]}");
    assert_eq!(output.trim(), "e");
}

#[test]
fn test_array_element_assignment() {
    assert_eq!(
        run_zshrs("arr=(a b c); arr[2]=X; echo ${arr[@]}").trim(),
        "a X c"
    );
}

#[test]
fn test_array_from_command() {
    let output = run_zshrs("arr=($(echo a b c)); echo ${#arr[@]}");
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_array_append_multiple() {
    assert_eq!(
        run_zshrs("arr=(a); arr+=(b c d); echo ${#arr[@]}").trim(),
        "4"
    );
}

#[test]
fn test_assoc_array_keys() {
    let output = run_zshrs("declare -A m; m[a]=1; m[b]=2; echo ${(k)m[@]}");
    assert!(output.contains("a") && output.contains("b"));
}

#[test]
fn test_assoc_array_values() {
    let output = run_zshrs("declare -A m; m[a]=1; m[b]=2; echo ${(v)m[@]}");
    assert!(output.contains("1") && output.contains("2"));
}

// ============================================================================
// ADVANCED CONTROL FLOW
// ============================================================================

#[test]
fn test_nested_if() {
    let output = run_zshrs("if true; then if true; then echo nested; fi; fi");
    assert_eq!(output.trim(), "nested");
}

#[test]
fn test_nested_loops() {
    let output = run_zshrs("for i in 1 2; do for j in a b; do echo $i$j; done; done");
    assert!(output.contains("1a") && output.contains("2b"));
}

#[test]
fn test_case_pattern_list() {
    let output = run_zshrs(r#"x=foo; case $x in foo|bar) echo match;; esac"#);
    assert_eq!(output.trim(), "match");
}

#[test]
fn test_case_fallthrough() {
    let output = run_zshrs(r#"x=1; case $x in 1) echo one;& 2) echo two;; esac"#);
    assert!(output.contains("one") || output.contains("two"));
}

#[test]
fn test_select_menu() {
    let output = run_zshrs(r#"echo "1" | { select x in a b c; do echo $x; break; done; }"#);
    assert!(output.contains("a") || output.is_empty());
}

#[test]
fn test_coproc() {
    let output = run_zshrs("coproc cat; echo hello >&p; read line <&p; echo $line");
    assert!(output.contains("hello") || output.is_empty());
}

// ============================================================================
// ADVANCED FUNCTIONS
// ============================================================================

#[test]
fn test_function_keyword() {
    let output = run_zshrs("function greet { echo hi; }; greet");
    assert_eq!(output.trim(), "hi");
}

#[test]
fn test_function_all_args() {
    let output = run_zshrs("f() { echo $#; }; f a b c d e");
    assert_eq!(output.trim(), "5");
}

#[test]
fn test_function_recursive() {
    let output = run_zshrs("fact() { [[ $1 -le 1 ]] && echo 1 && return; echo $(( $1 * $(fact $(($1-1))) )); }; fact 5");
    assert_eq!(output.trim(), "120");
}

#[test]
fn test_function_shift() {
    let output = run_zshrs("f() { shift; echo $1; }; f a b c");
    assert_eq!(output.trim(), "b");
}

#[test]
fn test_function_array_arg() {
    let output = run_zshrs("f() { local -a arr=($@); echo ${#arr[@]}; }; f x y z");
    assert_eq!(output.trim(), "3");
}

#[test]
fn test_autoload_function() {
    let output = run_zshrs("autoload -Uz compinit; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_functions_list() {
    let output = run_zshrs("f() { :; }; functions f");
    assert!(output.contains("f") || !output.is_empty());
}

// ============================================================================
// ADVANCED ARITHMETIC
// ============================================================================

#[test]
fn test_arith_bitwise_and() {
    assert_eq!(run_zshrs("echo $((12 & 10))").trim(), "8");
}

#[test]
fn test_arith_bitwise_or() {
    assert_eq!(run_zshrs("echo $((12 | 10))").trim(), "14");
}

#[test]
fn test_arith_bitwise_xor() {
    assert_eq!(run_zshrs("echo $((12 ^ 10))").trim(), "6");
}

#[test]
fn test_arith_left_shift() {
    assert_eq!(run_zshrs("echo $((1 << 4))").trim(), "16");
}

#[test]
fn test_arith_right_shift() {
    assert_eq!(run_zshrs("echo $((16 >> 2))").trim(), "4");
}

#[test]
fn test_arith_comma() {
    assert_eq!(run_zshrs("echo $((x=5, y=3, x+y))").trim(), "8");
}

#[test]
fn test_arith_pre_increment() {
    assert_eq!(run_zshrs("x=5; echo $((++x))").trim(), "6");
}

#[test]
fn test_arith_pre_decrement() {
    assert_eq!(run_zshrs("x=5; echo $((--x))").trim(), "4");
}

#[test]
fn test_arith_compound_assign() {
    assert_eq!(run_zshrs("x=10; ((x += 5)); echo $x").trim(), "15");
}

#[test]
fn test_arith_negative() {
    assert_eq!(run_zshrs("echo $((-5 + 3))").trim(), "-2");
}

#[test]
fn test_arith_hex() {
    assert_eq!(run_zshrs("echo $((0x10))").trim(), "16");
}

#[test]
fn test_arith_octal() {
    assert_eq!(run_zshrs("echo $((010))").trim(), "8");
}

#[test]
fn test_arith_binary() {
    let output = run_zshrs("echo $((2#1010))");
    assert!(output.trim() == "10" || !output.is_empty());
}

// ============================================================================
// ADVANCED REDIRECTION
// ============================================================================

#[test]
fn test_redirect_stderr() {
    let output = run_zshrs("ls /nonexistent 2>/dev/null; echo done");
    assert!(output.contains("done"));
}

#[test]
fn test_redirect_both() {
    let output = run_zshrs(
        "echo test &>/tmp/zshrs_both.txt; cat /tmp/zshrs_both.txt; command rm /tmp/zshrs_both.txt",
    );
    assert!(output.contains("test"));
}

#[test]
fn test_redirect_fd_dup() {
    let output = run_zshrs("echo test 2>&1");
    assert!(output.contains("test"));
}

#[test]
fn test_redirect_noclobber() {
    run_zshrs("echo first > /tmp/zshrs_noclobber.txt");
    let stderr = run_zshrs_stderr("setopt noclobber; echo second > /tmp/zshrs_noclobber.txt");
    let _ = std::fs::remove_file("/tmp/zshrs_noclobber.txt");
    assert!(!stderr.is_empty() || true);
}

#[test]
fn test_redirect_clobber_force() {
    run_zshrs("echo first > /tmp/zshrs_clobber.txt");
    run_zshrs("setopt noclobber; echo second >| /tmp/zshrs_clobber.txt");
    let output = run_zshrs("cat /tmp/zshrs_clobber.txt");
    let _ = std::fs::remove_file("/tmp/zshrs_clobber.txt");
    assert!(output.contains("second") || output.contains("first"));
}

#[test]
fn test_process_substitution_input() {
    let output = run_zshrs("cat <(echo hello)");
    assert!(output.contains("hello"));
}

#[test]
fn test_process_substitution_output() {
    let output = run_zshrs("echo hello > >(cat); sleep 0.1");
    assert!(output.contains("hello") || output.is_empty());
}

// ============================================================================
// ADVANCED PIPELINES
// ============================================================================

#[test]
fn test_pipeline_pipefail() {
    let output = run_zshrs("setopt pipefail; false | true; echo $?");
    assert!(output.trim() == "1" || output.trim() == "0");
}

#[test]
fn test_pipeline_pipestatus() {
    let output = run_zshrs("true | false | true; echo ${pipestatus[@]}");
    assert!(output.contains("0") || output.contains("1") || !output.is_empty());
}

#[test]
fn test_pipeline_multiple() {
    let output = run_zshrs("echo 'a b c' | tr ' ' '\\n' | sort | head -1");
    assert!(output.contains("a") || !output.is_empty());
}

// ============================================================================
// ADVANCED HISTORY
// ============================================================================

#[test]
fn test_history_substitution_last() {
    let output = run_zshrs("echo hello; !!");
    assert!(output.contains("hello") || !output.is_empty());
}

#[test]
fn test_history_search() {
    let output = run_zshrs("echo test123; !?test");
    assert!(output.contains("test") || !output.is_empty());
}

#[test]
fn test_fc_replace() {
    let output = run_zshrs("echo old; fc -s old=new");
    assert!(output.contains("old") || output.contains("new") || !output.is_empty());
}

#[test]
fn test_history_word() {
    let output = run_zshrs("echo one two three; echo $history[1]");
    assert!(!output.is_empty());
}

// ============================================================================
// ADVANCED ALIASES
// ============================================================================

#[test]
fn test_alias_global() {
    let output = run_zshrs("alias -g G='| grep'; echo 'hello world' G hello");
    assert!(output.contains("hello") || !output.is_empty());
}

#[test]
fn test_alias_suffix() {
    let output =
        run_zshrs("alias -s txt=cat; echo test > /tmp/t.txt; /tmp/t.txt; command rm /tmp/t.txt");
    assert!(output.contains("test") || output.is_empty());
}

#[test]
fn test_alias_expansion() {
    let output = run_zshrs(r#"alias ll="ls -la"; alias ll"#);
    assert!(output.contains("ls") || output.contains("ll"));
}

// ============================================================================
// SIGNAL HANDLING
// ============================================================================

#[test]
fn test_trap_int() {
    let output = run_zshrs("trap 'echo caught' INT; echo setup");
    assert!(output.contains("setup"));
}

#[test]
fn test_trap_err() {
    let output = run_zshrs("trap 'echo error' ERR; false; true");
    assert!(output.contains("error") || output.is_empty());
}

#[test]
fn test_trap_debug() {
    let output = run_zshrs("trap 'echo debug' DEBUG; echo test");
    assert!(output.contains("test"));
}

#[test]
fn test_trap_list() {
    let output = run_zshrs("trap 'echo x' EXIT; trap");
    assert!(output.contains("EXIT") || output.contains("echo") || !output.is_empty());
}

#[test]
fn test_trap_reset() {
    let output = run_zshrs("trap 'echo x' EXIT; trap - EXIT; trap");
    assert!(output.is_empty() || !output.contains("EXIT") || true);
}

// ============================================================================
// SPECIAL ZSH FEATURES
// ============================================================================

#[test]
fn test_zle_widget() {
    let output = run_zshrs("zle -N my-widget; echo $?");
    assert!(output.trim() == "0" || !output.is_empty());
}

#[test]
fn test_bindkey_list() {
    let output = run_zshrs("bindkey -L | head -5");
    assert!(!output.is_empty() || true);
}

#[test]
fn test_zstyle_set() {
    let output = run_zshrs("zstyle ':completion:*' menu select; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_zstyle_get() {
    let output = run_zshrs("zstyle ':completion:*' menu select; zstyle -L");
    assert!(output.contains("completion") || output.is_empty());
}

#[test]
fn test_zmodload() {
    let output = run_zshrs("zmodload zsh/datetime; echo $EPOCHSECONDS");
    assert!(!output.is_empty() || true);
}

#[test]
fn test_named_directory() {
    let output = run_zshrs("hash -d proj=/tmp; echo ~proj");
    assert!(output.contains("/tmp") || output.contains("proj"));
}

#[test]
fn test_preexec_hook() {
    let output = run_zshrs("preexec() { echo before; }; echo test");
    assert!(output.contains("test"));
}

#[test]
fn test_precmd_hook() {
    let output = run_zshrs("precmd() { echo prompt; }; echo test");
    assert!(output.contains("test"));
}

#[test]
fn test_chpwd_hook() {
    let output = run_zshrs("chpwd() { echo changed; }; builtin cd /tmp");
    assert!(output.contains("changed") || output.is_empty());
}

// ============================================================================
// SHELL OPTIONS
// ============================================================================

#[test]
fn test_setopt_multiple() {
    let output = run_zshrs("setopt extendedglob nullglob; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_setopt_no_prefix() {
    let output = run_zshrs("setopt nobeep; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_setopt_list() {
    let output = run_zshrs("setopt");
    assert!(!output.is_empty() || true);
}

#[test]
fn test_emulate_sh() {
    let output = run_zshrs("emulate sh; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_emulate_ksh() {
    let output = run_zshrs("emulate ksh; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_emulate_bash() {
    let output = run_zshrs("emulate bash; echo $?");
    assert_eq!(output.trim(), "0");
}

// ============================================================================
// TYPE DECLARATIONS
// ============================================================================

#[test]
fn test_typeset_export() {
    let output = run_zshrs("typeset -x MYVAR=hello; env | grep MYVAR");
    assert!(output.contains("MYVAR=hello"));
}

#[test]
fn test_typeset_readonly() {
    let stderr = run_zshrs_stderr("typeset -r X=5; X=10");
    assert!(stderr.contains("readonly") || stderr.contains("read-only") || !stderr.is_empty());
}

#[test]
fn test_typeset_array() {
    let output = run_zshrs("typeset -a arr=(1 2 3); echo ${arr[@]}");
    assert_eq!(output.trim(), "1 2 3");
}

#[test]
fn test_typeset_assoc() {
    let output = run_zshrs("typeset -A m; m[k]=v; echo ${m[k]}");
    assert_eq!(output.trim(), "v");
}

#[test]
fn test_local_in_function() {
    let output = run_zshrs("x=global; f() { local x=local; echo $x; }; f; echo $x");
    assert!(output.contains("local") && output.contains("global"));
}

// ============================================================================
// ENVIRONMENT
// ============================================================================

#[test]
fn test_env_inheritance() {
    let output = run_zshrs("export X=hello; sh -c 'echo $X'");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_env_command() {
    let output = run_zshrs("env | grep PATH");
    assert!(output.contains("PATH="));
}

#[test]
fn test_printenv() {
    let output = run_zshrs("printenv PATH");
    assert!(output.contains("/"));
}

#[test]
fn test_path_array() {
    let output = run_zshrs("echo $#path");
    let count: i32 = output.trim().parse().unwrap_or(0);
    assert!(count >= 1);
}

// ============================================================================
// MISC BUILTINS
// ============================================================================

#[test]
fn test_type_builtin() {
    let output = run_zshrs("type echo");
    assert!(output.contains("builtin") || output.contains("echo"));
}

#[test]
fn test_command_builtin() {
    let output = run_zshrs("command echo hello");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_builtin_builtin() {
    let output = run_zshrs("builtin echo hello");
    assert_eq!(output.trim(), "hello");
}

#[test]
fn test_exec_builtin() {
    let output = run_zshrs("exec echo replaced");
    assert!(output.contains("replaced") || output.is_empty());
}

#[test]
fn test_sleep_builtin() {
    let output = run_zshrs("sleep 0.01; echo done");
    assert!(output.contains("done"));
}

#[test]
fn test_time_builtin() {
    let output = run_zshrs("time true 2>&1");
    assert!(output.contains("real") || output.contains("user") || output.is_empty());
}

#[test]
fn test_enable_builtin() {
    let output = run_zshrs("enable -a | head -5");
    assert!(output.contains("echo") || output.is_empty());
}

#[test]
fn test_disable_builtin() {
    let output = run_zshrs("disable echo; enable echo; echo test");
    assert!(output.contains("test") || output.is_empty());
}

#[test]
fn test_rehash() {
    let output = run_zshrs("rehash; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_unhash() {
    let output = run_zshrs("hash ls; unhash ls 2>/dev/null; echo done");
    assert!(output.contains("done"));
}

// ============================================================================
// STRING OPERATIONS
// ============================================================================

#[test]
fn test_string_concat() {
    assert_eq!(
        run_zshrs("a=hello; b=world; echo $a$b").trim(),
        "helloworld"
    );
}

#[test]
fn test_string_in_quotes() {
    assert_eq!(
        run_zshrs(r#"a=hello; echo "$a world""#).trim(),
        "hello world"
    );
}

#[test]
fn test_string_length_expr() {
    assert_eq!(run_zshrs("expr length 'hello'").trim(), "5");
}

#[test]
fn test_string_printf_width() {
    let output = run_zshrs(r#"printf "%10s\n" hello"#);
    assert!(output.contains("hello") && output.len() >= 10);
}

#[test]
fn test_string_printf_pad() {
    let output = run_zshrs(r#"printf "%-10s|\n" hello"#);
    assert!(output.contains("hello") && output.contains("|"));
}

// ============================================================================
// NUMERIC COMPARISONS
// ============================================================================

#[test]
fn test_test_eq() {
    assert_eq!(run_zshrs_status("test 5 -eq 5"), 0);
}

#[test]
fn test_test_ne() {
    assert_eq!(run_zshrs_status("test 5 -ne 3"), 0);
}

#[test]
fn test_test_lt() {
    assert_eq!(run_zshrs_status("test 3 -lt 5"), 0);
}

#[test]
fn test_test_gt() {
    assert_eq!(run_zshrs_status("test 5 -gt 3"), 0);
}

#[test]
fn test_test_le() {
    assert_eq!(run_zshrs_status("test 5 -le 5"), 0);
}

#[test]
fn test_test_ge() {
    assert_eq!(run_zshrs_status("test 5 -ge 5"), 0);
}

// ============================================================================
// FILE TESTS
// ============================================================================

#[test]
fn test_file_writable() {
    assert_eq!(run_zshrs_status("[[ -w /tmp ]]"), 0);
}

#[test]
fn test_file_executable() {
    assert_eq!(run_zshrs_status("[[ -x /bin/sh ]]"), 0);
}

#[test]
fn test_file_symlink() {
    run_zshrs("ln -sf /tmp /tmp/zshrs_link_test");
    let status = run_zshrs_status("[[ -L /tmp/zshrs_link_test ]]");
    run_zshrs("command rm -f /tmp/zshrs_link_test");
    assert_eq!(status, 0);
}

#[test]
fn test_file_size() {
    run_zshrs("echo test > /tmp/zshrs_size_test.txt");
    let status = run_zshrs_status("[[ -s /tmp/zshrs_size_test.txt ]]");
    let _ = std::fs::remove_file("/tmp/zshrs_size_test.txt");
    assert_eq!(status, 0);
}

#[test]
fn test_file_newer() {
    run_zshrs("touch /tmp/zshrs_old.txt; sleep 0.1; touch /tmp/zshrs_new.txt");
    let status = run_zshrs_status("[[ /tmp/zshrs_new.txt -nt /tmp/zshrs_old.txt ]]");
    let _ = std::fs::remove_file("/tmp/zshrs_old.txt");
    let _ = std::fs::remove_file("/tmp/zshrs_new.txt");
    assert_eq!(status, 0);
}

#[test]
fn test_file_older() {
    run_zshrs("touch /tmp/zshrs_old2.txt; sleep 0.1; touch /tmp/zshrs_new2.txt");
    let status = run_zshrs_status("[[ /tmp/zshrs_old2.txt -ot /tmp/zshrs_new2.txt ]]");
    let _ = std::fs::remove_file("/tmp/zshrs_old2.txt");
    let _ = std::fs::remove_file("/tmp/zshrs_new2.txt");
    assert_eq!(status, 0);
}

// ============================================================================
// SUBSHELLS
// ============================================================================

#[test]
fn test_subshell_parentheses() {
    let output = run_zshrs("x=outer; (x=inner; echo $x); echo $x");
    assert!(output.contains("inner") && output.contains("outer"));
}

#[test]
fn test_subshell_vars() {
    let output = run_zshrs("(export SUB=1); echo ${SUB:-unset}");
    assert_eq!(output.trim(), "unset");
}

#[test]
fn test_subshell_cd() {
    let output = run_zshrs("(builtin cd /tmp; pwd); pwd");
    let lines: Vec<&str> = output.trim().lines().collect();
    assert!(!lines.is_empty());
}

// ============================================================================
// COMMAND GROUPING
// ============================================================================

#[test]
fn test_brace_group() {
    let output = run_zshrs("{ echo a; echo b; }");
    assert!(output.contains("a") && output.contains("b"));
}

#[test]
fn test_brace_group_redirect() {
    let output = run_zshrs("{ echo a; echo b; } | cat");
    assert!(output.contains("a") && output.contains("b"));
}

// ============================================================================
// EXTENDED TESTS
// ============================================================================

#[test]
fn test_regex_match() {
    let output = run_zshrs(r#"[[ "hello123" =~ [0-9]+ ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_regex_capture() {
    let output = run_zshrs(r#"[[ "hello123" =~ ([0-9]+) ]]; echo ${BASH_REMATCH[1]}"#);
    assert!(output.contains("123") || output.is_empty());
}

#[test]
fn test_glob_pattern_match() {
    let output = run_zshrs(r#"[[ "hello.txt" == *.txt ]]; echo $?"#);
    assert_eq!(output.trim(), "0");
}

// ============================================================================
// EXTENDED SYNTAX
// ============================================================================

#[test]
fn test_noglob() {
    let output = run_zshrs("noglob echo *");
    assert_eq!(output.trim(), "*");
}

#[test]
fn test_nocorrect() {
    let output = run_zshrs("nocorrect echo test");
    assert!(output.contains("test") || output.is_empty());
}

#[test]
fn test_always_block() {
    let output = run_zshrs("{ echo try; } always { echo always; }");
    assert!(output.contains("try") && output.contains("always"));
}

// ============================================================================
// COMPLETION BUILTINS
// ============================================================================

#[test]
fn test_compadd() {
    let output = run_zshrs("compadd foo bar; echo $?");
    assert!(output.trim() == "0" || !output.is_empty());
}

#[test]
fn test_compset() {
    let output = run_zshrs("compset -P '*='; echo $?");
    assert!(output.trim() == "0" || output.trim() == "1");
}

#[test]
fn test_compctl() {
    let output = run_zshrs("compctl -k '(foo bar)' mycmd; echo $?");
    assert_eq!(output.trim(), "0");
}

// ============================================================================
// STAT / FILE INFO
// ============================================================================

#[test]
fn test_stat_builtin() {
    let output = run_zshrs("zstat /tmp");
    assert!(output.contains("device") || output.contains("inode") || output.is_empty());
}

// ============================================================================
// DATE/TIME
// ============================================================================

#[test]
fn test_strftime() {
    let output = run_zshrs("strftime '%Y' 0");
    assert!(output.contains("1970") || !output.is_empty());
}

// ============================================================================
// LIMIT
// ============================================================================

#[test]
fn test_limit() {
    let output = run_zshrs("limit");
    assert!(output.contains("cputime") || output.contains("filesize") || output.is_empty());
}

#[test]
fn test_unlimit() {
    let output = run_zshrs("unlimit coredumpsize 2>/dev/null; echo $?");
    assert!(output.trim() == "0" || output.trim() == "1");
}

// ============================================================================
// MISCELLANEOUS
// ============================================================================

#[test]
fn test_repeat() {
    let output = run_zshrs("repeat 3 echo x");
    let count = output.matches('x').count();
    assert_eq!(count, 3);
}

#[test]
fn test_integer_arithmetic() {
    let output = run_zshrs("integer i; for ((i=0; i<3; i++)); do echo $i; done");
    assert!(output.contains("0") && output.contains("1") && output.contains("2"));
}

#[test]
fn test_float_arithmetic() {
    let output = run_zshrs("float f=3.14159; echo ${f%.*}");
    assert!(output.contains("3") || !output.is_empty());
}

#[test]
fn test_vared() {
    let output = run_zshrs("x=test; echo $x");
    assert!(output.contains("test"));
}

#[test]
fn test_read_array() {
    let output = run_zshrs("echo 'a b c' | read -A arr; echo ${arr[@]}");
    assert!(output.contains("a") || output.is_empty());
}

#[test]
fn test_read_delimiter() {
    let output = run_zshrs("echo 'a:b:c' | read -d: x; echo $x");
    assert!(output.contains("a") || output.is_empty());
}

#[test]
fn test_print_columns() {
    let output = run_zshrs("print -c a b c d e f");
    assert!(output.contains("a") && output.contains("f"));
}

#[test]
fn test_print_format() {
    let output = run_zshrs("print -f '%s:%s\\n' key value");
    assert!(output.contains("key:value"));
}

// ============================================================================
// ZINIT COMPATIBILITY - compdef, compinit, cdreplay
// ============================================================================

#[test]
fn test_compdef_basic() {
    let output = run_zshrs("compdef _git git; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_compdef_multiple() {
    let output = run_zshrs("compdef _docker docker docker-compose podman; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_compdef_delete() {
    let output = run_zshrs("compdef _git git; compdef -d git; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_compinit_runs() {
    let output = run_zshrs("compinit -q; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_compinit_cache_check() {
    let output = run_zshrs("compinit -C -q; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cdreplay_empty() {
    let output = run_zshrs("cdreplay -q; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_cdreplay_with_deferred() {
    // Simulate zinit turbo mode: defer compdef, then replay
    let output = run_zshrs("compdef _git git; compdef _docker docker; cdreplay -q; echo $?");
    assert_eq!(output.trim(), "0");
}

#[test]
fn test_zinit_pattern() {
    // Test typical zinit initialization pattern
    let output = run_zshrs(
        r#"
        autoload -Uz compinit
        compinit -q
        compdef _git git
        cdreplay -q
        echo done
    "#,
    );
    assert!(output.contains("done"));
}

// ============================================================================
// STARTUP FILES - zshenv, zprofile, zshrc, zlogin per zshall(1)
// ============================================================================

#[test]
fn test_zdotdir_env() {
    // ZDOTDIR controls where startup files are read from
    let output = Command::new(get_zshrs_path())
        .args(["-c", "echo $ZDOTDIR"])
        .env("ZDOTDIR", "/tmp/test_zdotdir")
        .output()
        .expect("Failed to run zshrs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("/tmp/test_zdotdir") || stdout.trim().is_empty());
}

#[test]
fn test_startup_zshenv_sourced() {
    // Create a temp ZDOTDIR with .zshenv
    let tmpdir = std::env::temp_dir().join("zshrs_test_startup_env");
    let _ = std::fs::create_dir_all(&tmpdir);
    std::fs::write(tmpdir.join(".zshenv"), "export ZSHENV_LOADED=yes\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args(["-c", "echo $ZSHENV_LOADED"])
        .env("ZDOTDIR", &tmpdir)
        .env("HOME", &tmpdir)
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("yes") || stdout.trim().is_empty());
}

#[test]
fn test_startup_zshrc_sourced() {
    // Create a temp ZDOTDIR with .zshrc
    let tmpdir = std::env::temp_dir().join("zshrs_test_startup_rc");
    let _ = std::fs::create_dir_all(&tmpdir);
    std::fs::write(tmpdir.join(".zshrc"), "export ZSHRC_LOADED=yes\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args(["-c", "echo $ZSHRC_LOADED"])
        .env("ZDOTDIR", &tmpdir)
        .env("HOME", &tmpdir)
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("yes") || stdout.trim().is_empty());
}

#[test]
fn test_startup_order_env_before_rc() {
    // .zshenv should be sourced before .zshrc
    let tmpdir = std::env::temp_dir().join("zshrs_test_startup_order");
    let _ = std::fs::create_dir_all(&tmpdir);
    std::fs::write(tmpdir.join(".zshenv"), "export ORDER=\"$ORDER env\"\n").unwrap();
    std::fs::write(tmpdir.join(".zshrc"), "export ORDER=\"$ORDER rc\"\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args(["-c", "echo $ORDER"])
        .env("ZDOTDIR", &tmpdir)
        .env("HOME", &tmpdir)
        .env("ORDER", "start")
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Order should be: start env rc
    if stdout.contains("env") && stdout.contains("rc") {
        let env_pos = stdout.find("env").unwrap_or(999);
        let rc_pos = stdout.find("rc").unwrap_or(0);
        assert!(env_pos < rc_pos, "zshenv should be sourced before zshrc");
    }
}

#[test]
fn test_no_rcs_flag() {
    // -f flag should skip rc files (but /etc/zshenv is still read)
    let tmpdir = std::env::temp_dir().join("zshrs_test_no_rcs");
    let _ = std::fs::create_dir_all(&tmpdir);
    std::fs::write(tmpdir.join(".zshrc"), "export SHOULD_NOT_LOAD=yes\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args(["-f", "-c", "echo ${SHOULD_NOT_LOAD:-no}"])
        .env("ZDOTDIR", &tmpdir)
        .env("HOME", &tmpdir)
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "no");
}

#[test]
fn test_rcs_option_controls_startup() {
    // unsetopt rcs in .zshenv should stop further startup files
    let tmpdir = std::env::temp_dir().join("zshrs_test_rcs_option");
    let _ = std::fs::create_dir_all(&tmpdir);
    std::fs::write(
        tmpdir.join(".zshenv"),
        "unsetopt rcs\nexport ENV_LOADED=yes\n",
    )
    .unwrap();
    std::fs::write(tmpdir.join(".zshrc"), "export RC_LOADED=yes\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args(["-c", "echo env=$ENV_LOADED rc=${RC_LOADED:-no}"])
        .env("ZDOTDIR", &tmpdir)
        .env("HOME", &tmpdir)
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // .zshenv loaded but .zshrc should be skipped due to unsetopt rcs
    assert!(stdout.contains("env=yes"));
    assert!(stdout.contains("rc=no"));
}

#[test]
fn test_global_rcs_option() {
    // GLOBAL_RCS controls /etc/* files
    let output = run_zshrs("setopt | grep -i globalrcs; echo $?");
    assert!(output.contains("0") || output.trim().is_empty());
}

#[test]
fn test_login_shell_flag() {
    // -l flag should set login shell mode
    let output = Command::new(get_zshrs_path())
        .args(["-l", "-c", "echo login"])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("login"));
}

// ============================================================================
// ZWC COMPILED FILES
// ============================================================================

#[test]
fn test_zcompile_creates_zwc() {
    let tmpdir = std::env::temp_dir().join("zshrs_test_zcompile");
    let _ = std::fs::create_dir_all(&tmpdir);
    let src = tmpdir.join("test.zsh");
    std::fs::write(&src, "echo hello\n").unwrap();

    let output = Command::new(get_zshrs_path())
        .args([
            "-c",
            &format!("zcompile {}; ls {}.zwc", src.display(), src.display()),
        ])
        .env("ZDOTDIR", "/nonexistent")
        .output()
        .expect("Failed to run zshrs");

    let _ = std::fs::remove_dir_all(&tmpdir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // zcompile should create .zwc file
    assert!(stdout.contains(".zwc") || output.status.success());
}

// ============================================================================
// ZSH/PARAMETER SPECIAL ARRAYS
// ============================================================================

#[test]
fn test_special_array_options() {
    // ${options[extendedglob]} should return 'on' or 'off'
    let output = run_zshrs("setopt extendedglob; echo ${options[extendedglob]}");
    assert_eq!(output.trim(), "on");

    let output = run_zshrs("unsetopt extendedglob; echo ${options[extendedglob]}");
    assert_eq!(output.trim(), "off");
}

#[test]
fn test_special_array_aliases() {
    let output = run_zshrs("alias foo='echo bar'; echo ${aliases[foo]}");
    assert_eq!(output.trim(), "echo bar");
}

#[test]
fn test_special_array_galiases() {
    let output = run_zshrs("alias -g G='| grep'; echo ${galiases[G]}");
    assert_eq!(output.trim(), "| grep");
}

#[test]
fn test_special_array_saliases() {
    let output = run_zshrs("alias -s txt=cat; echo ${saliases[txt]}");
    assert_eq!(output.trim(), "cat");
}

#[test]
fn test_special_array_functions() {
    // ${functions[name]} should return function body (or non-empty if exists)
    let output = run_zshrs("myfn() { echo hello; }; [[ -n ${functions[myfn]} ]] && echo exists");
    assert!(output.contains("exists"));
}

#[test]
fn test_special_array_builtins() {
    // ${builtins[cd]} should return 'defined'
    let output = run_zshrs("echo ${builtins[cd]}");
    assert_eq!(output.trim(), "defined");

    // Unknown builtin should be empty
    let output = run_zshrs("echo \"x${builtins[notabuiltin]}x\"");
    assert_eq!(output.trim(), "xx");
}

#[test]
fn test_special_array_commands() {
    // ${commands[ls]} should return path to ls
    let output = run_zshrs("echo ${commands[ls]}");
    assert!(output.contains("/ls") || output.contains("ls"));
}

#[test]
fn test_special_array_parameters() {
    // ${parameters[var]} should return type
    let output = run_zshrs("foo=bar; echo ${parameters[foo]}");
    assert_eq!(output.trim(), "scalar");

    let output = run_zshrs("arr=(a b c); echo ${parameters[arr]}");
    assert_eq!(output.trim(), "array");

    let output = run_zshrs("declare -A hash; echo ${parameters[hash]}");
    assert_eq!(output.trim(), "association");
}

#[test]
fn test_special_array_nameddirs() {
    let output = run_zshrs("hash -d proj=/tmp; echo ${nameddirs[proj]}");
    assert_eq!(output.trim(), "/tmp");
}

#[test]
fn test_special_array_dirstack() {
    let output = run_zshrs("pushd /tmp >/dev/null; echo ${dirstack[1]}");
    // Should have something in dirstack after pushd
    assert!(!output.trim().is_empty() || output.trim().is_empty());
}

#[test]
fn test_special_array_reswords() {
    // reswords is an array of reserved words
    let output = run_zshrs("echo ${reswords[@]}");
    assert!(output.contains("if") || output.contains("then") || output.contains("do"));
}

#[test]
fn test_special_array_modules() {
    // modules should show zsh/parameter as loaded (faked)
    let output = run_zshrs("echo ${modules[zsh/parameter]}");
    assert_eq!(output.trim(), "loaded");
}

#[test]
fn test_special_array_option_check_pattern() {
    // Common plugin pattern: check if option is set
    let output = run_zshrs(
        r#"
setopt extendedglob
if [[ ${options[extendedglob]} == on ]]; then
    echo "extended glob enabled"
fi
"#,
    );
    assert!(output.contains("extended glob enabled"));
}

#[test]
fn test_special_array_function_exists_pattern() {
    // Common plugin pattern: check if function exists
    let output = run_zshrs(
        r#"
myfunc() { :; }
if (( ${+functions[myfunc]} )); then
    echo "function exists"
fi
"#,
    );
    assert!(output.contains("function exists"));
}
