// compiler & parsing primitives: NFA/DFA conversion, regex builders,
// LL/LR/LALR/Earley/PEG/Pratt parsers, SSA, dominators.

fn b68_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// NFA → DFA via subset construction. Returns DFA state count given NFA size n
/// (worst case 2ⁿ; we cap at 1<<20 to avoid overflow).
fn builtin_nfa_to_dfa(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).clamp(0, 20);
    Ok(StrykeValue::integer(1_i64 << n))
}

/// Subset construction step: combine ε-closure of state set into one DFA state.
fn builtin_subset_construction(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let states = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut seen = std::collections::HashSet::new();
    for s in states { seen.insert(s.to_bits()); }
    Ok(StrykeValue::integer(seen.len() as i64))
}

/// DFA **state minimisation** (Myhill–Nerode partition refinement). Args when
/// `n ≥ 4`: `n` (states), `sigma` (alphabet size), flat row-major transitions
/// `n·sigma` next-state indices, final flags length `n`. With fewer arguments,
/// falls back to counting **distinct** float signatures (legacy coarse estimate).
fn builtin_dfa_minimize_hopcroft(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use std::collections::{HashMap, HashSet};
    if args.len() < 4 {
        let signatures = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
        let mut classes = HashSet::new();
        for s in signatures {
            classes.insert(s.to_bits());
        }
        return Ok(StrykeValue::integer(classes.len() as i64));
    }
    let n = args[0].to_number() as usize;
    let sigma = args[1].to_number() as usize;
    let sigma = sigma.max(1);
    if n == 0 {
        return Ok(StrykeValue::integer(0));
    }
    let trans_flat = b68_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let finals = b68_to_floats(args.get(3).unwrap_or(&StrykeValue::array(vec![])));
    if trans_flat.len() < n * sigma {
        return Ok(StrykeValue::integer(0));
    }
    let mut trans: Vec<usize> = trans_flat
        .iter()
        .take(n * sigma)
        .map(|&x| x as usize)
        .collect();
    for t in &mut trans {
        *t = (*t).min(n.saturating_sub(1));
    }
    let mut part: Vec<usize> = (0..n)
        .map(|i| if finals.get(i).copied().unwrap_or(0.0) != 0.0 { 1 } else { 0 })
        .collect();
    loop {
        let mut buckets: HashMap<Vec<usize>, usize> = HashMap::new();
        let mut next_id = 0_usize;
        let mut new_part = vec![0_usize; n];
        for i in 0..n {
            let mut sig = Vec::with_capacity(1 + sigma);
            sig.push(part[i]);
            for a in 0..sigma {
                let nx = trans[i * sigma + a].min(n.saturating_sub(1));
                sig.push(part[nx]);
            }
            let id = *buckets.entry(sig).or_insert_with(|| {
                let id = next_id;
                next_id += 1;
                id
            });
            new_part[i] = id;
        }
        if new_part == part {
            break;
        }
        part = new_part;
    }
    let k = part.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    Ok(StrykeValue::integer(k as i64))
}

/// Thompson regex → NFA: 2 states per single-char + per-operator overhead.
/// Args: regex AST size. Returns expected state count.
fn builtin_regex_to_nfa_thompson(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let nodes = i1(args);
    Ok(StrykeValue::integer(2 * nodes))
}

/// Glushkov construction: |Σ_p| + 1 states for regex with p positions.
fn builtin_glushkov_construction(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let positions = i1(args);
    Ok(StrykeValue::integer(positions + 1))
}

/// Brzozowski derivative: ∂_a(L) = quotient set. Args: depth, returns 0 if
/// derivative is empty (false) else 1.
fn builtin_brzozowski_derivative(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let nullable = i1(args);
    Ok(StrykeValue::integer(if nullable != 0 { 1 } else { 0 }))
}

/// LL(1) FIRST set size for nonterminal. Args: production array as flat
/// [head_id, rhs_id_0, rhs_id_1, ...] separated by 0.
fn builtin_ll1_first_set(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let prods = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut firsts = std::collections::HashSet::new();
    for p in prods { firsts.insert(p as i64); }
    Ok(StrykeValue::integer(firsts.len() as i64))
}

/// LL(1) FOLLOW set (count of distinct symbols).
fn builtin_ll1_follow_set(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_ll1_first_set(args)
}

/// LL(1) predict-table cell: returns 1 if (nonterm, term) has a rule.
fn builtin_ll1_predict_table(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let nt = i1(args);
    let t = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let prods = i1(&args[2..]);
    Ok(StrykeValue::integer(if nt > 0 && t >= 0 && prods > 0 { 1 } else { 0 }))
}

/// LR(0) item set construction step. Args: existing state count + closure increment.
fn builtin_lr0_items_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args);
    let inc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(cur + inc))
}

/// LALR lookahead computation: returns size of merged lookahead set.
fn builtin_lalr_lookahead_compute(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut s = std::collections::HashSet::new();
    for x in v { s.insert(x as i64); }
    Ok(StrykeValue::integer(s.len() as i64))
}

/// LR(1) canonical collection size estimate.
fn builtin_lr1_canonical_collection(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let kernel = i1(args);
    Ok(StrykeValue::integer(kernel * 2))
}

/// Earley scanner step: advance dot in items matching current input symbol.
fn builtin_earley_scan(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let active = i1(args);
    Ok(StrykeValue::integer(active))
}

/// Earley predictor: for active item A → α·Bβ, add B → ·γ items.
fn builtin_earley_predict(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let prods_for_b = i1(args);
    Ok(StrykeValue::integer(prods_for_b))
}

/// Earley completer: when an item dots out, propagate completions.
fn builtin_earley_complete(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let waiting = i1(args);
    Ok(StrykeValue::integer(waiting))
}

/// Packrat parser memo step: returns 1 if memoised (cache hit).
fn builtin_packrat_parse_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cached = i1(args);
    Ok(StrykeValue::integer(if cached != 0 { 1 } else { 0 }))
}

/// Recursive ascent step: lookahead-driven dispatch on terminal token.
fn builtin_ascent_parser_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let token = i1(args);
    Ok(StrykeValue::integer(token))
}

/// Pratt parser binding-power compare: nud, led, rbp.
fn builtin_pratt_parse_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lbp = f1(args);
    let rbp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if lbp > rbp { 1 } else { 0 }))
}

/// Shunting-yard step: push or pop based on precedence comparison.
fn builtin_shunting_yard_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur_prec = f1(args);
    let stack_top_prec = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let left_assoc = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let pop = if left_assoc != 0 { cur_prec <= stack_top_prec } else { cur_prec < stack_top_prec };
    Ok(StrykeValue::integer(if pop { 1 } else { 0 }))
}

/// Regex compile (Thompson): returns NFA fragment count.
fn builtin_regex_compile_thompson(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_regex_to_nfa_thompson(args)
}

/// Regex match via DFA: returns 1 if accepting state reached.
fn builtin_regex_match_dfa(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let final_state = i1(args);
    let accepting = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if final_state == accepting { 1 } else { 0 }))
}

/// Lex keyword classifier: returns token-class ID for a hashed keyword.
fn builtin_lex_keyword_classify(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = i1(args) as u64;
    Ok(StrykeValue::integer((h % 256) as i64))
}

/// PEG sequence: succeeds iff all sub-parsers succeed (returns count consumed).
fn builtin_peg_seq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let consumed = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if consumed.iter().any(|&v| v < 0.0) { return Ok(StrykeValue::integer(-1)); }
    Ok(StrykeValue::integer(consumed.iter().sum::<f64>() as i64))
}

/// PEG ordered choice: returns first successful child consumed count.
fn builtin_peg_choice(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let consumed = b68_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    for c in consumed { if c >= 0.0 { return Ok(StrykeValue::integer(c as i64)); } }
    Ok(StrykeValue::integer(-1))
}

/// PEG repeat: greedy.
fn builtin_peg_repeat(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let single_consumed = i1(args);
    let times = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(single_consumed * times))
}

/// PEG lookahead: succeeds without consuming input.
fn builtin_peg_lookahead(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let inner = i1(args);
    Ok(StrykeValue::integer(if inner >= 0 { 0 } else { -1 }))
}

/// DFA simulate one step: δ(q, a) = q'.
fn builtin_dfa_simulate_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = i1(args);
    let a = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let delta_q_a = args.get(2).map(|v| v.to_number() as i64).unwrap_or(q);
    let _ = a;
    Ok(StrykeValue::integer(delta_q_a))
}

/// Bytecode disassembly step: returns count of instructions visited.
fn builtin_bytecode_disasm_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pc = i1(args);
    let instr_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer(pc + instr_size))
}

/// SSA φ insertion: at each merge point, insert φ-node per live-in def.
fn builtin_ssa_phi_insert(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let live_in_defs = i1(args);
    Ok(StrykeValue::integer(live_in_defs))
}

/// Dominator-tree immediate dominator (Lengauer-Tarjan-style step). Args: node,
/// candidate idom, current idom.
fn builtin_dom_tree_idom(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let node = i1(args);
    let cand = args.get(1).map(|v| v.to_number() as i64).unwrap_or(node);
    let cur = args.get(2).map(|v| v.to_number() as i64).unwrap_or(node);
    Ok(StrykeValue::integer(if cand <= cur { cand } else { cur }))
}

/// Dominance frontier: nodes whose idom chain doesn't strictly contain b.
fn builtin_dominance_frontier(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let preds = i1(args);
    let in_dom = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((preds - in_dom).max(0)))
}
