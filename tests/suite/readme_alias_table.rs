//! The README's alias table is a promise to the reader: every short spelling
//! listed there should work. It drifted from the implementation — `med`, `win`
//! and `zp` were documented as aliases for `median`, `windowed` and `zip` but
//! failed with "Undefined subroutine", because the list-builtin dispatch gate
//! is duplicated between `vm_helper.rs` and `vm.rs` and the VM's copy was
//! missing them.
//!
//! Two checks, both read from the README itself so the table stays the single
//! source of truth:
//!   1. every row has the six cells its header declares (two rows did not, and
//!      rendered misaligned on GitHub);
//!   2. every documented alias actually resolves.

use crate::common::eval_string;

/// Rows of the alias table, as cell vectors. Located by its header rather than
/// by line number so edits above it cannot silently move the window.
fn alias_table_rows() -> Vec<Vec<String>> {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("read README.md");
    let header = "| Alias | Function | Alias | Function | Alias | Function |";
    let start = readme
        .find(header)
        .expect("alias table header not found in README.md — update this test");

    let mut rows = Vec::new();
    for line in readme[start..].lines().skip(2) {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            break; // table ended
        }
        let inner = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|');
        rows.push(inner.split('|').map(|c| c.trim().to_string()).collect());
    }
    assert!(
        rows.len() > 30,
        "expected the full alias table, parsed only {} rows",
        rows.len()
    );
    rows
}

/// A markdown table renders correctly only when every row has the same number
/// of cells as its header. Two rows were short and displayed misaligned.
#[test]
fn readme_alias_table_rows_are_well_formed() {
    let malformed: Vec<String> = alias_table_rows()
        .iter()
        .filter(|cells| cells.len() != 6)
        .map(|cells| format!("{} cells: {:?}", cells.len(), cells))
        .collect();

    assert!(
        malformed.is_empty(),
        "alias table rows must have 6 cells to render correctly: {malformed:#?}"
    );
}

/// Every alias the README advertises has to resolve. This is the check that
/// `med` / `win` / `zp` failed: documented for users, undefined at runtime.
#[test]
fn every_alias_documented_in_the_readme_resolves() {
    let mut documented: Vec<String> = Vec::new();
    for cells in alias_table_rows() {
        // Columns pair up as (alias, function) three times across the row.
        for i in [0, 2, 4] {
            let (Some(alias), Some(function)) = (cells.get(i), cells.get(i + 1)) else {
                continue;
            };
            let (Some(alias), Some(_)) = (
                alias.strip_prefix('`').and_then(|a| a.strip_suffix('`')),
                function.strip_prefix('`').and_then(|f| f.strip_suffix('`')),
            ) else {
                continue; // section label, empty cell, or `~>` operator row
            };
            if alias.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                documented.push(alias.to_string());
            }
        }
    }
    documented.sort();
    documented.dedup();
    assert!(
        documented.len() > 80,
        "expected ~100 documented aliases, extracted {}",
        documented.len()
    );

    let unresolved: Vec<&String> = documented
        .iter()
        .filter(|alias| eval_string(&format!(r#"exists $all{{{alias}}} ? "yes" : "no""#)) != "yes")
        .collect();

    assert!(
        unresolved.is_empty(),
        "aliases documented in the README but absent from %all: {unresolved:?} — \
         either register them or stop advertising them"
    );
}
