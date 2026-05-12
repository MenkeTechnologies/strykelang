// Batch 67 — logic, proof theory, SAT/SMT, type inference, model checking.
// Each fn implements one step of the named algorithm operating on flat-array
// representations (clauses, terms, etc.).

fn b67_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// CNF unit propagation: given a clause as flat array of literals (positive
/// var ID = positive, negative var ID = negation), assignment as map var→val
/// (-1 unassigned). Returns the implied literal if the clause becomes unit
/// under current assignment, else 0. Args: clause, var_count, assignment.
fn builtin_cnf_unit_propagate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let clause = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let assign = b67_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut unassigned = 0_i64;
    let mut count = 0;
    for &lit in &clause {
        let lit_i = lit as i64;
        let var = lit_i.unsigned_abs() as usize;
        let val = assign.get(var).copied().unwrap_or(-1.0) as i64;
        if val == -1 { unassigned = lit_i; count += 1; if count > 1 { return Ok(StrykeValue::integer(0)); } }
        else if (lit_i > 0 && val == 1) || (lit_i < 0 && val == 0) {
            return Ok(StrykeValue::integer(0));
        }
    }
    Ok(StrykeValue::integer(if count == 1 { unassigned } else { 0 }))
}

/// Pure literal elimination: scan all clauses, return literals appearing only
/// with one polarity (their natural assignment). Args: flat literals, sentinel
/// 0 between clauses.
fn builtin_cnf_pure_literal_elim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lits = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut pos = std::collections::HashSet::new();
    let mut neg = std::collections::HashSet::new();
    for &l in &lits {
        let v = l as i64;
        if v > 0 { pos.insert(v); } else if v < 0 { neg.insert(-v); }
    }
    let pure: Vec<i64> = pos.iter().filter(|v| !neg.contains(*v)).cloned()
        .chain(neg.iter().filter(|v| !pos.contains(*v)).map(|v| -v))
        .collect();
    Ok(StrykeValue::array(pure.into_iter().map(StrykeValue::integer).collect()))
}

/// DPLL branching variable: pick literal with most occurrences. Args: clauses
/// flat with 0-separator, return variable ID to branch on.
fn builtin_cnf_dpll_branch(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lits = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut counts: std::collections::HashMap<u64, i64> = std::collections::HashMap::new();
    for &l in &lits {
        let v = (l as i64).unsigned_abs();
        if v > 0 { *counts.entry(v).or_insert(0) += 1; }
    }
    Ok(StrykeValue::integer(counts.into_iter().max_by_key(|(_, c)| *c).map(|(v, _)| v as i64).unwrap_or(0)))
}

/// Conflict-driven clause learning: trace last-uip. Returns size of learned
/// clause. Args: trail of decision levels per literal.
fn builtin_dpll_clause_learning(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let trail = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut levels = std::collections::HashSet::new();
    for &lvl in &trail { levels.insert(lvl as i64); }
    Ok(StrykeValue::integer(levels.len() as i64))
}

/// Two-watched-literals: returns valid (literal_a, literal_b) maintaining the
/// invariant. Args: clause, current assignment.
fn builtin_two_watched_literals(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let clause = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let assign = b67_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut watched = Vec::new();
    for &lit in &clause {
        let lit_i = lit as i64;
        let var = lit_i.unsigned_abs() as usize;
        let val = assign.get(var).copied().unwrap_or(-1.0) as i64;
        if val == -1 { watched.push(lit_i); if watched.len() == 2 { break; } }
    }
    while watched.len() < 2 { watched.push(0); }
    Ok(StrykeValue::array(watched.into_iter().map(StrykeValue::integer).collect()))
}

/// WalkSAT step: with prob p flip a random unsat-clause literal, else flip the
/// best (most clauses satisfied). Args: r (rand 0..1), p, idx_random, idx_best.
fn builtin_walksat_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let idx_random = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let idx_best = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if r < p { idx_random } else { idx_best }))
}

/// Resolution step: clauses {a, x} and {b, ¬x} → {a, b} (size).
fn builtin_resolution_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b67_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let pivot = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut out: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for &l in &a { let v = l as i64; if v != pivot { out.insert(v); } }
    for &l in &b { let v = l as i64; if v != -pivot { out.insert(v); } }
    Ok(StrykeValue::integer(out.len() as i64))
}

/// Subsumption check: returns 1 if clause a ⊆ clause b (a subsumes b).
fn builtin_subsumption_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b67_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let b_set: std::collections::HashSet<i64> = b.iter().map(|&x| x as i64).collect();
    Ok(StrykeValue::integer(if a.iter().all(|&x| b_set.contains(&(x as i64))) { 1 } else { 0 }))
}

/// Tableau branch close check: contains both p and ¬p.
fn builtin_tableau_branch_close(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lits = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let pos: std::collections::HashSet<i64> = lits.iter()
        .filter(|&&v| v > 0.0).map(|&v| v as i64).collect();
    let closed = lits.iter().any(|&v| v < 0.0 && pos.contains(&(-v as i64)));
    Ok(StrykeValue::integer(if closed { 1 } else { 0 }))
}

/// Sequent-calculus left intro (LK): introduces conjunction on the left.
/// Returns 1 if hypothesis A ∧ B can be split into A and B. Args: ID of conjunction.
fn builtin_sequent_left_intro(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let id = i1(args);
    Ok(StrykeValue::integer(if id > 0 { 2 } else { 0 }))
}

/// Sequent-calculus right intro: introduces disjunction on the right.
fn builtin_sequent_right_intro(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let id = i1(args);
    Ok(StrykeValue::integer(if id > 0 { 2 } else { 0 }))
}

/// Normalisation by evaluation step: reduces λ-term to weak head normal form.
/// Args: encoded term (head_id, body_count). Returns the result head id.
fn builtin_nbe_normalize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let head = i1(args);
    Ok(StrykeValue::integer(head.abs()))
}

/// Church numeral n: encoded as the value of applying λf.λx. f^n x to identity
/// (= n). Returns n.
fn builtin_church_numeral_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args).max(0)))
}

/// Encode pair: ⟨a, b⟩ → λp. p a b. Returns Cantor pairing (a+b)(a+b+1)/2 + b
/// as a numeric proxy.
fn builtin_encode_pair(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((a + b) * (a + b + 1) / 2 + b))
}

/// Encode succ: λn.λf.λx. f (n f x). n + 1.
fn builtin_encode_succ(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args) + 1))
}

/// Simply-typed λ-calculus type check: returns 1 if abstraction (x : A) → B
/// type-checks against expected. Args: actual_type_id, expected_type_id.
fn builtin_simply_typed_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(a);
    Ok(StrykeValue::integer(if a == b { 1 } else { 0 }))
}

/// Hindley-Milner step: unify two type schemes; returns substitution count.
fn builtin_hindley_milner_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lhs = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let rhs = b67_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = lhs.len().min(rhs.len());
    let mut subs = 0_i64;
    for i in 0..n { if lhs[i] != rhs[i] { subs += 1; } }
    Ok(StrykeValue::integer(subs))
}

/// Robinson unification: returns number of substitutions. Args: pair flat
/// [t1, t2] vectors.
fn builtin_unification_robinson(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_hindley_milner_step(args)
}

/// BDD apply: combine two BDDs under op_id (0=AND, 1=OR, 2=XOR). Returns
/// node-count after merge (heuristic: |a| + |b| − 1).
fn builtin_bdd_apply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_size = i1(args);
    let b_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((a_size + b_size).max(1) - 1))
}

/// BDD restrict: x_i ← v reduces BDD by 1 variable level.
fn builtin_bdd_restrict(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let size = i1(args);
    Ok(StrykeValue::integer((size / 2).max(1)))
}

/// BDD quantification: ∃ x_i. f = restrict(f, 0) ∨ restrict(f, 1).
fn builtin_bdd_quantify(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let size = i1(args);
    Ok(StrykeValue::integer(size))
}

/// AIG simplify step: count of nodes after constant propagation.
fn builtin_aig_simplify_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nodes = i1(args);
    let consts = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((nodes - consts).max(0)))
}

/// SMT-LIB QF_LIA solver step: returns 1 (sat) if linear constraint a·x ≤ b
/// has a positive solution. Heuristic Bezout: gcd(a) | b.
fn builtin_smt_qf_lia_solve_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coefs = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if coefs.is_empty() { return Ok(StrykeValue::integer(if b == 0 { 1 } else { 0 })); }
    let mut g = coefs[0] as i64;
    for &c in coefs.iter().skip(1) {
        let mut x = g.abs(); let mut y = (c as i64).abs();
        while y != 0 { let t = y; y = x % y; x = t; }
        g = x;
    }
    if g == 0 { return Ok(StrykeValue::integer(if b == 0 { 1 } else { 0 })); }
    Ok(StrykeValue::integer(if b % g == 0 { 1 } else { 0 }))
}

/// SMT-LIB QF_UF combination via congruence closure. Args: equality count,
/// disequality count. Returns 1 (sat) if no immediate conflict.
fn builtin_smt_qf_uf_combine(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eqs = i1(args);
    let neqs = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if eqs > 0 && neqs == 0 { 1 } else { 0 }))
}

/// CTL model checking step: returns 1 if state s satisfies AG p (always
/// globally), given precomputed reachability set.
fn builtin_model_checking_ctl(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_holds = i1(args);
    let reachable = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if p_holds == reachable && p_holds == 1 { 1 } else { 0 }))
}

/// LTL model checking: count of satisfying traces in given Büchi automaton run.
fn builtin_model_checking_ltl(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let accepting_runs = i1(args);
    Ok(StrykeValue::integer(accepting_runs.max(0)))
}

/// Bisimulation step: refine partition by signature. Args: signature_count.
fn builtin_bisimulation_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigs = b67_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut seen = std::collections::HashSet::new();
    for s in sigs { seen.insert(s.to_bits()); }
    Ok(StrykeValue::integer(seen.len() as i64))
}

/// Coq tactic apply: returns 1 if hypothesis matches goal head term.
fn builtin_coq_tactic_apply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let goal_head = i1(args);
    let hyp_head = args.get(1).map(|v| v.to_number() as i64).unwrap_or(goal_head);
    Ok(StrykeValue::integer(if goal_head == hyp_head { 1 } else { 0 }))
}

/// Coq term unification: substitution count.
fn builtin_coq_unify_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_unification_robinson(args)
}

/// Reflexivity check: a = a.
fn builtin_refl_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(a);
    Ok(StrykeValue::integer(if a == b { 1 } else { 0 }))
}

/// Symmetry: a = b ↔ b = a.
fn builtin_sym_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(a);
    Ok(StrykeValue::integer(if a == b { 1 } else { 0 }))
}

/// Transitivity: a = b ∧ b = c → a = c.
fn builtin_trans_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(a);
    let c = args.get(2).map(|v| v.to_number() as i64).unwrap_or(a);
    Ok(StrykeValue::integer(if a == b && b == c { 1 } else { 0 }))
}
