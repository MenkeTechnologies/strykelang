//! Pretty-print parsed Perl back to source (`pe --fmt`).
//! Full AST round-trip: run `python3 tools/gen_fmt.py` and validate output before replacing this stub.

use crate::ast::*;

/// Best-effort one-line summary per statement.
pub fn format_program(p: &Program) -> String {
    p.statements
        .iter()
        .map(|s| format!("/* line {} */", s.line))
        .collect::<Vec<_>>()
        .join("\n")
}
