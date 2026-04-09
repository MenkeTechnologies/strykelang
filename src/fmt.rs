//! Pretty-print parsed Perl back to source (`pe --fmt`).
//! Full AST round-trip lives in `tools/gen_fmt.py` (regenerate when `ast.rs` changes).

use crate::ast::*;

/// Best-effort one-line summary per statement (avoids maintaining a large formatter here).
pub fn format_program(p: &Program) -> String {
    p.statements
        .iter()
        .map(format_statement_stub)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_statement_stub(s: &Statement) -> String {
    format!("/* line {} */", s.line)
}
