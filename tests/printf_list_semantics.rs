//! `printf` takes ONE list whose first element is the format (perldoc -f
//! printf: "printf FILEHANDLE LIST"). Splitting the format off *before*
//! flattening that list broke every form where the format did not arrive as a
//! separate argument.
//!
//! Each expectation here was checked against system `perl`.

use stryke::vm_helper::VMHelper;

/// Run `code` and return what it printed.
fn out(code: &str) -> String {
    let mut vm = VMHelper::new();
    vm.begin_capture();
    stryke::parse_and_run_string(code, &mut vm).expect("program failed");
    vm.end_capture()
}

/// The parenthesized call form: the list arrives as one argument, so taking
/// args[0] as the format made the format the whole stringified list —
/// `printf("%s-%d\n", "b", 2)` printed "-0\nb2".
#[test]
fn parenthesized_call_uses_its_first_element_as_the_format() {
    assert_eq!(out(r#"printf("%s-%d\n", "b", 2);"#), "b-2\n");
}

/// The unparenthesized form was already correct and must stay so.
#[test]
fn bare_list_form_is_unchanged() {
    assert_eq!(out(r#"printf "%s-%d\n", "c", 3;"#), "c-3\n");
}

/// An array is its elements, so its first element is the format — the same rule,
/// and the reason the fix is a flatten-then-split rather than a parser special
/// case for parentheses.
#[test]
fn an_array_supplies_the_format_and_the_arguments() {
    assert_eq!(out(r#"my @a = ("%s=%d\n", "d", 4); printf(@a);"#), "d=4\n");
}

/// A format held in a scalar, with arguments after it.
#[test]
fn a_scalar_format_with_arguments() {
    assert_eq!(
        out(r#"my $f = "%05.2f\n"; printf($f, 3.14159);"#),
        "03.14\n"
    );
}

/// A format with no arguments formats nothing — it does not fall back to `$_`,
/// because the format was given.
#[test]
fn a_lone_format_consumes_no_arguments() {
    assert_eq!(out(r#"printf("%s\n", "single");"#), "single\n");
}

/// Bare `printf;` still takes its format from the topic.
#[test]
fn bare_printf_uses_the_topic_as_format() {
    assert_eq!(out(r#"$_ = "topic\n"; printf;"#), "topic\n");
}
