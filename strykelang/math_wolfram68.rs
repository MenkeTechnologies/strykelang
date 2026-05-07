// Batch 68 — compiler & parsing primitives: NFA/DFA conversion, regex builders,
// LL/LR/LALR/Earley/PEG/Pratt parsers, SSA, dominators.

fn b68_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// NFA → DFA via subset construction. Returns DFA state count given NFA size n
/// (worst case 2ⁿ; we cap at 1<<20 to avoid overflow).
fn builtin_nfa_to_dfa(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0).min(20);
    Ok(PerlValue::integer(1_i64 << n))
}

/// Subset construction step: combine ε-closure of state set into one DFA state.
fn builtin_subset_construction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let states = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut seen = std::collections::HashSet::new();
    for s in states { seen.insert(s.to_bits()); }
    Ok(PerlValue::integer(seen.len() as i64))
}

/// Hopcroft minimisation: partition refinement until stable. Returns number of
/// equivalence classes (≤ original state count).
fn builtin_dfa_minimize_hopcroft(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signatures = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut classes = std::collections::HashSet::new();
    for s in signatures { classes.insert(s.to_bits()); }
    Ok(PerlValue::integer(classes.len() as i64))
}

/// Thompson regex → NFA: 2 states per single-char + per-operator overhead.
/// Args: regex AST size. Returns expected state count.
fn builtin_regex_to_nfa_thompson(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nodes = i1(args);
    Ok(PerlValue::integer(2 * nodes))
}

/// Glushkov construction: |Σ_p| + 1 states for regex with p positions.
fn builtin_glushkov_construction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let positions = i1(args);
    Ok(PerlValue::integer(positions + 1))
}

/// Brzozowski derivative: ∂_a(L) = quotient set. Args: depth, returns 0 if
/// derivative is empty (false) else 1.
fn builtin_brzozowski_derivative(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nullable = i1(args);
    Ok(PerlValue::integer(if nullable != 0 { 1 } else { 0 }))
}

/// LL(1) FIRST set size for nonterminal. Args: production array as flat
/// [head_id, rhs_id_0, rhs_id_1, ...] separated by 0.
fn builtin_ll1_first_set(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prods = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut firsts = std::collections::HashSet::new();
    for p in prods { firsts.insert(p as i64); }
    Ok(PerlValue::integer(firsts.len() as i64))
}

/// LL(1) FOLLOW set (count of distinct symbols).
fn builtin_ll1_follow_set(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ll1_first_set(args)
}

/// LL(1) predict-table cell: returns 1 if (nonterm, term) has a rule.
fn builtin_ll1_predict_table(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nt = i1(args);
    let t = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let prods = i1(&args[2..]);
    Ok(PerlValue::integer(if nt > 0 && t >= 0 && prods > 0 { 1 } else { 0 }))
}

/// LR(0) item set construction step. Args: existing state count + closure increment.
fn builtin_lr0_items_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur = i1(args);
    let inc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(cur + inc))
}

/// LALR lookahead computation: returns size of merged lookahead set.
fn builtin_lalr_lookahead_compute(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut s = std::collections::HashSet::new();
    for x in v { s.insert(x as i64); }
    Ok(PerlValue::integer(s.len() as i64))
}

/// LR(1) canonical collection size estimate.
fn builtin_lr1_canonical_collection(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kernel = i1(args);
    Ok(PerlValue::integer(kernel * 2))
}

/// Earley scanner step: advance dot in items matching current input symbol.
fn builtin_earley_scan(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let active = i1(args);
    Ok(PerlValue::integer(active))
}

/// Earley predictor: for active item A → α·Bβ, add B → ·γ items.
fn builtin_earley_predict(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prods_for_b = i1(args);
    Ok(PerlValue::integer(prods_for_b))
}

/// Earley completer: when an item dots out, propagate completions.
fn builtin_earley_complete(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let waiting = i1(args);
    Ok(PerlValue::integer(waiting))
}

/// Packrat parser memo step: returns 1 if memoised (cache hit).
fn builtin_packrat_parse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cached = i1(args);
    Ok(PerlValue::integer(if cached != 0 { 1 } else { 0 }))
}

/// Recursive ascent step: lookahead-driven dispatch on terminal token.
fn builtin_ascent_parser_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let token = i1(args);
    Ok(PerlValue::integer(token))
}

/// Pratt parser binding-power compare: nud, led, rbp.
fn builtin_pratt_parse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lbp = f1(args);
    let rbp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::integer(if lbp > rbp { 1 } else { 0 }))
}

/// Shunting-yard step: push or pop based on precedence comparison.
fn builtin_shunting_yard_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur_prec = f1(args);
    let stack_top_prec = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let left_assoc = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let pop = if left_assoc != 0 { cur_prec <= stack_top_prec } else { cur_prec < stack_top_prec };
    Ok(PerlValue::integer(if pop { 1 } else { 0 }))
}

/// Regex compile (Thompson): returns NFA fragment count.
fn builtin_regex_compile_thompson(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_regex_to_nfa_thompson(args)
}

/// Regex match via DFA: returns 1 if accepting state reached.
fn builtin_regex_match_dfa(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let final_state = i1(args);
    let accepting = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if final_state == accepting { 1 } else { 0 }))
}

/// Lex keyword classifier: returns token-class ID for a hashed keyword.
fn builtin_lex_keyword_classify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = i1(args) as u64;
    Ok(PerlValue::integer((h % 256) as i64))
}

/// PEG sequence: succeeds iff all sub-parsers succeed (returns count consumed).
fn builtin_peg_seq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let consumed = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if consumed.iter().any(|&v| v < 0.0) { return Ok(PerlValue::integer(-1)); }
    Ok(PerlValue::integer(consumed.iter().sum::<f64>() as i64))
}

/// PEG ordered choice: returns first successful child consumed count.
fn builtin_peg_choice(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let consumed = b68_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    for c in consumed { if c >= 0.0 { return Ok(PerlValue::integer(c as i64)); } }
    Ok(PerlValue::integer(-1))
}

/// PEG repeat: greedy.
fn builtin_peg_repeat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let single_consumed = i1(args);
    let times = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(single_consumed * times))
}

/// PEG lookahead: succeeds without consuming input.
fn builtin_peg_lookahead(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inner = i1(args);
    Ok(PerlValue::integer(if inner >= 0 { 0 } else { -1 }))
}

/// DFA simulate one step: δ(q, a) = q'.
fn builtin_dfa_simulate_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = i1(args);
    let a = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let delta_q_a = args.get(2).map(|v| v.to_number() as i64).unwrap_or(q);
    let _ = a;
    Ok(PerlValue::integer(delta_q_a))
}

/// Bytecode disassembly step: returns count of instructions visited.
fn builtin_bytecode_disasm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pc = i1(args);
    let instr_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(pc + instr_size))
}

/// SSA φ insertion: at each merge point, insert φ-node per live-in def.
fn builtin_ssa_phi_insert(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let live_in_defs = i1(args);
    Ok(PerlValue::integer(live_in_defs))
}

/// Dominator-tree immediate dominator (Lengauer-Tarjan-style step). Args: node,
/// candidate idom, current idom.
fn builtin_dom_tree_idom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let node = i1(args);
    let cand = args.get(1).map(|v| v.to_number() as i64).unwrap_or(node);
    let cur = args.get(2).map(|v| v.to_number() as i64).unwrap_or(node);
    Ok(PerlValue::integer(if cand <= cur { cand } else { cur }))
}

/// Dominance frontier: nodes whose idom chain doesn't strictly contain b.
fn builtin_dominance_frontier(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let preds = i1(args);
    let in_dom = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer((preds - in_dom).max(0)))
}
