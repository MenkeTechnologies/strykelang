//! Source-level proof that AOP advice bodies never fall back to the AST tree-walker.
//!
//! The previous v1 implementation ran advice bodies via `Interpreter::exec_block`
//! (a tree-walker). That path doesn't carry the bytecode compiler's `our`-qualified
//! name resolution, so `$count` inside an advice body wrote to a different storage
//! key (`count`) than the surrounding code (`main::count`). The fix lowers each
//! advice body to bytecode at compile time and dispatches it through
//! `run_block_region`, the same VM path used by `map { }` / `grep { }` blocks.
//!
//! This file is the immune system. If anyone re-introduces an `interp.exec_block`
//! call inside `dispatch_with_advice`, the assertion below fails — protecting the
//! invariant that advice executes on the bytecode VM, not the tree-walker.
//!
//! Mirror of zshrs `tests/tree_walker_absent.rs` (the file that pinned the deletion
//! of zshrs's `execute_simple` / `execute_pipeline` / `execute_list` / etc. tree
//! walkers when those constructs moved to the fusevm bytecode path).

use std::fs;
use std::path::PathBuf;

fn read_vm_rs() -> String {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("strykelang/vm.rs");
    fs::read_to_string(&p).unwrap_or_else(|e| panic!("cannot read {}: {}", p.display(), e))
}

/// Locate the `dispatch_with_advice` method body. Panics if the function moves
/// or is renamed without this test being updated — same defensive style as the
/// zshrs tests.
fn dispatch_with_advice_body(src: &str) -> &str {
    let needle = "fn dispatch_with_advice";
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("`{}` not found in strykelang/vm.rs", needle));
    // Find the opening `{` of the body, then walk balanced braces to its match.
    let bytes = src.as_bytes();
    let mut i = start;
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    assert!(i < bytes.len(), "no opening `{{` after {}", needle);
    let body_start = i + 1;
    let mut depth = 1usize;
    let mut j = body_start;
    while j < bytes.len() && depth > 0 {
        match bytes[j] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        j += 1;
    }
    assert_eq!(depth, 0, "unbalanced braces in dispatch_with_advice");
    &src[body_start..j - 1]
}

/// Strip `// …` line comments so we only inspect real code, not commentary
/// that happens to mention `exec_block` (e.g. a "we never call exec_block
/// here" reminder shouldn't trip the regression check).
fn strip_line_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let cut = line.find("//").unwrap_or(line.len());
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// The advice body must NOT be executed via the tree-walker. Any
/// `interp.exec_block` (or `self.interp.exec_block`) call inside
/// `dispatch_with_advice` is a regression: the body would lose access to
/// `our`-qualified scalars and other compile-time-resolved names.
#[test]
fn dispatch_with_advice_does_not_call_tree_walker_exec_block() {
    let src = read_vm_rs();
    let body = dispatch_with_advice_body(&src);
    let body = strip_line_comments(body);
    let needles = [
        "interp.exec_block",
        "interp.exec_block_with_tail",
        "interp.exec_block_no_scope",
        "interp.exec_block_no_scope_with_tail",
        "interp.exec_block_smart",
    ];
    for needle in &needles {
        assert!(
            !body.contains(needle),
            "dispatch_with_advice still calls `{}` — AOP advice bodies must run \
             through the bytecode VM (run_block_region), not the tree-walker. \
             See tests/tree_walker_absent_aop.rs for the rationale.",
            needle
        );
    }
}

/// The advice body MUST go through `run_block_region` (the VM bytecode dispatch
/// helper used by `map { }` / `grep { }` blocks). If this call disappears,
/// either the dispatch path was rewritten in a way that bypasses it (regression)
/// or moved to a helper — in which case update this test to point at the helper
/// rather than relax the invariant.
#[test]
fn dispatch_with_advice_routes_through_run_block_region() {
    let src = read_vm_rs();
    let body = dispatch_with_advice_body(&src);
    assert!(
        body.contains("run_block_region"),
        "dispatch_with_advice no longer calls `run_block_region` — the advice body \
         must execute through the bytecode VM. Either restore the call or update \
         this test to track the new helper. See tests/tree_walker_absent_aop.rs."
    );
}

/// A second invariant: `Op::RegisterAdvice` must record the body's chunk-block
/// index (`body_block_idx`) so the VM knows which compiled bytecode region to
/// dispatch at advice-firing time. If this field disappears from
/// `RuntimeAdviceDecl` or `Intercept`, advice bodies have nowhere to point.
#[test]
fn advice_decl_carries_a_compiled_block_index() {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("strykelang/bytecode.rs");
    let bc = fs::read_to_string(&p).expect("read strykelang/bytecode.rs");
    assert!(
        bc.contains("body_block_idx"),
        "RuntimeAdviceDecl must carry a `body_block_idx` so the VM can dispatch \
         the advice body via `run_block_region` instead of falling back to the \
         tree-walker. See tests/tree_walker_absent_aop.rs."
    );
}
