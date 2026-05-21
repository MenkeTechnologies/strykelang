# BUGS.md — Known parity gaps and surprising behaviors

Captured 2026-05-04 from a behavior-pinning sweep against `stryke v0.11.12` on
macOS aarch64; continuously updated since. Additional behavior pins live in
`tests/suite/behavior_pin_2026_05*.rs` (rolling `_a..z`, `_aa..` batches).
Entries below pair each documented bug with the pinning tests that lock the
*current* output.

When a bug is fixed, remove the entry from this file. The pinning test
in `tests/suite/behavior_pin_2026_05*.rs` stays as the regression guard.
Numeric IDs are not reused, so historical references in commits still
resolve to git history.

Severity legend:

- `parity` — diverges from Perl 5; intentional or accidental TBD
- `bug` — observably wrong vs documented intent
- `polish` — non-critical UX/error-message issue

## BUG-120 — `cosine_distance` with a zero-length vector operand returns **1** — **`polish`**

When either argument has Euclidean norm ~0 (`cosine_similarity` is undefined),
`builtin_cosine_distance` clamps to **1** (maximum distance). That matches the
Rust guard `na < 1e-15 || nb < 1e-15` but differs from ecosystems that propagate
NaN instead of a finite sentinel.

Pin test: `cosine_distance_zero_operand_is_unit_bx` in
`tests/suite/behavior_pin_2026_05_bx.rs`.

## BUG-122 — `js_divergence` / `js_div` vs `jensen_shannon_div` disagree (nats vs bits) — **`bug`**

`js_divergence` (in `math_wolfram3.rs`) builds KL terms with **natural**
logarithms. `jensen_shannon_div` is wired to `kullback_jensen_div`
(`math_wolfram40.rs`), which uses **log2** in each KL term. The two therefore
differ by a factor of **`ln 2`** for the same distributions even though docs
refer to both as Jensen–Shannon-style quantities.

Illustrative non-uniform pair (pinned numerically):

- `sprintf("%.12f", jensen_shannon_div(...)) → "0.031596722287"`
- `sprintf("%.12f", js_div(...))          → "0.021901178968"`

Pin tests: `jensen_shannon_div_triple_bx`, `js_divergence_triple_nats_bx` in
`tests/suite/behavior_pin_2026_05_bx.rs`.

## BUG-123 — `chi_squared_distance` vs `chisquare_metric` differ by a factor of **2** — **`bug`**

Both walk the elementwise \(\sum_i (p_i-q_i)^2/(p_i+q_i)\)
pattern, but `chi_squared_distance` (`math_wolfram4.rs`) multiplies by
**`0.5`** while `chisquare_metric` (`math_wolfram40.rs`) omits it. Names
give no indication which convention applies.

Pins: `chisquare_metric_axis_pair_by`, `chisquare_metric_equals_twice_chi_squared_distance_by`
in `tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-124 — `csiszar_phi_div` is \(\sum_i q_i \ln(p_i/q_i) = -\mathrm{KL}(Q\|P)\), not an unsigned ϕ-form — **`bug`**

Rust comment claims “Csiszár ϕ-divergence: \(\sum q \, \phi(p/q)\)” with the
usual convex \(\phi\) so the sum is \(\mathrm{KL}(P\|Q)\) nonnegative. The
implementation instead accumulates **`q_i * ln(p_i/q_i)`**, which yields
**\(-\mathrm{KL}(Q\|P)\)** and can surface **negative floats** whenever
\(Q\neq P\).

Pin test: `csiszar_phi_div_coin_pair_by` in
`tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-125 — `relative_entropy_kl` measures KL in **bits**; `kl_divergence` / `kl_div` use **nats** — **`bug`**

`builtin_relative_entropy_kl` (`math_wolfram40.rs`) uses `(p/q).log2()`.
The older `builtin_kl_divergence` path (`math_wolfram3.rs`) uses `.ln()`
throughout. Multiply the former by \(\ln 2\) to reproduce the latter for the
same \(P,Q\).

Pin tests: `relative_entropy_kl_uses_bits_by`,
`relative_entropy_kl_times_ln2_matches_kl_div_by` in
`tests/suite/behavior_pin_2026_05_by.rs`.

## BUG-126 — Entropy/share builtins read only **`args.first()`**, dropping comma-arg tails — **`bug`**

Many helpers flatten **one** positional argument (`arg_to_vec(&args[0])` or read
via `args.first()` as a lone arrayref/scalar). Supplying probabilities or values
**as Perl variads** (`f(p1, p2, p3)`, no square brackets) therefore keeps only the
leading scalar and ignores the comma-separated tails. Pass a single **array ref**
explicitly (`f([ p1, p2, … ])`) to aggregate the intended list today.

Demonstrated builtins (non-exhaustive):

| Builtin | Pins |
|---------|------|
| `joint_entropy_step` | `joint_entropy_four_uniform_coin_bits_array_bz`, `joint_entropy_variadic_trailing_probs_ignored_tail_bz` |
| `herfindahl_hirschman`, `hhi` | `herfindahl_hirschman_normalized_quarter_shares_array_bz`, `hhi_variadic_trailing_shares_use_first_squared_only_tail_bz` |
| `gini_impurity` | `gini_impurity_three_class_normalized_array_bz`, `gini_impurity_variadic_first_probability_only_tail_bz` |
| `entropy_bits` | `entropy_bits_four_coin_array_equals_two_tail_bz`, `entropy_bits_variadic_degenerate_after_truncation_tail_bz` |
| `log_sum_exp`, `lse` | `log_sum_exp_array_maximum_dominated_stable_bz`, `log_sum_exp_variadic_first_scalar_only_tail_bz` |
| `lorenz_curve_points` | `lorenz_curve_points_sorted_three_in_array_ca`, `lorenz_curve_points_variadic_truncated_tail_ca` |
| `grade_up` | `grade_up_permutation_three_ca`, `grade_up_variadic_first_element_only_ca` |
| `grade_down` | `grade_down_permutation_three_ca`, `grade_down_variadic_first_scalar_only_ca` |
| `npv` | `npv_array_discounts_four_uniform_periods_ce`, `npv_variadic_second_bucket_only_counts_lead_outflow_ce` |
| `irr` | `irr_array_newton_positive_rate_ce`, `irr_variadic_first_flow_only_interprets_second_as_guess_ce`, `irr_satisfies_npv_near_zero_residual_ce` |
| `payback_period` | `payback_requires_array_bucket_second_arg_ce` (variadic commas miss the **`args[1]`** array bucket → **`undef`**) |
| `discounted_payback` | `discounted_payback_requires_array_middle_bucket_ce` (same **`args[1]`** coupling) |
| `resistance_parallel` | `resistance_parallel_three_resistors_array_cf`, `resistance_parallel_variadic_ignores_trailing_cf` |
| `resistance_series` | `resistance_series_array_sum_cf`, `resistance_series_variadic_first_only_cf` |
| `capacitance_parallel` / `capacitance_series` | **`capacitance_parallel_series_array_buckets_cf`** (`arg_to_vec` on **`args.first()`** only) |
| `inductance_parallel` / `inductance_series` | **`inductance_parallel_formula_matches_reciprocal_cf`**, **`inductance_series_linear_sum_cf`** |
| `charcodes_to_string` | **`charcodes_to_string_array_round_trip_hi_cg`**, **`charcodes_to_string_variadic_second_codepoint_dropped_tail_cg`** |
| `squared` / `sq` | **`squared_three_ch`**, **`squared_variadic_second_operand_ignored_ch`**, **`sq_alias_matches_squared_ch`** |
| `cubed` / `cb` | **`cubed_two_ch`**, **`cubed_variadic_second_operand_ignored_ch`**, **`cb_alias_matches_cubed_ch`** |
| `uniq` | **`uniq_variadic_deduplicates_neighbors_ch`**, **`uniq_single_array_bucket_treated_as_atom_ch`** |
| `mutual_information`, `mi` | **`mutual_information_flat_list_joint_de`**, **`mutual_information_two_by_two_matrix_de`**, **`mutual_information_second_operand_silent_de`** (**`args[1]`** discarded — joint only from **`args[0]`**) |


Pins documenting **tail truncation** split across **`tests/suite/behavior_pin_2026_05_bz.rs`**,
**`behavior_pin_2026_05_ca.rs`** (Lorenz + `grade_*`), **`behavior_pin_2026_05_ce.rs`** (NPV/IRR + paybacks), and **`behavior_pin_2026_05_cf.rs`**
(passive **R/L/C** ladders). Companion geo/string pins live in **`behavior_pin_2026_05_cg.rs`**
(geohashes, projections, kernels, AES/Simon graph helpers).

**`behavior_pin_2026_05_ca.rs`** also pins assorted ML helpers (`confusion_counts`, `mcc`,
`hinge_loss`, …) strictly for reproducible floats — **not** tail-drop cases.

List / stats companion pins: **`tests/suite/behavior_pin_2026_05_ch.rs`** (also **`chain_from`**
**`ARRAYREF`** pitfall — **BUG-142**), **`behavior_pin_2026_05_ci.rs`** (streaming / `to_list` traps — **BUG-143** … **BUG-146**),
and **`behavior_pin_2026_05_cj.rs`** (list glue + **`permutations([...])`** — **BUG-147**, **`concat`** — **BUG-148**),
**`behavior_pin_2026_05_ck.rs`** (**`without([...], LIST)`** — **BUG-149**),
**`behavior_pin_2026_05_cl.rs`** (**BUG-151** … **BUG-155** — clamp / strings / `hamming` / `substr` / **`reverse([...])`**),
**`behavior_pin_2026_05_cm.rs`** (**`seq`** multi-arg — **BUG-156**),
**`behavior_pin_2026_05_cn.rs`** (**`parse_int("0xff")`** — **BUG-158**; **`transpose`** nested AoA — **BUG-159**; regex helper arg order — **BUG-160**),
**`behavior_pin_2026_05_co.rs`** (**`percentile`** / **`quantile`** conventions — **BUG-161**; **`take`** / **`product`** **`ARRAYREF`** buckets cross-ref **BUG-143** / **BUG-140**),
**`behavior_pin_2026_05_cp.rs`** (scalar planar **`chebyshev` / `slope` / `midpoint`** vs vector distances — **BUG-162**),
**`behavior_pin_2026_05_cq.rs`** (**`running_reduce`** + **`$a`/`$b`** — **BUG-163**; **`uri_resolve` / `uri_normalize`** byte vectors — **BUG-164**),
**`behavior_pin_2026_05_cr.rs`** (**`string_take_while` / `string_drop_while`** charset-prefix semantics — **BUG-165**; **`nth`** on **`ARRAYREF`** — **BUG-166**),
**`behavior_pin_2026_05_cs.rs`** (**`hamming`** vs **`hamming_distance`** — **BUG-168**; **`matrix_transpose`** — cross-ref **BUG-159** / variadic transpose),
**`behavior_pin_2026_05_ct.rs`** (**`hhi` / `herfindahl_hirschman`** share vector — **BUG-169**),
**`behavior_pin_2026_05_cu.rs`** (**`moving_average` / `batch` / `chunk_n`** arity — **BUG-170**),
**`behavior_pin_2026_05_cv.rs`** (**`ml_binary_cross_entropy`** open interval — **BUG-171**),
**`behavior_pin_2026_05_cw.rs`** (**`jaccard_similarity`** string-set collapse on vector args — **BUG-172**; **`mode([…])`** bracket
operand — **BUG-173**),
**`behavior_pin_2026_05_cx.rs`** (**`windowed` / `chunked`** — **BUG-174**; **`trimmed_mean`** — **BUG-175**; **`base_convert`**
two-arg numeric — **BUG-176**),
**`behavior_pin_2026_05_cy.rs`** (**`graph_density`** — **BUG-177**; **`transpose`** vs **`matrix_transpose`** — **BUG-178**),
**`behavior_pin_2026_05_cz.rs`** (**`pmt`** arg order — **BUG-179**; **`format_percent`** — **BUG-180**),
**`behavior_pin_2026_05_da.rs`** (**`anova_oneway`** nested AoA — **BUG-181**; **`trapz` / `simpson`** second operand — **BUG-182**),
**`behavior_pin_2026_05_db.rs`**: **BUG-183** (search/bounds needle-first), **BUG-184** (`dice_coefficient` strings), **BUG-185** (`winsorize` percent-first),
**`behavior_pin_2026_05_dc.rs`**: **BUG-186** (`unzip` vs row pairs), **BUG-187** (`clamp_list` inverted bounds panic),
**`behavior_pin_2026_05_dd.rs`**: **BUG-188** (`datetime_strftime` arg order), **BUG-189** (`mahalanobis` malformed rows panic), **`product([…])`** tail of **BUG-140**.
**`behavior_pin_2026_05_de.rs`**: **BUG-126** (`mutual_information` / `mi` ignores the second operand), **BUG-190** (`rbinom` two-arg **`prob` → `size`**), **BUG-191** (`numerical_gradient` **`my ($x)=@_`** vs coordinate **`ARRAY`**) — pins also cover **BB / hypergeom**, **windows**, **info divergences**, **graph** summaries, **moments**, **`hungarian_assignment`**.
**`behavior_pin_2026_05_df.rs`**: **BUG-192** (`lerp` is **`(A, B, T)`**, not shader-style **`(T, A, B)`**) plus pins for **gamma / polygamma**, **Jacobians & Hessians**, **Weibull / lognormal / survival**, **scores & distances**, **clustering indices**, **χ² & F**, **GL quadrature**, **`rk4` / `euler_ode`**, **`brent_root`**.
**`behavior_pin_2026_05_dg.rs`**: **BUG-193** (Black–Scholes IDE **`S,K,r,T,σ`** vs runtime **`S,K,T,r,σ`**), **BUG-194** (`hamming_distance` **`ARRAY`** operands), **BUG-195** (`romberg_quad` combine step) — also **Greeks**, **special functions**, **geometry**, **EM / k-means**, **number theory**, **string metrics**.
**`behavior_pin_2026_05_dh.rs`**: **BUG-196** (`crt` / `chinese_remainder` needs **two list buckets**), **BUG-197** (`simplex_volume_3d([[…]])` vs **`tetrahedron_volume`**), **BUG-198** (`derangements` ≠ subfactorial) — plus **ζ-series**, **splines / EWMA**, **special functions**, **bond analytics**, **graph & tests**, **CF / hash / NT**.
**`behavior_pin_2026_05_di.rs`**: **BUG-199** (`graph_is_tree` / **`parse_adj_list`** — adjacency **lists** vs 0/1 **matrix** rows), **BUG-200** (`snowball_stem_english` needs **codepoint lists**) — plus **3D geo**, **paths & flows**, **ML loss slices**, **codec**, **tensor bits**.
**`behavior_pin_2026_05_dj.rs`**: **BUG-202** (**`prim_mst`** disconnected / **zero-weight** edges) — plus **interpolation**, **orthogonal polynomials**, **Gray / Conway**, **activations**, **range maps**.
**`behavior_pin_2026_05_dk.rs`**: **PDF** suite pins, **graph / search micro-ops**, **jump hash**, **SDF / noise**, **Chebyshev / Hermite**, **Mandelbrot / Hanoi**.
**`behavior_pin_2026_05_dl.rs`**: **BUG-204** (**`db_simhash_bit`** name vs **sign** semantics) — **Wolfram48 DB/sketch/cost** pins, **quantiles**, **multiset / multinomial**, **elliptic / polylog / Zernike / spherical harmonic**.

## BUG-127 — `iota_range` ignores arguments after the first — **`polish`**

`builtin_iota_range` consumes only \(N\) from `args[0]`. Passing `iota_range(5,
99)` (or longer comma tails) parses as Perl variadic call sites normally do but
everything after **`5`** is discarded with no arity error, so callers can
mistakenly believe they threaded multiple ranges.

Pins: `iota_range_zero_until_n_exclusive_cb`,
`iota_range_trailing_numeric_args_ignored_matches_five_only_cb` in
`tests/suite/behavior_pin_2026_05_cb.rs`.

## BUG-130 — `detrend_linear` returns **slope**, not **detrended samples** — **`polish`**

Despite the noun-like name mirroring MATLAB's `detrend`, the builtin returns **`num/den`** from
the single least-squares line fit — a scalar slope estimate only. Users expecting residual series
subtract the fit manually today.

Pin: `detrend_linear_pure_ramp_slope_one_cd` in `tests/suite/behavior_pin_2026_05_cd.rs`.

## BUG-132 — **`bs_*` greeks** (`bs_delta`, **`bs_theta`**, **`bs_rho`**) are **call** formulas — **`polish`**

`builtin_bs_delta` returns **`N(d1)`** only — textbook **put \(\Delta\)** is **`N(d1) - 1`** (pins show the
**\(-1\)** parity gap next to **`bs_delta`**). **`bs_theta`** and **`bs_rho`** inline the derivatives of the **call**
price (**`-r · K · e^{-rT} · N(d2)`** curvature terms), **not** the put equivalents (which flip signs on pieces
stemming from \(\partial N(-d\*)/\partial T\) / \(\rho\)).

Pins documenting current call-only Greeks: **`bs_delta_returns_call_delta_cdf_d1_ce`**,
**`bs_put_delta_equals_call_delta_minus_one_ce`**, **`bs_theta_call_style_negative_ce`**, **`bs_rho_call_style_positive_ce`**
in **`tests/suite/behavior_pin_2026_05_ce.rs`**.

## BUG-136 — **`geohash_neighbor`** nudges \(\Delta\)lat/\(\Delta\)lon with **tiny isotropic **`2^{-(5·len/2)}`** (\(i32\)**) **step** → **effective no-op at common precisions** — **`bug`**

`builtin_geohash_neighbor` decodes **`s`**, then shifts **lat** / **lon** by **one magnitude** (**`step = 1 /
2^{(\texttt{len} \cdot 5 / 2)}`** in Rust integer division) every direction. Typical **~6-character** hashes use a **sub-cell**
**\(\Delta\)** versus the **child-bit** quantization of **`geohash_encode`** — perturbations Round-trip inside the **same**
base-32 string (**`geohash_neighbor_cardinals_are_identity_at_precision_six_cg`**). Applying the **same \(\Delta\)**
to **latitude** and **longitude** also ignores customary **North–South** vs **East–West** bin anisotropy. **`match dir.as_str()`**
fall-through assigns **\((0, 0)\)** for unknown direction tokens (**`geohash_neighbor_unknown_direction_leaves_hash_unchanged_cg`**)
instead of an error.

Pins: **`geohash_neighbor_cardinals_are_identity_at_precision_six_cg`**, **`geohash_neighbor_unknown_direction_leaves_hash_unchanged_cg`**
in **`tests/suite/behavior_pin_2026_05_cg.rs`**.

## BUG-137 — **`box_blur_kernel`** first argument is **half-width radius `r`**, output side **`2r+1`** — **`polish`**

`builtin_box_blur_kernel` computes **`n = 2·r + 1`** from `args.first()` as an integer **radius** (`math_wolfram14.rs`). Callers
supplying **`box_blur_kernel(7)`** expecting a **\(7\times7\)** stencil actually materialize a **\(15\times15\)** (**`2·7+1`**) kernel.
The entry value is **`1 / n²`** (uniform norm).

Pin: **`box_blur_kernel_radius_three_is_seven_squared_weights_cg`** in **`tests/suite/behavior_pin_2026_05_cg.rs`**.

## BUG-139 — **`normalize`** docs mention **`OUT_MIN, OUT_MAX, LIST`**; implementation always **`0..1`** — **`polish`**

Rustdoc on **`builtin_normalize`** sketches a **`normalize OUT_MIN, OUT_MAX, LIST`** form. The body
fixes **`out_min`** / **`out_max`** at **`0.0` / `1.0`** and flattens **all** positional arguments into
the sample multiset, so leading “range” operands become ordinary data rows.

Pin: **`normalize_extra_leading_scalars_folded_into_source_strip_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-141 — **`frequencies` / string operands** — one scalar ⇒ one hash key (**`polish`**)

Flattening treats a **`Str`** Perl value as a **single countable item**, so **`frequencies("aab")`**
returns **`{"aab" => 1}`** unless the string is first split into graphemes (**`chars(...)`** /
**`split("", ...)`**). Not a hashing bug once element cardinality is understood, but differs from
“count characters” intuition.

Pins: **`frequencies_whole_string_counts_as_one_key_ch`**, **`frequencies_chars_aab_two_keys_ch`**,
**`pfrequencies_matches_frequencies_large_multiset_parallel_path_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-142 — **`chain_from([[...],[...]])`** leaves inner **`ARRAYREF`** buckets as opaque atoms — **`bug`**

`builtin_chain_from` does `flatten_args` then **`item.to_list()`** per segment. **`StrykeValue::to_list`**
only expands **`HeapObject::Array`** (`Array` storages); a typical literal inner **`[..., ...]`**
is stored as **`ArrayRef`** (RW handle), whose **`to_list`** arm falls through **`_ ⇒
vec![self.clone()]`**. A single outer array argument **`([[1,2],[3]])`** therefore concatenates **four**
**list-valued slots** instead of draining their elements. Spreading the same buckets as Perl variadic
arguments (**`chain_from([1,2],[3],[4])`**) already worked.

Pins: **`chain_from_variadic_top_level_lists_concat_ch`**,
**`chain_from_single_outer_arrayref_leaves_inner_lists_unmerged_bug_ch`** in
**`tests/suite/behavior_pin_2026_05_ch.rs`**.

## BUG-143 — **`StrykeValue::to_list` + iterator plumbing** treat many **`ARRAYREF`** / “one arg” shapes as **atoms** — **`bug` / `polish`**

- **`HeapObject::ArrayRef`** (typical literal **`[ … ]`**) falls through **`StrykeValue::to_list`’s `_` arm** and becomes a **single opaque cell** instead of cloning the inner vector (unlike **`HeapObject::Array`**). Any helper that only calls **`to_list()`** (rather than **`map_flatten_outputs`**) mis-counts operands: pinned for **`head`** / **`tail`** / **`drop`** / **`take`** with **`head([1,2,3], 2)`**.
- Streaming builtins that special-case “one non-iterator argument” still route through **`into_pull_iter`**: that path also uses **`to_list`**, so **`ARRAYREF` sources** expose **one streamed item** (breaks **`chunk(2, [...])`** expectations). Variadic / iterator call shapes work today — e.g. **`chunk(2, range(1, 5))`**, **`dedup(1, 1, 2)`**.
- **`enumerate`**, **`dedup`**, **`chunk`**: when passed a **single** list argument, the implementation wraps **`StrykeValue::array(args.to_vec())`** for the pull source, so **`enumerate([a,b])`** yields **one** indexed row **`[0, list]`** (the whole list as the item) rather than per-element indices (contrast **`enumerate(range(1, 3))`**).
- **`PerlIterator::collect_all` on `CycleIterator` is intentionally `vec![]`** (infinite source guard), but **`flatten_args` / `map_flatten_outputs` call `collect_all`** for iterators — so compositions like **`take_n(6, cycle([1, 2, 3]))`** materialize **`()`** today.

Pins throughout **`tests/suite/behavior_pin_2026_05_ci.rs`** (file module doc enumerates the **`_ci`** suffix names).

## BUG-144 — **`transpose([[row1],[row2]])` does *not* transpose an AoA** — **`polish`**

`builtin_transpose` only ingests **top-level actuals** whose **`.as_array_ref()`** succeeds — one nested bracket form **`([[1,2],[3,4]])`** is parsed as **one row** whose columns are the **inner row refs**, not a 2×2 matrix. Use **`transpose`** with **multiple row operands** (**`transpose([1, 2], [3, 4])`**).

Pins: **`transpose_single_nested_outer_array_clusters_rows_bug_ci`**, **`transpose_two_row_arguments_column_major_ci`**.

## BUG-148 — **`concat` / `chain`** on **`ARRAYREF` operands** streams **one cell per argument** — **`polish`**

**`builtin_concat`** wraps each actual in **`into_pull_iter`**. A plain **`[...]`** value is an **`ARRAYREF`**
whose iterator surfaces **the whole list as one pulled item**, not element-by-element. Stringifying the
concat iterator therefore looks like **one bucket per argument** — e.g. **`([1, 2], [3], [4, 5])`** —
whereas **`chain_from([1, 2], [3], [4, 5])`** flattens top-level list slots today.

Pins: **`concat_iterator_one_bucket_per_arrayref_arg_cj`**, **`chain_from_three_lists_eager_flat_cj`**
in **`tests/suite/behavior_pin_2026_05_cj.rs`**.

## BUG-151 — **`clamp` three-scalar Perl order **`clamp($x,$min,$max)`** is mis-read as **`clamp($min,$max,@list)`** — **`polish`**

**`builtin_clamp`** treats **three** operands as **`min, max, first list value`** when the flattened
tail after the first two args is **non-empty** (even for a **single** trailing scalar). So
**`clamp(11, 0, 10)`** becomes **min=11**, **max=0**, values **`[10]`**, and **`10 < 11`** clamps to
**`11`** instead of **`10`**. For scalars, use **`clamp(0, 10, 11)`** (stryke **min,max,value** order)
or **`clamp_list`**.

Pins: **`clamp_scalar_inside_range_cl`**, **`clamp_value_min_max_order_misread_as_min_max_list_bug_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-152 — **`reverse($scalar)`** path-dependent string: **tail/assign** reverse; **`join("", …)`** does not — **`bug` / `polish`**

For a **string scalar** **`$s`**, **`reverse($s)`** as a **statement tail** or **`my $t = reverse($s); $t`** stringifies **`cba`**, but **`join("", reverse($s))`** stays **`abc`** today — list flattening / topic context treats the operand differently than assignment / return-value stringification.

Pins: **`reverse_scalar_tail_expr_stringifies_reversed_cl`**, **`reverse_scalar_after_let_binding_reversed_cl`**, **`reverse_scalar_join_list_context_stays_forward_bug_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**. (**`reverse_str`** remains the explicit grapheme reversal helper.)

## BUG-153 — bare **`hamming`** is the **DSP window**, not **string Hamming distance** — **`polish`**

Dispatch maps **`"hamming"`** to **`window_hamming`**. For **edit distance** on two strings, use
**`hamming_distance`** or **`hamming_distance_str`**.

Pins: **`hamming_distance_bit_flip_one_cl`** in **`tests/suite/behavior_pin_2026_05_cl.rs`**.


## BUG-155 — **`reverse([...])`** does not reverse **inner** elements (single **`ARRAYREF`** actual) — **`polish`**

Like **`uniq([…])`** / iterator bucket pitfalls, a **single** bracket array passed to **`reverse`**
is not **`map_flatten_outputs`**’d into a variadic list — **`stringify(reverse([1, 2, 3]))`** stays
**`[1, 2, 3]`**. Use **`reverse_list`**, **`reverse(1,2,3)`**, or **`reverse @{ $aref }`**-style
flattening when porting Perl.

Pins: **`reverse_variadic_three_ints_cl`**, **`reverse_single_inline_arrayref_identity_shape_cl`**, **`reverse_list_drains_bracket_list_cl`**
in **`tests/suite/behavior_pin_2026_05_cl.rs`**.

## BUG-156 — **`seq` is not Bash/Raku numeric `seq FIRST LAST` — only first arg is used** — **`polish`**

**`builtin_seq`** documents **`seq COLL`** — it turns one collection into a list (and **`UNDEF`**
when empty). **`seq(2, 5)`** therefore only inspects **`2`** (stringifies as **`"2"`**), not a range;
use **`range(2, 5)`** for inclusive integer steps.

Pin: **`seq_two_args_only_first_used_bug_cm`** in **`tests/suite/behavior_pin_2026_05_cm.rs`**.

## BUG-158 — **`parse_int("0xff")` without an explicit radix is not hex** — **`polish`**

**`parse_int`** only interprets a leading **`0x`** when the second-argument radix is **`16`**. A
literal **`parse_int("0xff")`** numifies **`0`** and stops (**`0`**, not **`255`**). Use
**`parse_int("ff", 16)`** (or **`hex` / `sprintf`**) for hex byte strings.

Pin: **`parse_int_zero_x_without_radix_is_zero_bug_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**.

## BUG-159 — **`transpose`** treats a **single** nested AoA as **one row** (use variadic rows or **`matrix_transpose`**) — **`polish`**

**`transpose`** is documented as variadic rows: **`transpose(@row_a, @row_b, …)`**. Passing **one**
value that is itself an AoA (**`transpose([[1,2],[3,4]])`**) flattens only the **outer** wrapper: the
implementation iterates **`args`**, not **`args[0].rows`**, so you get a **1×2** “row of rowrefs” and a
column-major shuffle — not a **2×2** transpose. **`matrix_transpose([[1,2],[3,4]])`** matches the
usual matrix expectation.

Pins: **`transpose_variadic_rows_cn`**, **`transpose_single_nested_aoa_columns_wrapped_bug_cn`**,
**`matrix_transpose_nested_aoa_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**, and **`matrix_transpose_nested_two_by_two_cs`** in **`tests/suite/behavior_pin_2026_05_cs.rs`**.

## BUG-160 — **`count_regex_matches`** argument order differs from **`split_regex` / `match_all` / `replace_regex`** — **`polish`**

**`count_regex_matches(STR, PATTERN)`** puts the **haystack first**. The other regex helpers in the
same family take **pattern-first** call sites: **`split_regex(PAT, STR)`**, **`match_all(PAT, STR)`**,
**`replace_regex(PAT, REPL, STR)`**. Easy to permute arguments when mixing builtins in one script.

Pins: **`count_regex_matches_digits_cn`**, **`split_regex_csv_cn`**, **`match_all_digit_pattern_first_cn`**,
**`replace_regex_global_digits_cn`** in **`tests/suite/behavior_pin_2026_05_cn.rs`**.

## BUG-161 — **`percentile`** vs **`quantile`**: **percent scale** (0–100) **and** operand order differs — **`polish`**

**`builtin_percentile`** takes **`(P, LIST...)`** — the **probability mass** is **`args.first()`**, clamped to
**`[0, 100]`**, and the sample is **`args[1..]`**. **`builtin_quantile`** takes **`(LIST..., P)`** — **all but the
last** argument are data values, and **`P`** is **`args.last()`** in the **`[0, 1]`** interval with linear
interpolation between sorted neighbors.

So **`percentile(0.5, DATA)`** is **not** “half”; it is the **0.5th percentile** (bottom bucket after rounding).
The median in **`percentile`** units is **`percentile(50, DATA)`**. **`quantile(DATA, 0.5)`** is the usual **`0.5`**
quantile (**median**); the swapped call **`quantile(0.5, DATA)`** accidentally quantiles the scalar **`0.5`**
with default/leftover semantics and does **not** match **`quantile(DATA, 0.5)`**.

Pins: **`percentile_fifty_median_co`**, **`percentile_fraction_is_percent_units_not_quantile_bug_co`**,
**`quantile_half_matches_intuition_co`**, **`quantile_probability_first_arg_is_not_list_plus_p_bug_co`**,
**`percentile_zero_and_hundred_extrema_co`** in **`tests/suite/behavior_pin_2026_05_co.rs`**.

## BUG-162 — Planar **`chebyshev_distance` / `slope` / `midpoint`** are **four-scalar** APIs; vector distances differ — **`polish`**

**`chebyshev_distance`** is **`(x1, y1, x2, y2)`** on the Euclidean plane. Two bracket “point” operands
(**`chebyshev_distance([0, 0], [3, 4])`**) are not unpacked into coordinates — the call numifies the
container values and can return **`0`** instead of **`max(|Δx|, |Δy|)`**.

**`slope`** and **`midpoint`** use the same **four-numeric-actual** shape **(`x1`, `y1`, `x2`, `y2`)**.
Feeding two lists intended as paired samples does not compute a linear regression slope; it repartitions
scalars and can yield **`inf`** when the effective **Δx** clamps to zero.

Prefer **`distance` / `manhattan_distance` / `euclidean_distance`** (two vector operands) for
coordinate-array workflows; use the scalar planar builtins only when you truly mean a two-point planar
construction.

Pins: **`chebyshev_distance_four_scalars_cp`**, **`chebyshev_two_vectors_coerces_to_zero_bug_cp`**,
**`slope_four_coordinates_cp`**, **`slope_with_two_vector_args_vertical_line_inf_bug_cp`**,
**`midpoint_four_coordinates_cp`** in **`tests/suite/behavior_pin_2026_05_cp.rs`**.

## BUG-164 — **`uri_resolve` / `uri_normalize`** take **numeric byte vectors**, not **URI strings** — **`bug`**

Both helpers feed **`b81_to_bytes`**, which expands the first argument with **`arg_to_vec`** and then casts
each Perl value with **`to_number() as u8`**. Ordinary **`"http://…"`** strings therefore do not become
UTF-8 bytes — they stringify as a lump scalar that **`arg_to_vec`** does not split into octets — and
classification / “change counts” bear no relation to RFC 3986 on strings.

Pass an explicit byte array (e.g. **`[104, 116, 116, 112, …]`** for **`http…`**) if you need the
current implementation’s behaviour; do not assume **`uri_resolve(STR)`** performs reference resolution.

Pins: **`uri_resolve_byte_vector_absolute_uri_cq`**, **`uri_resolve_plain_string_misclassified_relative_bug_cq`**,
**`uri_normalize_counts_upper_bytes_cq`** in **`tests/suite/behavior_pin_2026_05_cq.rs`**.

## BUG-165 — **`string_take_while` / `string_drop_while`** filter a **leading prefix** against an **allowed-char set**, not a Perl predicate — **`polish`**

Both builtins (`math_wolfram11.rs`: **`builtin_string_take_while`**, **`builtin_string_drop_while`**) treat the
second operand as a string of characters to match from the start of the first string (greedy charset scan).
They are **not** list-style **`take_while { … }`** callback filters; passing a code ref or expecting regex-like
behaviour will not work.

Pins: **`string_take_while_charset_prefix_not_predicate_cr`**, **`string_drop_while_charset_prefix_not_predicate_cr`**
in **`tests/suite/behavior_pin_2026_05_cr.rs`**.

## BUG-168 — Bare **`hamming`** is the **DSP window**; **string Hamming distance** is **`hamming_distance`** — **`polish`**

**`window_hamming`** is exported under the bare name **`hamming`** (`builtins.rs` dispatch shares the alias with
**`window_hamming`**). The unrelated string metric lives only on **`hamming_distance`**, which routes to
**`builtin_hamming`** (characterwise mismatch count, equal lengths). Feeding two bitstrings into **`hamming(...)`**
does **not** compare them — it builds a window whose size comes from **`args[0].to_int()`** after string→number
coercion, producing window coefficients unrelated to the second “argument”.

Use **`hamming_distance($a, $b)`** for edit counts; use **`hamming($n)`** or **`window_hamming($n)`** for the taper
vector.

Pins: **`dsp_hamming_window_four_stringify_cs`**, **`string_hamming_distance_bitstrings_cs`** in
**`tests/suite/behavior_pin_2026_05_cs.rs`**.

## BUG-171 — **`ml_binary_cross_entropy(Y, P)`** returns **`inf`** when **`P ≤ 0`** or **`P ≥ 1`** — **`polish`**

**`builtin_ml_binary_cross_entropy`** (**`math_wolfram45.rs`**) guards **`ln P`** / **`ln(1−P)`** by rejecting **`p <= 0`** or **`p >= 1`**
with **`inf`**, so “certain” probabilities (**`1`**, **`0`**) are not admissible even though the analytic limit is finite on one
branch. Use **`P`** in **`(0, 1)`** (e.g. **`1 - ε`**) near the boundary.

Pins: **`ml_binary_cross_entropy_interior_cv`**, **`ml_binary_cross_entropy_prob_one_is_inf_bug_cv`** in **`tests/suite/behavior_pin_2026_05_cv.rs`**.

## BUG-172 — **`jaccard_similarity(A, B)`** on numeric vectors uses **stringified element sets** — **`polish`**

**`builtin_jaccard_similarity`** (**`builtins.rs`**) builds **`HashSet<String>`** from **`flatten_args`** over each side. Any multiset / order /
multiplicity information is lost: e.g. **`[1, 0, 1]`** and **`[0, 1, 1]`** both become **`{"0", "1"}`**, so the coefficient is **`1`**
instead of the multiset Jaccard one would expect for binary masks. For multiset-aware similarity, use primitives that compare aligned
vectors (or build explicit count maps). **`jaccard_index`** follows the same string-set pattern on **`arg_to_vec`** elements.

Pins: **`jaccard_similarity_binary_masks_collapse_to_unit_bug_cw`**, **`jaccard_similarity_unique_elements_matches_index_cw`** (contrast)
in **`tests/suite/behavior_pin_2026_05_cw.rs`**.

## BUG-174 — **`windowed` / `chunked`** treat a **bracket list** **`[LIST], N`** as a **single** list cell — **`polish`**

**`windowed_with_want`** / chunked sibling (**`list_builtins.rs`**) split **`args[..len−1]`** into raw **`StrykeValue`** cells without
**`flatten_args`** / **`to_list()`**. A **tuple** **`(1, 2, 3, 4)`** (or comma-arg tails) supplies **four** scalar slots, but **`[1, 2,
3, 4]`** is **one** slot whose length is **`1`**, so **`N > len`** and the list result is empty (**`windowed`**) or a single outer chunk
(**`chunked`**). Prefer **`windowed((…), N)`** (or **`LIST |> windowed(N)`** per compiler message) when the list is one grouped value.

Pins: **`windowed_tuple_two_overlap_three_windows_cx`**, **`windowed_bracket_array_yields_empty_bug_cx`**, **`chunked_tuple_pairs_cx`**,
**`chunked_bracket_array_single_outer_chunk_bug_cx`** in **`tests/suite/behavior_pin_2026_05_cx.rs`**.

## BUG-176 — **`base_convert(N, FROM)`** (two-arg numeric) numifies to **`"…"`** then parses in **`FROM`** radix — **`polish`**

**`builtin_base_convert`** (**`builtins_extended.rs`**) takes **`args[0].to_string()`**, **`args[1]`** as **source** radix, **`args[2]`** as **target**
radix (default **10**). **`base_convert(255, 16)`** therefore parses **`"255"`** as a **base-16** literal (**`0x255 = 597`**), not “decimal **255**
converted to hex”. Safe pattern: **`base_convert("255", 10, 16)`** (string + explicit **from**/**to**).

Pins: **`base_convert_decimal_string_to_hex_cx`**, **`base_convert_two_arg_numeric_parses_string_in_from_radix_bug_cx`** in
**`tests/suite/behavior_pin_2026_05_cx.rs`**.

## BUG-177 — **`graph_density`** expects an **adjacency list**, not **`(|V|, |E|)`** scalars — **`polish`**

**`builtin_graph_density`** (**`math_wolfram13.rs`**) calls **`parse_adj_list`** on **`args.first()`** only. **`graph_density(4, 3)`** does **not**
compute **3 / C(4, 2)**; the second argument is ignored and the numeric **`4`** is not a valid graph shell, so the density collapses to **0**
(**`n < 2`** guard or empty parse). Pass **Adjacency lists** like **`[[1], [0, 2], [1]]`**.

Pins: **`graph_density_three_node_path_cy`**, **`graph_density_spurious_numeric_pair_yields_zero_bug_cy`** in
**`tests/suite/behavior_pin_2026_05_cy.rs`**.

## BUG-178 — **`transpose`** on a **2×2 AoA** is **not** **`matrix_transpose`** — **`polish`**

For **`[[1, 2], [3, 4]]`**, **`matrix_transpose`** flips rows/columns to **`[[1, 3], [2, 4]]`**, but **`transpose([[1, 2], [3, 4]])`**
**`stringify`** as **`([[1, 2]], [[3, 4]])`** (pairs of row buckets), not the numeric adjoint layout. For linear-algebra transpose of
numeric matrices, prefer **`matrix_transpose`** (cross-ref **BUG-159** nested **`transpose`** pins where applicable).

Pins: **`matrix_transpose_two_by_two_cy`**, **`transpose_list_of_row_refs_not_matrix_transpose_bug_cy`** in
**`tests/suite/behavior_pin_2026_05_cy.rs`**.

## BUG-179 — **`pmt`** argument order is **`RATE, NPER, PV`** — **`polish`**

**`builtin_pmt`** (**`builtins_extended.rs`**) reads **`rate = args[0]`**, **`nper = args[1]`**, **`pv = args[2]`**. **`pmt(10000,
0.05/12, 360)`** is wrong if the first slot was meant to be principal: **`rate = 10000`** yields absurd payments. Excel-compatible order is
**rate → periods → present value**.

Pins: **`pmt_monthly_loan_standard_order_cz`**, **`pmt_principal_first_slot_absurd_payment_bug_cz`** in
**`tests/suite/behavior_pin_2026_05_cz.rs`**.

## BUG-181 — **`anova_oneway([[...],[...]])`** nests **one** group — **`polish`**

**`builtin_anova_oneway`** flattens **comma-separated arguments** into independent sample groups. A **single** outer arrayref
**`[[1, 2, 3], [2, 3, 4]]`** is still **one** operand → **one** merged group, so the implementation reports **`anova: need at least 2 groups`**
instead of a shape/type error. The intended call is variadic **`anova_oneway([1, 2, 3], [2, 3, 4])`** (or equivalent comma
arguments).

Pins: **`anova_oneway_variadic_two_groups_da`**, **`anova_oneway_nested_aoa_error_message_da`** in
**`tests/suite/behavior_pin_2026_05_da.rs`**.

## BUG-182 — **`trapz(YS, …)`** / **`simpson(YS, …)`** second slot is **`dx`**, not **`XS`** — **`polish`**

**`builtin_trapz`** / **`builtin_simpson`** treat **`args[0]`** as the **Y** sample vector and **`args[1]`** as optional
**`dx`** (scalar spacing). Passing **`trapz([x0,x1,…], [y0,y1,…])`** (NumPy-style paired abscissa/ordinate arrays) does **not**
integrate against the X ordinate — the second array **numifies** to a scalar step (**0** when it does not look like a single
number), yielding a **0** area with no arity error.

Pins: **`trapz_simpson_evenly_spaced_y_with_dx_one_da`**, **`trapz_two_array_operands_second_becomes_dx_zero_da`** in
**`tests/suite/behavior_pin_2026_05_da.rs`**.

## BUG-183 — **`binary_search` / `lower_bound` / `upper_bound` / `equal_range`** take **needle first** — **`polish`**

These builtins read **`args[0]`** as the **target scalar** and treat **`args[1..]`** (flattened) as the sorted list. The call
**`binary_search([1, 3, 5], 5)`** uses the **array** as the numeric target (via **`to_number`**) and **`5`** alone as the list —
yielding **not found** / bogus bounds — instead of a type error. Correct: **`binary_search(5, [1, 3, 5, 7])`**, **`lower_bound(5,
…)`**, etc.

Pins: **`binary_search_lower_upper_correct_needle_first_db`**, **`binary_search_swapped_args_not_found_db`**, **`lower_bound_swapped_args_returns_zero_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-184 — **`dice_coefficient`** (and **`overlap_coefficient`**) on **strings** are **single-token sets** — **`polish`**

**`arg_to_vec("abc")`** is **one** cell (`"abc"`), not per-character grams. **`dice_coefficient("abc", "abd")`** compares **`{abc}`** vs **`{abd}`**
(intersection **0**), not character bigrams / multiset overlap. Pass explicit lists (e.g. **`split(//, $s)`** or codepoint lists) when
character-level Dice is intended.

Pins: **`dice_coefficient_strings_singleton_tokens_db`**, **`dice_coefficient_numeric_lists_expected_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-185 — **`winsorize(PCT, DATA…)`** — **percent first** — **`polish`**

**`builtin_winsorize`** (**`builtins_extended.rs`**) uses **`args[0]`** as **`pct`** and **`flatten_args(args[1..])`** as the samples. **`winsorize([1,…], 10)`**
interprets the **array** as **`pct`** (after **`to_number`**) and **`10`** alone as the dataset — a silent garbage path. Correct:
**`winsorize(10, 1, 2, …)`** or **`winsorize(10, [ … ])`**.

Pins: **`winsorize_percent_first_bracket_list_db`**, **`winsorize_array_first_yields_scalar_noise_db`** in **`tests/suite/behavior_pin_2026_05_db.rs`**.

## BUG-186 — **`unzip`** with one nested **`[[a,b],[c,d]]`** mis-pairs columns — **`polish`**

**`builtin_unzip`** (**`builtins.rs`**) calls **`flatten_args`** on **`args`**, yielding **two** outer cells for **`[[1, 10], [2, 20]]`**, then walks that list pairwise as if it were a **flat** zipper of scalars — **`1`** with **`10`** land in the **A** column, **`[2, 20]`**’s string/int cells never participate as intended. Use **`unzip(1, 10, 2, 20)`** / **`unzip_pairs([[1, 10], [2, 20]])`** for pair rows.

Pins: **`zip_interleave_unzip_flat_dc`**, **`unzip_nested_aof_pairs_mispairs_bug_dc`** in **`tests/suite/behavior_pin_2026_05_dc.rs`**.

## BUG-188 — **`datetime_strftime`** is **`(EPOCH, FMT)`**, not strftime-first — **`polish`**

**`native_codec::datetime_strftime(epoch, fmt)`** (**`builtins.rs`** dispatch **`datetime_strftime` / `dtf`**) takes **Unix epoch** as **`args[0]`** and the **chrono format string** as **`args[1]`**. Reversing the operands feeds **`"%Y"`** through **`to_number`** as the “epoch” and uses the integer epoch as the **format pattern**, yielding useless output (pinned string differs from a real strftime of that instant).

Pins: **`datetime_strftime_epoch_then_fmt_dd`**, **`datetime_strftime_swapped_args_returns_epoch_dd`** in **`tests/suite/behavior_pin_2026_05_dd.rs`**.

## BUG-190 — **`rbinom(N, P)`** (two arguments) threads **`P`** into **`size`**, not **`prob`** — **`polish`**

**`builtin_rbinom`** (**`builtins_extended.rs`**) is **`rbinom(n, size, prob)`** with **`prob`** defaulting to **0.5** when omitted. A **two-argument** call **`rbinom(4, 0.5)`** therefore sets **`size = to_number(0.5) as usize → 0`** (Bernoulli trials loop runs **zero** times ⇒ **`k = 0`** every draw). This matches neither R’s **`rbinom(n, size, prob)`** surface when the user meant **`size = 1`**, nor an **`rbinom(n, prob)`** shorthand.

Pins: **`rbinom_two_arg_interprets_prob_as_size_bug190_de`** in **`tests/suite/behavior_pin_2026_05_de.rs`**.

## BUG-191 — **`numerical_gradient`** supplies **`$_[0]`** as the coordinate **arrayref**; **`my ($x) = @_`** treats **`$x`** as the **ref** — **`polish`**

**`builtin_numerical_gradient`** (**`math_wolfram3.rs`**) perturbs each coordinate and invokes the user sub via **`call_user_n`**, passing the current position vector for Perl as **`$_[0]`** (**`ARRAY`**). Writing **`sub { my ($x) = @_; … }`** binds **`$x`** to that **reference**. Numeric uses of **`$x`** apply **ref numification** (here **`· + ·`** drives **`length`/`1`**-style behavior), not the float **`xᵢ`**, so **`f(x+h) ≈ f(x−h)`** and the central difference reports **0**. Correct pattern: **`sub { my $a = $_[0]; my @y = @$a; … }`** (or index **`$_[0][$i]`** explicitly).

Pins: **`numerical_gradient_my_x_at_wrong_grad_bug191_de`**, **`numerical_gradient_arrayref_callback_de`** in **`tests/suite/behavior_pin_2026_05_de.rs`**.

## BUG-192 — **`lerp`** is **`lerp(A, B, T)`**, not **`lerp(T, A, B)`** — **`polish`**

**`builtin_lerp`** (**`builtins.rs`**) implements **`a + (b - a) * t`** with **`args[0] → a`**, **`args[1] → b`**, **`args[2] → t`**. Graphics / GLSL call sites often use **`mix(a,b,t)`** or a mentally **`lerp(t, a, b)`** order; here **`lerp(0.5, 10, 20)`** binds **`a=0.5`**, **`b=10`**, **`t=20`** ⇒ **`0.5 + 9.5·20 = 190.5`** instead of the halfway **15** from **`lerp(10, 20, 0.5)`**.

Pins: **`lerp_inv_lerp_smoothstep_remap_df`** (canonical **`lerp(10, 20, 0.5) → 15`**), **`lerp_shader_style_args_numify_to_giant_bug192_df`** in **`tests/suite/behavior_pin_2026_05_df.rs`**.

## BUG-193 — IDE **`black_scholes_{call,put}`** / **`bscall` / `bsput`** docs use **`S, K, r, T, σ`**, but **`builtins_extended.rs`** is **`S, K, T, r, σ`** — **`polish`**

**`builtin_black_scholes_call` / `builtin_black_scholes_put`** read **`t ← args[2]`**, **`r ← args[3]`**, **`σ ← args[4]`** (see struct comments in **`builtins_extended.rs`**). **`lsp.rs`** advertises **`($S, $K, $r, $T, $sigma)`**, swapping **time** and **rate** relative to the implementation. The shipped example **`bscall(100, 100, 0.05, 1, 0.2)`** is therefore **not** the pinned ATM price **`~10.45`**; the matching call is **`black_scholes_call(100, 100, 1, 0.05, 0.2)`**.

Pins: **`black_scholes_call_put_spot_strike_time_rate_vol_bug193_dg`**, **`bscall_doc_order_swaps_time_and_rate_bug193_dg`** in **`tests/suite/behavior_pin_2026_05_dg.rs`**.

## BUG-195 — **`romberg_quad`** is a **Richardson / trapezoid combine step** `(4^m·T_{n,m-1} − T_{n-1,m-1})/(4^m − 1)`, **not** `romberg(f, a, b, …)` integration — **`polish`**

**`builtin_romberg_quad`** (**`math_wolfram72.rs`**) ignores a callback and operates on **three scalars** already extracted from the Romberg table. Passing **`sub { … }`** as in **`romberg`** silently numifies to garbage / defaults. Use **`romberg`** for interval quadrature; use **`romberg_quad(t_n_mm1, t_nm1_mm1, m)`** only for the explicit extrapolation step.

Pins: **`romberg_integrate_vs_quad_combine_bug195_dg`** in **`tests/suite/behavior_pin_2026_05_dg.rs`**.

## BUG-196 — **`crt` / `chinese_remainder`** needs **`[r…], [m…]`** buckets — variadic **`crt(r1, m1, r2, m2)`** is silently wrong — **`polish`**

**`builtin_chinese_remainder`** (**`builtins_extended.rs`**) builds **`rems`** from **`arg_to_vec(args[0])`** and **`mods`** from **`arg_to_vec(args[1])`**. Passing four scalars **`crt(2, 5, 3, 7)`** leaves **`args[1]=5`** only — **`mods`** becomes **`[5]`** (one modulus), **`rems`** **`[2]`**, and the routine returns **`2`** instead of **`17`** for the **\(5·7\)** system. Use **`crt([2, 3], [5, 7])`** / **`chinese_remainder([…], […])`** as **`math_wolfram` / `lsp.rs`** show.

Pins: **`chinese_remainder_buckets_vs_flat_scalars_bug196_dh`** in **`tests/suite/behavior_pin_2026_05_dh.rs`**.

## BUG-197 — **`simplex_volume_3d`** is an alias of **`tetrahedron_volume`** and does **not** unpack a **4×3** point matrix — **`polish`**

**`builtin_simplex_volume_3d`** (**`math_wolfram28.rs`**) forwards **`args`** unchanged to **`builtin_tetrahedron_volume`**, which reads **`args[0..3]`** as **three 3-vectors** (`vec3` each) and leaves **`d`** at the default **`(0,0,0)`** when a **single** nested **`[[p0],[p1],[p2],[p3]]`** matrix is passed. **`simplex_volume_3d([[…]])`** therefore returns **`0`** for the unit simplex. Pass **four** operands: **`tetrahedron_volume([0,0,0], [1,0,0], [0,1,0], [0,0,1])`**.

Pins: **`tetrahedron_volume_unit_simplex_dh`**, **`simplex_volume_3d_matrix_arg_yields_zero_bug197_dh`** in **`tests/suite/behavior_pin_2026_05_dh.rs`**.

## BUG-199 — **`graph_is_tree`**, **`graph_density`, …** use **`parse_adj_list`** — treat operands as **neighbor-index lists**, not 0/1 **adjacency matrices** — **`polish`**

**`parse_adj_list`** (**`math_wolfram2.rs`**) walks each top-level row with **`arg_to_vec`** and **`to_number`**, producing **lists of neighbor indices**. A “matrix” **`[[0, 1], [1, 0]]`** is **not** interpreted as “no self-loop, one cross-edge”: row **0** becomes neighbors **`{0, 1}`** (including a **self-loop**), so **`edges ≠ n−1`** and **`graph_is_tree`** returns **`0`**. **\(K_2\)** as a path must be **`[[1], [0]]`**.

Pins: **`graph_tree_count_edges_max_degree_bug199_matrix_vs_list_di`** in **`tests/suite/behavior_pin_2026_05_di.rs`**.

## BUG-200 — **`snowball_stem_english`** consumes **Unicode codepoint integers**, not **Perl strings** — **`polish`**

**`builtin_snowball_stem_english`** (**`math_wolfram69.rs`**) calls **`b69_to_codepoints`** on **`args[0]`**. A string like **`"running"`** does not unpack into letters here, so the stem collapses to a bogus numeric **`0`** in **`stringify`**. Pass **`[114, 117, …]`** / the codepoint form the helper expects.

Pins: **`snowball_stem_english_codepoints_not_string_bug200_di`** in **`tests/suite/behavior_pin_2026_05_di.rs`**.

## BUG-204 — **`db_simhash_bit`** reads like **bit index** but implements **scalar sign** — **`polish`**

**`builtin_db_simhash_bit`** (**`math_wolfram48.rs`**) returns **`1`** when **`args[0] ≥ 0`** and **`0`** when negative — a **two-level sign quantization**, not a **bit position** extracted from a 64-bit hash word (as the name / inline doc “**bit index**” suggests). Real SimHash combines per-feature hashed bits; this helper is closer to **`signbit` / per-dimension thresholding**.

Pins: **`db_simhash_positive_is_one_bug204_dl`**, **`db_simhash_negative_is_zero_bug204_dl`** in **`tests/suite/behavior_pin_2026_05_dl.rs`**.

## BUG-001 — `clamp` direct-vs-piped heuristic misroutes single-value pipe

`clamp` uses a runtime heuristic to distinguish `clamp(MIN, MAX, LIST...)`
from a pipe-style call where the LHS is inserted at `args[0]`. The heuristic
checks "if `args[2..]` expands to multiple items, treat the first two as
min/max" (`builtins.rs:6738`). When exactly one value is passed, both call
shapes have identical arity, so the pipe form is decoded as the direct form
and clamps the *min* and *max* against the lone value:

```sh
$ stryke -e 'print clamp(0, 10, 15)'
10                      # direct, correct

$ stryke -e 'print 15 |> clamp(0, 10)'
15                      # piped, wrong — should be 10
```

Tests: not yet pinned (requires deciding which behavior is canonical
before locking it). Suggested fix: distinguish via call site (parser knows
whether it lowered a `|>`) rather than via runtime arity heuristic.

Severity: **bug**. Pipe-friendliness is a feature stryke ships, so a
broken pipe form for a documented builtin is high-visibility.


## POLISH-001 — Builtin-redefinition error tells user to use `fn` when they already did

When a `fn` declaration shadows a stryke builtin, the rejection message
reads:

> `id` is a stryke builtin and cannot be redefined (this is not Perl 5;
> use `fn` not `sub`, or pass --compat)

…but the user typed `fn` already. The message should branch on the
keyword observed:

- if `sub` — keep current text
- if `fn` — drop the "use `fn` not `sub`" half; only suggest `--compat`

Tests: `redefining_builtin_id_is_rejected`,
`redefining_builtin_squared_is_rejected` (these only assert that an error
is raised, not the wording, so they survive a wording fix).

Severity: **polish**.


## POLISH-002 — `++` on a non-lvalue reports `PostfixOp on non-scalar`

```sh
$ stryke -e '("b"++)'
VM compile error (unsupported): PostfixOp on non-scalar at -e line 0.
```

The operand is a scalar; the issue is that it is not assignable. A more
accurate message would be "Can't modify constant string in postfix ++"
(matches Perl 5 phrasing) or "postfix ++ requires an lvalue".

Severity: **polish**.


## BUG-002 — Blessed arrayrefs stringify with `HASH` tag

```sh
$ stryke -e 'my $o = bless [1,2,3], "Bar"; print "$o\n"; print ref($o)'
Bar=HASH(0x...)
Bar
$ perl   -e 'my $o = bless [1,2,3], "Bar"; print "$o\n"; print ref($o)'
Bar=ARRAY(0x559abc...)
Bar
```

`ref()` correctly returns `Bar`; only the stringification is wrong (always
`HASH`). The `0x...` literal placeholder is intentional (stryke does not
expose live addresses).

Tests: `bless_arrayref_stringifies_with_hash_tag_today`.

Severity: **bug**.


## BUG-003 — `$self->SUPER::method` overflows the stack inside `class extends`

```sh
$ stryke -e '
class Animal { fn speak { "generic" } }
class Dog extends Animal { fn speak { "woof+" . $self->SUPER::speak } }
say Dog()->speak;'
thread 'main' has overflowed its stack
fatal runtime error: stack overflow, aborting
```

The Perl-5-style `our @ISA = (...)` + `$self->SUPER::speak` form works
correctly (see `perl5_super_call_through_isa_works`). The bug is specific
to the native-class MRO path.

Tests: `class_extends_overrides_parent_method` (works without SUPER),
`perl5_super_call_through_isa_works` (the path that does work).

Severity: **bug**. Almost any non-trivial class hierarchy will need
`SUPER::`; without it, `extends` is half-broken.


## BUG-004 — Pipe `|>` with arrayref LHS does not auto-dereference

```sh
$ stryke -e 'my @a = (1..5); print @a |> sum'
15                                  # @-array LHS works
$ stryke -e 'print [1..5] |> sum'
0                                   # arrayref LHS broken
$ stryke -e 'my @r = [1..5] |> map { _ * 2 }; print scalar @r, ":", $r[0]'
1:0                                 # one iteration with _ = the arrayref
```

Either of two fixes is reasonable: auto-deref arrayref LHS into a list, or
reject arrayref LHS at parse time so the user is forced to write `@$ref |>`.

Tests: `pipe_with_arrayref_into_sum_returns_zero_today`,
`pipe_with_arrayref_through_map_returns_single_zero_today`,
`pipe_with_array_var_through_map_and_sum` (the form that works).

Severity: **bug**. Arrayrefs are the natural unit of data flow in stryke
(every pipe-friendly builtin returns one), so a broken pipe entry-point
for arrayrefs is high-visibility.


## BUG-006 — `chomp @array` does not behave as documented

```sh
$ stryke -e 'my @s = ("a\n", "b\n"); chomp @s; print join("|", @s)'
2
$ perl   -e 'my @s = ("a\n", "b\n"); chomp @s; print join("|", @s)'
a|b
```

The number `2` is the count of items in `@s` (or the chomp count, which
would be `2` regardless). Whether the array is mutated is unclear from this
output alone — needs a focused investigation. Pinning is deferred until the
behavior is understood.

Severity: **bug** (pending root-cause analysis).


## BUG-007 — `Util->greet(...)` of a `Self.greet($name)` static method passes class as first arg

```sh
$ stryke -e '
class Util { fn Self.greet($name) { "hi, $name" } }
say Util->greet("world");'
hi, Util
```

`Util->greet("world")` should either be rejected (this is a static method,
call it as `Util.greet("world")`) or strip the class name from the front
of `@_` before binding. Today the user gets a silent argument shift.

Tests: `arrow_invoke_of_static_method_passes_class_as_first_arg_today`.

Severity: **bug**.


## POLISH-003 — `say BAREWORD()->method()` parses BAREWORD as a filehandle

```sh
$ stryke -e 'class C { fn m2($x) { $x * 2 } } say C()->m2(5)'
print on unopened filehandle C at -e line 1.
$ stryke -e 'class C { fn m2($x) { $x * 2 } } say(C()->m2(5))'
10
```

Workaround: parenthesize the argument to `say`. The error message at
least names the offending bareword, which helps; a smarter
"is-this-a-class?" check could give a friendlier hint.

Severity: **polish**.


## POLISH-004 — Class method named `m` is parsed as the regex-match operator

```sh
$ stryke -e 'class C { fn m($x, $y) { $x + $y } }'
Expected method name, got Regex("$x, $y", "", '(') at -e line 1.
```

The lexer sees `m(` after `fn` and commits to the regex-match form. A
post-`fn` lookahead would resolve this. Workaround: name the method
something other than `m` (or `s`, `tr`, `y`, `qr`, `q`, `qq`, `qw`).

Severity: **polish**.


## BUG-012 — `each %hash` always returns an empty list

```sh
$ stryke -e 'my %h = (a=>1); my @kv = each %h; print scalar @kv'
0
$ perl   -e 'my %h = (a=>1); my @kv = each %h; print scalar @kv'
2
```

The companion `while (my ($k, $v) = each %h)` form is rejected at VM
lowering with "my/our/state/local in expression context with multiple or
non-scalar decls". `keys`/`values` work correctly, so iteration is
possible — just not in the `each` style.

Tests: `each_returns_empty_list_today`,
`while_my_pair_each_rejected_at_runtime_today`.

Severity: **bug**. Standard hash iterator; many libraries use it.


## BUG-015 — Reference `==` always returns true (placeholder address)

Stryke deliberately stringifies refs as `KIND(0x...)` with a literal
placeholder rather than a live address (this is a documented design
choice). The numeric form of a ref is therefore always 0, and `==` between
any two refs is always true:

```sh
$ stryke -e 'my @a; my @b; print \@a == \@b ? "eq" : "ne"'
eq
$ stryke -e 'my @a; print 0 + \@a'
0
```

Tests: `ref_numeric_value_is_zero_today`.

Severity: **bug**. The fix is either to expose live addresses (loses the
deterministic-output property) or to compare refs by identity for `==`
without going through numification.


## BUG-020 — `$\`` (pre-match) does not parse outside string interpolation

```sh
$ stryke -e '"hello world" =~ /world/; my $p = $`; print "[$p]"'
Expected variable name after $ at -e line 1.
```

Workaround: `use English; my $p = $PREMATCH;` — that does parse and
captures correctly.

Tests: `premuf_via_english_alias_works`.

Severity: **bug** (low impact; rare idiom).


## BUG-021 — Scalar-ref to arrayref unwrap fails

```sh
$ stryke -e 'my $x = [1,2,3]; my $r = \$x; print $$r->[0]'
Can't use arrow deref on non-array-ref at -e line 1.
$ perl   -e 'my $x = [1,2,3]; my $r = \$x; print $$r->[0]'
1
```

Same with `${$r}->[0]`. The double-deref to reach an arrayref through a
scalar-ref intermediary is rejected.

Tests: `scalar_ref_to_arrayref_unwrap_fails_today`.

Severity: **bug**.


## BUG-022 — `weaken` runs but `isweak` always returns 0

```sh
$ stryke -e 'my $a = [1]; my $b = $a; weaken($b); print isweak($b) ? "weak" : "strong"'
strong
$ perl -MScalar::Util=weaken,isweak -e '...'
weak
```

Tests: `weaken_does_not_make_isweak_true_today`.

Severity: **bug**. Weak-ref semantics are needed for cycle-breaking; if
`weaken` is a no-op then long-lived parent/child structures will leak.


## BUG-023 — Autovivification of nested hash/array fails

```sh
$ stryke -e 'my %h; $h{k}[0] = "first"; print "@{$h{k}}"'
Can't assign to arrow array deref on non-array-ref at -e line 1.
$ perl   -e 'my %h; $h{k}[0] = "first"; print "@{$h{k}}"'
first
```

Workaround: pre-allocate the inner ref:
`$h{k} = []; $h{k}[0] = "first";`.

Tests: `autoviv_hash_then_array_index_fails_today`.

Severity: **bug**. Autovivification is a major Perl ergonomic feature.


## BUG-024 — `given/when` fails inside subs and with arrayref patterns

Two related failures, both raise "unexpected control flow in tree-assisted
opcode":

```sh
# 1. arrayref smart-match
$ stryke -e 'use feature "switch"; my $x = 3;
             given ($x) { when ([1..5]) { print "low" } default { print "?" } }'
unexpected control flow in tree-assisted opcode

# 2. given/when wrapped in a sub
$ stryke -e 'use feature "switch";
             sub g { my $x = $_[0]; given ($x) { when ("hi") { return "M" } default { return "N" } } }
             print g("hi")'
unexpected control flow in tree-assisted opcode
```

Top-level `given/when` with scalar literals or `/regex/` works fine.

Tests: `given_when_arrayref_range_fails_today`,
`given_when_inside_sub_fails_today`.

Severity: **bug**. The sub-wrapped form is the way most code uses
given/when.


## BUG-032 — `$&` not interpolated in `s///` replacement string

```sh
$ stryke -e 'my $s = "abc 123"; $s =~ s/(\d+)/$&/g; print $s'
abc $&
$ perl   -e 'my $s = "abc 123"; $s =~ s/(\d+)/$&/g; print $s'
abc 123
```

Numbered captures (`$1`, `$2`, …) DO interpolate in replacements; only
`$&` is broken. (Same root issue as BUG-029 for double-quoted strings.)

Tests: `dollar_amp_not_interpolated_in_replacement_today`,
`captures_dollar_one_dollar_two_work_in_replacement`.

Severity: **bug**.


## BUG-033 — Multiple heredocs on a single line not supported

```sh
$ stryke -e 'print <<A, <<B;
A1
A
B1
B
'
Undefined subroutine &B1 at -e line 5.
```

Stryke consumes the first heredoc body correctly but then parses the
second body as code instead of as the second heredoc's content.
Workaround: split into separate prints.

Tests: `multiple_heredocs_on_same_line_not_supported_today`.

Severity: **bug**.


## BUG-035 — `open "-|", "cmd", "arg"` list form drops the extra args

```sh
$ stryke -e 'open my $fh, "-|", "echo", "hi"; my $l = <$fh>; print "[", $l, "]"'
[
]                       # `echo` ran with no arg, only "\n" came back
$ stryke -e 'open my $fh, "-|", "echo hi"; my $l = <$fh>; print "[", $l, "]"'
[hi
]                       # single-string shell form works
$ perl   -e 'open my $fh, "-|", "echo", "hi"; my $l = <$fh>; print "[", $l, "]"'
[hi
]
```

Tests: `pipe_open_read_string_form_captures_subprocess_stdout`,
`pipe_open_read_list_form_drops_args_today`.

Severity: **bug**. The list form is the safe (no-shell-quoting) idiom and
should be preferred.


## BUG-036 — `$obj->can("method")` returns a coderef that doesn't actually invoke

```sh
$ stryke -e '
package Cat; sub new { bless {}, shift } sub meow { "meow!" }
package main;
my $c = Cat->new;
my $m = $c->can("meow");
print "ref=", ref($m), " direct=", $c->meow, " via=", $m->($c) // "U"'
ref=CODE direct=meow! via=U

$ perl ...
ref=CODE direct=meow! via=meow!
```

`can` correctly returns a CODE reference for an existing method, but
calling that ref with the receiver as the first arg returns undef instead
of running the method body. Direct invocation works.

Tests: `can_returns_coderef_but_invocation_returns_undef_today`,
`can_returns_truthy_for_existing_method`,
`can_returns_falsy_for_missing_method`.

Severity: **bug**. Common idiom: `$obj->can($method) and $obj->$method(...)`
relies on the returned ref actually calling through.


## BUG-038 — `pos($s)` returns undef outside the `while (//g)` form

```sh
$ stryke -e 'my $s = "abc"; $s =~ /a/g; print defined(pos($s)) ? "Y" : "N"'
N
$ perl   -e 'my $s = "abc"; $s =~ /a/g; print defined(pos($s)) ? "Y" : "N"'
Y
```

The `while ($s =~ /g)` loop form correctly reports `pos()` at each
iteration; pinning the working form ensures we don't lose it. Stand-alone
`/g` followed by `pos()` returns undef.

Tests: `pos_outside_while_loop_is_undef_today`,
`pos_advances_with_each_g_match`.

Severity: **bug** (low impact).


## BUG-039 — `<*.ext>` angle-bracket glob shorthand not parsed

```sh
$ stryke -e 'my @f = </etc/host*>; print scalar @f'
Unexpected token NumLt at -e line 1.
$ stryke -e 'my @f = glob "/etc/host*"; print scalar @f'
3
```

Workaround: use the `glob` function form, which works correctly.

Tests: `angle_bracket_glob_form_is_parse_error_today`,
`glob_function_form_lists_matches`.

Severity: **bug** (small surface).


## BUG-040 — `tie $var, $class, ...` does not invoke FETCH/STORE

```sh
$ stryke -e '
package T; sub TIESCALAR { my ($cls, $v) = @_; bless \$v, $cls }
sub FETCH { "fetched:" . ${$_[0]} }
sub STORE { ${$_[0]} = $_[1] . "!" }
package main;
my $x; tie $x, "T", "init"; print $x; $x = "new"; print "/", $x'
/new                    # stryke (FETCH never fires)
$ perl ...
fetched:init/fetched:new!
```

`tie` does not error, but neither FETCH nor STORE is called on subsequent
reads/writes; the variable behaves as untied.

Tests: `tie_scalar_fetch_store_not_invoked_today`.

Severity: **bug**. Tied vars are how DBM/file-backed scalars work in
Perl modules.


## BUG-041 — `\@` prototype does not auto-take ref of array argument

```sh
$ stryke -e 'sub f (\@) { sort @{$_[0]} }
            my @a = (3,1,2);
            my @r = f(@a);
            print "@r"'
Can't dereference non-reference as array at -e line 1.
$ perl ...
1 2 3
```

The Perl convention is that `\@` in a prototype causes `f(@a)` to be
silently rewritten as `f(\@a)` so the callee receives a single arrayref
in `$_[0]`. Stryke passes the flattened array elements instead.

Workaround: drop the prototype and have callers pass `\@a` explicitly.

Tests: `backslash_at_prototype_does_not_auto_take_ref_today`.

Severity: **bug**.


## BUG-044 — AOP `after` block sees `$?` as 0, not the original return value

```sh
$ stryke -e '
fn payload { 42 }
after "payload" { print "got $? "; }
payload();'
got 0
```

The `aop.rs` module's preamble explicitly documents `$?` as the original
return value:

> after  "<glob>" { ... }   # run after; sees $INTERCEPT_MS, $INTERCEPT_US, $? (retval)

Stryke populates the timing variables (`$INTERCEPT_MS`, `$INTERCEPT_US`)
correctly and exposes the sub name in `$INTERCEPT_NAME`, but `$?` is
always 0 inside the after block. Workaround: use `around` with `proceed()`
and inspect the return value directly.

Tests: `aop_after_dollar_question_is_zero_not_return_value_today`,
`aop_intercept_name_visible_in_after` (the parts that work).

Severity: **bug**. Documented behavior diverges from observed.


## PARITY-017 — Embedded code blocks `(?{ ... })` not supported in regex

```sh
$ stryke -e '"abc" =~ /a(?{ "side" })b/'
Invalid regex /a(?{ "side" })b/: PCRE2: error compiling pattern at offset 3: unrecognized character after (? or (?-
```

stryke uses PCRE2, which deliberately omits Perl's `(?{...})` (embedded
code) and `(??{...})` (deferred-eval pattern) extensions because they
require runtime escape into the host language. Recursive patterns
(`(?R)`), conditional patterns (`(?(1)yes|no)`) and atomic groups
(`(?>...)`) all work.

Tests: `embedded_code_in_regex_is_rejected_today`,
`regex_recursion_via_question_r_works`,
`regex_conditional_pattern_works`,
`regex_atomic_group_prevents_backtrack`.

Severity: **parity** (intentional engine choice).


## BUG-046 — `trait` cannot declare fields

```sh
$ stryke -e 'trait Counter { count: Int = 0; fn inc { 1 } }'
Expected `fn` in trait definition at -e line 1.
```

Stryke's `trait` blocks accept only `fn` declarations; fields must live
in the impl'ing class. Moose `role`s by contrast can declare attributes.

Tests: `trait_with_field_is_parse_error_today`.

Severity: **parity / design choice**. Worth deciding whether to keep
trait-as-method-only or extend to attributes.


## BUG-047 — `ARRAY` / `ArrayRef` / `HashRef` field/param types fail to match

```sh
$ stryke -e 'class S { items: ARRAY = [] } S()'
class S field `items`: expected ARRAY, got ARRAY at -e line 1.
$ stryke -e 'class S { items: ArrayRef = [] } S()'
class S field `items`: expected ArrayRef, got ARRAY at -e line 1.
$ stryke -e 'class S { items: Array = [] } S()'
                       # works
```

Stryke's supported type names are `Int`, `Str`, `Float`, `Bool`,
`Array`, `Hash`, `Ref`, `Any` (any unrecognized name becomes
`Struct(name)`, which always type-mismatches the runtime tag for
arrayrefs/hashrefs). Anyone coming from Moose-land will reach for
`ArrayRef`/`HashRef` first and get a confusing error.

Tests: `class_field_array_uppercase_keyword_does_not_match_array_default_today`,
`class_field_arrayref_keyword_does_not_match_array_default_today`,
`class_field_array_type_accepts_arrayref_default` (the form that works).

Severity: **bug** (high friction). Either accept the Moose names as
aliases or improve the error message to say "did you mean `Array`?".


## BUG-048 — `ref()` on stryke-native class instances returns the empty string

```sh
$ stryke -e 'class C { v: Int = 0 } my $c = C(v => 5);
            print "[", ref($c), "]/", $c->isa("C") ? "Y" : "N"'
[]/Y
$ stryke -e 'my $h = bless { v => 0 }, "H"; print ref($h)'
H
```

`isa()` works correctly; the bug is specific to `ref()`. Moose-style
`ref($obj) eq "ClassName"` checks across the codebase silently fail,
which can quietly route data through default branches.

Tests: `ref_of_stryke_class_instance_is_empty_today`,
`ref_of_blessed_hashref_returns_class_name`.

Severity: **bug**.


## PARITY-018 — `printf "%d"` with float overflow saturates instead of wrapping

```sh
$ stryke -e 'printf "%d", 1e20'
9223372036854775807                 # i64::MAX
$ perl   -e 'printf "%d", 1e20'
-1                                  # wraps modulo 2^64
```

Stryke uses Rust's `as i64` which saturates; Perl uses C's `long`-style
cast which wraps. Neither matches a useful "bigint" answer — the value
1e20 simply doesn't fit in 64 bits.

Tests: `printf_d_with_large_float_saturates_to_i64_max_today`.

Severity: **parity** (defined behavior; differs from Perl).


## BUG-051 — PerlIO layers in `open` mode strings are rejected

```sh
$ stryke -e 'open my $fh, ">:utf8", "/tmp/x"'
Unknown open mode '>:utf8' at -e line 1.
$ stryke -e 'open my $fh, "<:raw", "/tmp/x"'
Unknown open mode '<:raw' at -e line 1.
```

Workaround: the bare `>` / `<` modes work; data is byte-stream by
default. Programs that need encoding can `Encode::decode("UTF-8", $bytes)`
once the data is read in. (Encode itself is not loaded today either —
see BUG-052.)

Tests: `open_with_utf8_layer_is_rejected_today`.

Severity: **bug**.


## BUG-052 — `prototype("BUILTIN")` returns empty for built-ins

```sh
$ stryke -e 'print "[", prototype("push"), "]"'
[]
$ stryke -e 'print "[", prototype("scalar"), "]"'
[]
$ perl   -e 'print "[", prototype("push"), "]/[", prototype("scalar"), "]"'
[+@]/[$]
```

User-defined subs still report their prototypes correctly:

```sh
$ stryke -e 'sub myf ($) { 1 } print prototype \&myf'
$
```

Tests: `prototype_of_push_is_empty_today`,
`prototype_of_scalar_is_empty_today`,
`prototype_of_user_sub_returns_proto_string`.

Severity: **bug**.


## BUG-055 — `\U` / `\L` not honored in `s///` replacement

```sh
$ stryke -e 'my $s = "abc def"; $s =~ s/\b(\w)/\U$1/g; print $s'
\Uabc \Udef
$ perl   -e 'my $s = "abc def"; $s =~ s/\b(\w)/\U$1/g; print $s'
Abc Def
```

`\U`/`\L`/`\u`/`\l` work correctly inside double-quoted string
interpolation; only the substitution-replacement path is broken.
Workaround: the `/e` flag with a `uc()`/`lc()` call:

```sh
$ stryke -e 'my $s = "abc def"; $s =~ s/\b(\w)/uc($1)/ge; print $s'
Abc Def
```

Tests: `upper_case_escape_in_substitution_is_literal_today`,
`s_e_flag_with_uc_call_works`,
`upper_case_escape_uppercases_until_e` (the interpolation path that works).

Severity: **bug**.


## BUG-056 — `%-` (named multi-capture hash) keeps only the last match

```sh
$ stryke -e '"abc 123 def 456" =~ /(?<wd>\w+)/g; print join(",", @{$-{wd}})'
456
$ perl   -e '"abc 123 def 456" =~ /(?<wd>\w+)/g; print join(",", @{$-{wd}})'
abc,123,def,456
```

`%+` (single-match named hash) works correctly. `%-` is for accumulating
all `/g` matches; stryke overwrites instead of appending.

Tests: `percent_minus_multi_capture_returns_only_last_today`,
`percent_plus_named_capture_works`.

Severity: **bug**.


## BUG-058 — `chunk(N, LIST)` returns one arrayref instead of N-sized groups

```sh
$ stryke -e 'my @r = chunk(2, 1..6); print scalar @r'
1
$ stryke -e 'my @r = chunk_n(2, 1..6); print scalar @r'
3
```

The `chunk` builtin behaves as a no-op grouping (single arrayref). The
`chunk_n` builtin does what users probably mean. Either rename `chunk` →
`chunk_n` and add an alias, or fix `chunk` to mean N-sized groups.

Tests: `chunk_alone_returns_one_arrayref_today`,
`chunk_n_groups_into_runs_of_n`,
`chunk_while_groups_consecutive_runs`.

Severity: **bug** (high friction; the conventional name is broken).


## BUG-059 — `partition(sub { ... }, LIST)` returns empty arrays

```sh
$ stryke -e 'my @r = partition(sub { $_ > 3 }, 1..6);
            print "0=[", join(",",@{$r[0]}), "] 1=[", join(",",@{$r[1]}), "]"'
0=[] 1=[]
$ stryke -e 'my @r = partition { _ > 3 } 1..6;
            print "0=[", join(",",@{$r[0]}), "] 1=[", join(",",@{$r[1]}), "]"'
0=[4,5,6] 1=[1,2,3]
```

Stryke's block form (no `sub` keyword) works correctly. The Perl-style
`sub { ... }` form parses but silently returns empty.

Tests: `partition_block_form_splits_into_yes_and_no`,
`partition_sub_form_returns_empty_arrays_today`.

Severity: **bug**.


## BUG-060 — Range flip-flop in scalar context evaluates as a list-range

```sh
$ stryke -e 'for my $i (1..6) { print "$i;" if $i == 2 .. $i == 4 }'
1;3;4;5;6;
$ perl   -e 'for my $i (1..6) { print "$i;" if $i == 2 .. $i == 4 }'
2;3;4;
```

The flip-flop operator (Perl `..` in scalar context) is meant to track a
state machine: false until the left side becomes true (state on, emit a
firing token), true until the right side becomes true (state off). Stryke
evaluates `0 .. 0` as the list-range `(0)` — a non-empty list, therefore
truthy — and `1 .. 0` as the empty descending list.

Workaround: build the state machine manually with a closure-captured flag.

Tests: `range_flip_flop_in_conditional_evaluates_as_list_today`.

Severity: **bug**.


## BUG-061 — `pairs()` returns Pair objects that don't array-deref

```sh
$ stryke -e 'my @r = pairs(a => 1, b => 2); print ref $r[0]'
Pair
$ stryke -e 'my @r = pairs(a => 1, b => 2); my @kv = @{$r[0]}'
Can't dereference non-reference as array at -e line 1.
```

In Perl's `List::Util`, `pairs(...)` returns blessed two-element arrayrefs
that respond to both `->key`/`->value` and `@{$_}` patterns. Stryke's
Pair type only supports the method form.

Tests: `pairs_returns_pair_ref_kind_today`,
`pair_object_does_not_array_deref_today`.

Severity: **bug** (compat).


## BUG-062 — `group_by(sub { ... }, LIST)` parse error

```sh
$ stryke -e 'my %g = group_by(sub { $_ % 2 }, 1..6)'
Expected Comma, got Semicolon at -e line 1.
```

Same root cause as BUG-059 (partition): the `sub { ... }` calling
convention isn't accepted. Block form (`group_by { _ % 2 } 1..6`) parses
but produces a hash with a stringified-arrayref key. No working form
discovered yet.

Tests: `group_by_with_sub_keyword_is_parse_error_today`.

Severity: **bug**.


## BUG-063 — `take(N, LIST)` / `step(N, LIST)` argument order returns empty

```sh
$ stryke -e 'my @r = take(3, 1..10); print "@r"'

$ stryke -e 'my @r = take(qw(a b c d), 2); print "@r"'
a b
```

Stryke's signature is `take(LIST, COUNT)` — list first. The Perl-ish
`take(N, LIST)` ordering returns nothing. `step` has the same shape.

Tests: `take_list_then_count_keeps_first_n`,
`take_n_first_signature_returns_empty_today`,
`take_bareword_with_n_first_returns_empty_today`,
`step_with_n_first_signature_returns_empty_today`.

Severity: **bug** (calling-convention surprise; existing tests show the
list-first form is the contract).


## BUG-065 — `head(N, LIST)` returns just `N` instead of first N elements

```sh
$ stryke -e 'my @r = head(qw(a b c d e), 3); print "@r"'
a b c
$ stryke -e 'my @r = head(3, qw(a b c d e)); print "@r"'
3
```

The `(LIST, N)` order is the working contract — same as `take`, `drop`,
`tail`. The `(N, LIST)` form silently returns `(N)`.

Tests: `head_list_then_n_returns_first_n`,
`head_n_first_returns_just_n_today`,
`tail_list_then_count_returns_last_n`.

Severity: **bug** (calling-convention surprise).


## BUG-066 — `pairwise { $a + $b } @a, @b` returns empty list

```sh
$ stryke -e 'my @a = (1,2,3); my @b = (10,20,30);
            my @r = pairwise { $a + $b } @a, @b;
            print scalar @r'
0
$ perl -MList::Util=pairwise -e '...'
3                       # (11, 22, 33)
```

Stryke's `pairwise` builtin parses but produces nothing. Workaround:
manual `map` over indices.

Tests: `pairwise_block_form_returns_empty_today`.

Severity: **bug**.


## BUG-068 — AOP advice cannot mutate `@INTERCEPT_ARGS` or call `proceed(NEW_ARGS)`

```sh
$ stryke -e '
fn greet($name) { "hi $name" }
around "greet" {
  $INTERCEPT_ARGS[0] = uc($INTERCEPT_ARGS[0]);   # ignored
  proceed(uc($INTERCEPT_ARGS[0]));               # also ignored
}
print greet("world")'
hi world
```

Both the in-place mutation of `@INTERCEPT_ARGS` and the explicit-args
form `proceed(LIST)` get dropped — the original args reach the wrapped
sub. This makes around-advice unable to rewrite arguments.

Tests: `intercept_args_array_visible_in_before` (read-only access works),
`intercept_args_mutation_does_not_propagate_today`,
`proceed_with_explicit_args_does_not_override_today`.

Severity: **bug**. Argument-rewriting is a common AOP use case.


## BUG-069 — Multiple `around` advice for the same target does not compose

```sh
$ stryke -e '
fn val { 1 }
around "val" { proceed() + 10 }
around "val" { proceed() * 100 }
print val()'
11                       # only first registration applied
```

Perl-style aspect ordering would either compose both (e.g. 110) or stack
in declaration order. Stryke uses only the first registered around block.

Tests: `multiple_around_advice_does_not_compose_today`,
`multiple_before_and_after_fire_in_order` (the form that does work).

Severity: **bug**.


## BUG-070 — Explicit `return` inside `around` body is rejected by lowering

```sh
$ stryke -e '
fn val { 1 }
around "val" { my $r = proceed(); return $r + 10 }
val()'
AOP around advice body for `val` could not be lowered to bytecode
(likely contains a construct unsupported by block lowering, e.g. a literal `return`);
rewrite the body without it at -e line 3.
```

Implicit final-expression form (`{ proceed() + 10 }`) works. The error
message is helpful and tells the user to rewrite — pinned both forms so
the workaround stays valid if/when the underlying limitation is lifted.

Tests: `explicit_return_in_around_block_is_rejected_today`,
`implicit_final_value_in_around_is_used_as_return`.

Severity: **bug**.


## BUG-071 — `before`-advice `die` does not propagate to the caller's `eval`

```sh
$ stryke -e '
fn payload { print "G " }
before "payload" { print "B "; die "blocked\n" }
eval { payload() };
print "[$@]"'
B G G []        # before ran, original ran twice (?), $@ is empty
```

The `before` block's `die` neither aborts the call nor reaches `$@`
through the surrounding `eval`. Workarounds: handle the early-abort case
inside `before` itself, or move the gate into `around { ... }` and skip
`proceed()`.

Tests: `before_advice_die_does_not_propagate_today`.

Severity: **bug**.


## BUG-072 — `--lint` accepts strict-violating sources that runtime catches

```sh
$ stryke --lint -e 'use strict; $undeclared_xx = 5'
-e compile OK
$ stryke -e 'use strict; $undeclared_xx = 5'
Global symbol "$undeclared_xx" requires explicit package name (did you
forget to declare "my $undeclared_xx"?) at -e line 1.
```

Perl's `perl -c` catches this at compile time. Stryke's `--lint` only
runs through bytecode lowering and doesn't apply the strict-pragma
checker. Workaround: run the script for real (or wrap in `eval` and
inspect `$@`).

Tests: `parse_ok_for_strict_violator_but_runtime_fails`.

Severity: **bug** (the whole purpose of `--lint` is compile-time
verification).


## BUG-003 (expanded) — Three-level Perl-5 ISA + `SUPER::` chain also stack-overflows

The original BUG-003 was filed against stryke-native `class extends` +
`SUPER::`. This iteration confirmed the issue is broader: a three-class
Perl-5-style hierarchy (`our @ISA = ("Parent")`) where each level calls
`$self->SUPER::name` on the way up also overflows the stack:

```sh
$ stryke -e '
package A; sub new { bless {}, shift } sub name { "A" }
package B; our @ISA = ("A"); sub name { my $self = shift; $self->SUPER::name . "B" }
package C; our @ISA = ("B"); sub name { my $self = shift; $self->SUPER::name . "C" }
package main;
print C->new->name'
thread 'main' has overflowed its stack
```

Two-level chains (`A` → `B`) work; three or more crash. Method-resolution
state seems to lose its position cursor on the second hop.

Tests: `perl5_super_one_level_chain_works`,
`perl5_three_level_super_chain_at_least_parses`.

Severity: **bug**. Limits practical class hierarchies.


## BUG-073 — `BUILDARGS` method on a class is never invoked

```sh
$ stryke -e '
class Cat {
  name: Str = "?"
  fn BUILDARGS { print "BUILDARGS "; @_ }
  fn BUILD     { print "BUILD " }
}
Cat(name => "Felix")'
BUILD                       # BUILDARGS missing
```

`BUILD` is invoked correctly. `BUILDARGS` (the Moose-style hook for
preprocessing constructor arguments) is silently skipped. Workaround:
override `Self.new` to do the preprocessing directly.

Tests: `class_buildargs_method_not_invoked_today`,
`class_build_method_runs_at_construction`.

Severity: **bug** (compat with Moose-shaped class libraries).


## BUG-074 — `struct` lacks a `Pkg::new(...)` constructor

```sh
$ stryke -e 'struct Pt { x => Int, y => Int } Pt::new(3, 4)'
Undefined subroutine &Pt::new at -e line 1.
$ stryke -e 'struct Pt { x => Int, y => Int } Pt(3, 4)'
                       # works
```

Use the bareword constructor (`Pt(...)`). The Perl-classic `Pkg::new(...)`
form is only generated for `class` definitions, not `struct`s.

Tests: `struct_does_not_have_pkg_new_today`,
`struct_positional_construction_assigns_fields`.

Severity: **bug** (small surface).


## BUG-075 — `refaddr(\@a)` returns a fresh address per `\@a` evaluation

```sh
$ stryke -e 'my @a; print refaddr(\@a) == refaddr(\@a) ? "eq" : "ne"'
ne
$ perl -MScalar::Util=refaddr -e 'my @a; print refaddr(\@a) == refaddr(\@a) ? "eq" : "ne"'
eq
```

Each `\@a` evaluation creates a new ref-cell; stryke's `refaddr` returns
the cell's address rather than the underlying array's address. Aliased
copies (`my $s = $r`) do share the same refaddr, so propagating a captured
ref still works.

Tests: `refaddr_of_repeated_backslash_at_returns_different_addresses_today`,
`refaddr_of_aliased_scalar_is_same`.

Severity: **bug**. Common idiom for ref-identity tests
(`refaddr(\@a) == refaddr(\@b)`) gives wrong answers.


## BUG-077 — Postfix `for` modifier rejected on `my @r = ...` form

```sh
$ stryke -e 'sub f { @_ } my @r = f($_) for (1, 2, 3)'
postfix `for` is not supported on this statement form at -e line 1.
```

Other postfix-`for` forms work (`$x .= "y" for 1..3` is fine). The
`my @r = EXPR for LIST` shape is parser-rejected.

Tests: `postfix_for_on_my_at_assign_is_rejected_today`,
`postfix_for_on_simple_expression_works`.

Severity: **bug**.


## BUG-078 — BEGIN blocks run but their writes to package vars are lost

```sh
$ stryke -e '
our $log = "";
BEGIN { $main::log .= "B:" }
$log .= "M:";
print "[$log]"'
[M:]                        # B: lost
```

When BEGIN's `print`/`die` side effect is observed via stdout/stderr, it
runs as expected. But mutating `our`-declared globals from inside BEGIN
does not persist into the main body. Probably the BEGIN block's
compilation phase resets after main-body parsing assigns the initial
value.

Tests: `begin_block_mutations_to_package_vars_lost_today`,
`begin_runs_before_main_code_in_declaration_order` (the parse-only check).

Severity: **bug**.


## BUG-081 — `use integer` pragma is not honored

```sh
$ stryke -e 'use integer; print 7 / 3'
2.33333333333333                # CLI: float division
$ stryke ...via lib eval...
Can't locate integer.pm in @INC
```

The CLI silently ignores `use integer`; the library `eval` API tries to
load `integer.pm` from @INC and fails. Either path should switch `/` to
integer truncation when `use integer` is in scope.

Tests: `use_integer_pragma_lib_path_tries_to_load_integer_pm_today`,
`use_integer_pragma_at_least_parses`.

Severity: **bug**.


## BUG-083 — Regex `/n` flag (no auto-capture) not supported

```sh
$ stryke -e '"abc" =~ /(\w+)/n'                  # CLI
Undefined subroutine &n at -e line 1.
```

Perl 5.22+ added `/n` to disable auto-numbered captures. Stryke parses
the trailing `n` as a bareword sub. CLI raises an undefined-sub error;
the library `eval` API silently returns the string `"n"`.

Tests: `regex_n_flag_silently_returns_n_in_lib_eval_today`.

Severity: **bug**. Workaround: turn captures into `(?:...)` non-capturing
groups manually.


## BUG-084 — Possessive quantifiers (`a++`, `\d++`) act like greedy `+`

```sh
$ stryke -e 'print "aaab" =~ /a++ab/ ? "Y" : "N"'
Y                               # should be N (no backtrack from a++)
$ perl   -e 'print "aaab" =~ /a++ab/ ? "Y" : "N"'
N
```

Stryke's regex engine treats `a++` identically to `a+` — backtracking
proceeds normally. Atomic groups (`(?>a+)`) work correctly (BUG-024
companion); only possessive-quantifier suffixes are missing.

Tests: `possessive_quantifier_does_not_prevent_backtrack_today`,
`greedy_a_plus_with_backtrack_matches`.

Severity: **bug** (regex parity).


## BUG-086 — `use constant { ... }` hashref form rejected; list form collapses

```sh
$ stryke -e 'use constant ARR => (1, 2, 3); my @a = ARR; print "@a"'
3                              # only last comma operand kept
$ perl   -e 'use constant ARR => (1, 2, 3); my @a = ARR; print "@a"'
1 2 3

$ stryke -e 'use constant { ZERO => 0, ONE => 1 }; print ZERO'
use constant: expected list of NAME => VALUE pairs at -e line 1.
```

Single-value `use constant NAME => VALUE` works. The hashref-block form
and the multi-value `(LIST)` form both fail. Workaround: declare each
constant separately, or wrap a list constant in an arrayref:
`use constant DAYS => [qw(mon tue wed)]`.

Tests: `use_constant_simple_scalar`, `use_constant_arithmetic`,
`use_constant_arrayref_holds_list`,
`use_constant_paren_list_collapses_to_last_today`,
`use_constant_hashref_form_is_rejected_today`,
`use_constant_qw_becomes_arrayref_string`.

Severity: **bug** (parity with the canonical Perl idioms).


## BUG-087 — `use warnings` does not emit warnings

```sh
$ stryke -e 'use warnings; my $x; my $y = $x + 1; print $y'
1                              # no warning
$ perl   -e 'use warnings; my $x; my $y = $x + 1; print $y'
Use of uninitialized value $x in addition (+) at -e line 1.
1
```

Stryke parses `use warnings` and `no warnings` without error but no
diagnostic ever fires. CLI flags `-w` and `-W` are also no-ops.

Tests: `use_warnings_silent_on_undef_arithmetic_today`,
`use_warnings_silent_on_string_in_numeric_today`,
`no_warnings_pragma_runs_without_error`,
`lib_eval_runs_undef_arith_without_warnings`.

Severity: **bug**. Many test harnesses rely on `use warnings FATAL =>
'all'` to surface latent bugs.


## BUG-088 — `(&@)` block prototype with trailing args drops the trailing args

```sh
$ stryke -e '
sub myff (&@) { my $cb = shift; print "after-shift count=", scalar @_ }
myff { 1 } 5, 7'
after-shift count=0           # trailing args were not passed
```

Stryke parses `myff { ... } 5, 7` as `myff({...}); 5; 7;` — three
top-level comma operands. Workaround: explicit-paren call form
`myff(sub { ... }, 5, 7)` does pass all args correctly.

Tests: `block_at_prototype_with_trailing_args_evaluates_trailing_as_statements_today`,
`block_prototype_passes_block_as_first_arg`.

Severity: **bug** (common idiom for `apply(\&block, list)` style APIs).


## BUG-093 — `intercept_remove(NAME, KIND)` does not actually remove advice

```sh
$ stryke -e '
fn payload { print "G;" }
before "payload" { print "B;" }
after  "payload" { print "A;" }
payload();
intercept_remove("payload", "before");
payload();              # B; still fires'
B;G;A; B;G;A;
```

`intercept_clear(NAME)` (which removes ALL advice for the named target)
DOES work; only the per-kind variant is broken.

Tests: `intercept_clear_removes_all_advice_for_target`,
`intercept_remove_does_not_remove_advice_today`,
`intercept_remove_unknown_kind_does_not_panic`.

Severity: **bug**.


## BUG-094 — Three-level `eval { die ... } / die $@` chain drops innermost log mutations

```sh
$ stryke -e '
my $log = "";
eval {
  eval {
    eval { die "in\n" };
    $log .= "L1:" . $@;             # this mutation is lost
    die $@;
  };
  $log .= "L2:" . $@;
  die $@;
};
$log .= "L3:" . $@;
print $log'
L2:in
L3:in
                                    # L1: never made it into $log
```

The L1 append happens *between* the innermost `eval` ending and the
re-`die`; somewhere in that window the lexical `$log`'s mutation is
dropped. Two-level chains preserve all writes correctly (the existing
`nested_eval_die_rethrow_preserves_message` test pins that).

Tests: `three_level_die_rethrow_drops_innermost_log_today`,
`nested_eval_die_rethrow_preserves_message` (the 2-level form that
works).

Severity: **bug**.


## ~~BUG-089~~ DESIGN-001 — Closures capture outer-scope vars by value, writes are a compile-time error

**Not a bug — intentional language-design choice, strictly enforced.**
Stryke closures snapshot outer-scope `my` variables at capture time
rather than holding a live reference to their storage. This matches
Rust's `move ||` closure semantics, trades shared-mutable state for
race-free dispatch into the parallel runtime (`pmap`, `pfor`,
`cluster`, async/spawn blocks), and removes an entire class of "is
this closure-mutating-outer-var safe across threads?" questions from
the language.

**Strict enforcement** (compile-time): writes to an outer-scope `my`
variable from inside any sub body (`sub { }` / `fn { }` /
`sub foo { }`) are rejected by the compiler with this diagnostic:

```
cannot modify outer-scope `my $count` from inside a closure —
stryke closures capture by value to keep parallel dispatch
race-free. Use `mysync $count` for shared mutable state, or
`--compat` for Perl 5 shared-storage semantics
```

The three opt-out paths:

| Path | Storage | Use case |
|------|---------|----------|
| `mysync $x` | atomic shared cell | counters, accumulators, factory state, observer registries |
| `our $x` / `$main::x` | package global | cross-module shared state (always shared, every mode) |
| `--compat` mode | Perl 5 shared-storage | porting Perl code unchanged |

Reads of outer-scope `my` are fine — you get the snapshot value at
capture time. Mutations through *aggregate references* are fine too
— `my $h = {}; my $f = sub { $h->{k} = 42 }` works because the
ref-identity (the Arc to the underlying hash) is preserved across
capture; only the scalar `$h` itself isn't shared.

`defer { ... }` is exempt — it runs synchronously at scope exit with
intentionally shared state. The check fires only on subs stored as
closure values.

What this means for common patterns:

- Factory with internal state (now requires `mysync`):
  ```
  fn make_counter { mysync $n = 0; sub { ++$n } }
  my $c = make_counter(); $c->(); $c->(); $c->();   # 3
  ```
- For-loop iteration captures each iteration's fresh `my $i` correctly
  (no `mysync` needed — read-only):
  ```
  my @fs; for my $i (1..3) { push @fs, sub { $i } }   # [1, 2, 3]
  ```
- `map { my $captured = $x; sub { $captured } } LIST` — explicit
  per-iteration `my` snapshot, read-only in the closure.

What requires an idiom change vs Perl:

- Outer counter: declare `mysync $n` (or use `--compat`):
  ```
  # Idiomatic stryke (parallel-safe atomic counter)
  mysync $n = 0;
  my $inc = sub { $n++ };

  # Perl-compat (shared storage)
  # stryke --compat -e 'my $n = 0; my $inc = sub { $n++ };'
  ```
- Observer pattern: pass a hash/array ref through the closure (ref
  identity preserved across the snapshot — only scalars are
  copied-by-value).

Tests pinning the documented behaviour:
`closure_captures_outer_var_by_value` (was `_does_not_see_outer_var_mutation_today`),
`closure_modifying_outer_scalar_stays_local` (was `_does_not_propagate_today`),
`closure_does_not_observe_outer_array_push` (was `_today`),
`closure_does_not_observe_outer_hash_extension` (was `_today`),
`fn_factory_returning_sub_captures_factory_param`,
`for_loop_closure_captures_each_iteration_var`,
`factory_with_internal_state_is_a_working_counter`,
`map_inside_closure_captures_unique_per_iteration`.

Status: **DESIGN** (not a bug). Documented behaviour, distinguishes
stryke from Perl 5, motivated by parallel-safety.


## BUG-097 — `print {$fh} ...` braces form does not honor the filehandle

```sh
$ stryke -e '
open my $fh, ">", "/tmp/out" or die;
print {$fh} "data\n";
close $fh;
print "file: ", -s "/tmp/out"'
CODE(__ANON__)file: 0          # the brace expression is evaluated and printed
```

Stryke parses `print {$fh} ...` as `print { ... }` where the braces
introduce a hashref-or-block context, not as the filehandle-disambiguator
form. Workaround: `print $fh "data\n"` (no braces) when `$fh` is a
simple scalar.

Tests: `print_braces_filehandle_form_does_not_write_to_handle_today`.

Severity: **bug**.


## BUG-092 — Ternary inside `"@{[ ... ]}"` interpolation rejected at parse time

```sh
$ stryke -e 'my $x = 5; my $s = "@{[ $x > 0 ? "pos" : "neg" ]}"; print $s'
Unterminated @{ ... } in double-quoted string at -e line 1.
```

Stryke's interpolation parser bails on the inner `?`/`:` pair. Workaround:
move the ternary out: `my $r = $x > 0 ? "pos" : "neg"; my $s = "...$r..."`.

Tests: `ternary_inside_interpolated_anon_array_is_rejected_today`,
`ternary_outside_interpolation_works`.

Severity: **bug** (parser).


## BUG-102 — `refaddr(\&fn)` differs between repeated evaluations

```sh
$ stryke -e 'sub myff { 1 }
            my $r1 = \&myff; my $r2 = \&myff;
            print refaddr($r1) == refaddr($r2) ? "eq" : "ne"'
ne
$ perl -MScalar::Util=refaddr -e '...'
eq
```

Same root issue as BUG-075 (refaddr of `\@a`): each `\&fn` creates a
fresh ref-cell rather than returning the underlying CV's address. Pure
copy via `=` keeps the same refaddr.

Tests: `refaddr_of_repeated_backslash_amp_returns_different_today`.

Severity: **bug** (parity).


## BUG-103 — `prototype($coderef)` empty for anonymous-sub refs

```sh
$ stryke -e 'my $r = sub ($) { 42 }; print prototype($r)'
                                # empty
$ stryke -e 'sub myff ($) { 42 } print prototype(\&myff)'
$
```

Named-sub coderefs report their prototype correctly. Anonymous-sub
coderefs return empty.

Tests: `prototype_of_anonymous_sub_coderef_is_empty_today`,
`prototype_of_named_sub_via_amp_ref_works`.

Severity: **bug** (small surface; a workaround is to assign the anon
sub to a typeglob with a name).


## BUG-104 — `print $x - $y, list` parses `$x` as an indirect filehandle

```sh
$ stryke -e 'my $x = 5; my $y = 3; print $x - $y, "end"'
print on unopened filehandle 5 at -e line 1.
$ stryke -e 'my $x = 5; my $y = 3; print $x + $y, "end"'
8end                              # `+` form works
$ stryke -e 'my $x = 5; my $y = 3; print(($x - $y), "end")'
2end                              # parens work
$ perl   -e 'my $x = 5; my $y = 3; print $x - $y, "end"'
2end                              # Perl handles it
```

The `-` form trips stryke's indirect-filehandle parser because `-`
also means unary minus. The `+` form is unambiguous. Workaround: wrap
the expression in parens, or store the result in a temporary first.

Tests: `print_scalar_minus_scalar_with_trailing_args_parses_as_filehandle_today`,
`print_scalar_plus_scalar_with_trailing_args_works`,
`print_paren_workaround_for_minus_form_works`.

Severity: **bug** (parser ambiguity).


## BUG-106 — `to_json($data, $opts_hashref)` serializes both args as an array

```sh
$ stryke -e 'print to_json({a=>1, b=>2}, {pretty => 1})'
[{"a":1,"b":2},{"pretty":1}]
$ perl -MJSON::PP -e 'print JSON::PP->new->pretty->encode({a=>1, b=>2})'
{
   "a" : 1,
   "b" : 2
}
```

Stryke's `to_json` does not recognize a second-argument options hashref
— both args are flattened into a top-level JSON array. Workaround: use
`to_yaml` for human-readable output (which works), or implement
pretty-printing manually.

Tests: `to_json_two_arg_pretty_form_serializes_as_array_today`.

Severity: **bug** (low impact; rarely needed for machine-read JSON).


## PARITY-040 — Scalar-context `..` flip-flop operator is unimplemented

The classic `print if N..M` line-range flip-flop produces no output
in stryke; Perl emits the lines whose `$.`-counter falls in the
specified range. This breaks the canonical `awk '/start/,/end/'`
translation idiom that motivates flip-flops in the first place.

```sh
$ s -e 'for (1..10) { print "$_," if 3..5 } print "\n"'

$ perl -e 'for (1..10) { print "$_," if 3..5 } print "\n"'
3,4,5,
```

The list-context `..` works correctly (range expansion); only the
scalar-context flip-flop / flip-flap (`...`) variants are missing.
A full fix needs hidden per-occurrence state, the `E0`/`E1` edge-
counter Perl exposes, and the tri-dot non-eager variant.

Pinning test:
`flip_flop_scalar_context_does_not_match_perl_lines`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity**.


## PARITY-041 — Arrayref/hashref in numeric context returns 0, not the heap address

```sh
$ s -e 'my $r = [1,2,3]; print "num=", $r + 0, "\n"'
num=0
$ perl -e 'my $r = [1,2,3]; print "num=", $r + 0, "\n"'
num=4354497000
```

Perl exposes the heap address of a ref when it's used in numeric
context. Scripts that test `if ($ref + 0)` for definedness, or
compare two refs with `==` (numeric ref-equality), break under
stryke. Stringification of refs (`"$ref"`) still produces the
expected `ARRAY(0x...)` / `HASH(0x...)` text.

Pinning test:
`arrayref_in_numeric_context_returns_zero_not_address`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity**.


## PARITY-042 — `chr(N)` for N > 0x10FFFF or N < 0 returns the empty string

```sh
$ s -e 'my $c = chr(0x110000); print length($c), "\n"'
0
$ s -e 'my $c = chr(-1); print length($c), "\n"'
0
$ perl -e 'my $c = chr(0x110000); print length($c), "\n"'   # warns + emits
1
```

Stryke clamps to the valid Unicode range; Perl warns but still emits
a (potentially malformed) character up to chr <= 0x7FFFFFFF. The
stryke behavior is intentionally stricter for UTF-8 hygiene; pinning
both edge cases so a future change is deliberate.

Pinning tests:
`chr_above_max_unicode_returns_empty_string`,
`chr_negative_returns_empty_string`,
`chr_max_valid_unicode_works`
in `tests/suite/behavior_pin_2026_05_at.rs`.

Severity: **parity** (intentional-strictness).


## BUG-205 — `preduce_init INIT, { BLOCK } LIST` returns the init unchanged

```sh
$ s -e 'my $r = preduce_init 100, { _0 + _1 } (1, 2, 3, 4); print "r=$r\n"'
r=0
$ s -e 'my $r = preduce { _0 + _1 } 100, (1, 2, 3, 4); print "r=$r\n"'
r=110
```

The `preduce_init INIT, { BLOCK } LIST` argument-order form silently
returns 0 instead of folding the list into the init accumulator. The
working form is the regular `preduce { BLOCK } INIT, LIST` (init second).

Discovered while writing the parallel-primitives pin file
(`tests/suite/parallel_primitives_pin.rs`). The pin tests the working
form. A future fix should make `preduce_init` route to the same fold;
the current behavior is wrong on its face.

Severity: **bug**.


## BUG-206 — `from_yaml(to_yaml(...))` flattens 3+ level nested hashrefs

```sh
$ s -e '
    my $d = +{ a => +{ b => +{ c => +{ d => 1 } } } };
    my $back = from_yaml(to_yaml($d));
    print defined($back->{a}->{b}->{c}->{d}) ? "ok" : "missing", "\n"
'
missing
```

YAML round-trip of hash-of-hash-of-hash at depth ≥ 3 loses leaf values
on the deeper paths. Two-level nesting (`{a => {b => 1}}`) round-trips
fine. The pin file `tests/suite/codec_roundtrip_pin.rs` documents the
working depth-2 case; this entry tracks the deeper case as a known gap.

Root cause not yet diagnosed — likely either in the YAML emitter
(missing block-scalar indentation past two levels) or in the parser
(eager flatten on nested mapping). JSON/TOML do not exhibit this.

Severity: **bug**.


## BUG-208 — `box_blur_kernel(N)` returns a flat Array, not an ArrayRef

```sh
$ s -e '
    my $k = box_blur_kernel(3);
    print "ref=", ref($k) // "(none)", " len=", len($k), "\n";
    # Works: arrow-indexed access.
    print "k[0]=", $k->[0], "\n";
    # Fails: array-deref.
    my @rows = @$k;
    print "rows=", scalar(@rows), "\n"
'
ref= len=7
k[0]=0.111111
rows=1
```

`box_blur_kernel(3)` returns an Array value (length 7, all `1/9` ≈ 0.111)
rather than a 3×3 ArrayRef of rows. The arrow-index form works because
the Array is auto-indexed, but `@$k` dereferences as a 1-element wrap,
producing wrong-shape output for any caller expecting an MxN matrix.

The companion math kernels (`pauli_x`, `lu_decompose`, etc.) return
proper ArrayRef-of-ArrayRef matrices. Discovered while pinning these
shapes in `tests/suite/len_semantics_pin.rs`.

Severity: **bug**.


## BUG-209 — Pipe-forward into `>{ BLOCK }` passes the value as `$_`, never as `@_`

```sh
$ s -e '
    my @arr = (1, 2, 3, 4, 5);
    my @r = @arr |> >{ join(",", @_) };
    print "r=@r\n"
'
r=
$ s -e '
    my @arr = (1, 2, 3, 4, 5);
    my $r = @arr |> >{ join(",", @$_) };
    print "r=$r\n"
'
r=5,4,3,2,1
```

Wait, the second form prints reversed — that's the existing array-ref
binding from a prior stage. Setting that aside, the central observation:
the `>{ ... }` IIFE stage in a pipe-forward chain receives the LHS as
`$_` (a single scalar that's either the original value or an arrayref
if the LHS was an array), *not* as `@_`. So patterns like

```stryke
my @top5 = (1..100) |> sort { _1 <=> _0 } |> >{ @_[0:4] };
```

silently produce an empty `@_` and an empty result.

The pin for `pipe_iife_stage_receives_lhs_as_underscore` in
`tests/suite/pipe_forward_pin.rs` documents the working `$_` form.
Demos affected during round-6 work: `examples/stream_merge.stk` was
rewritten to materialize an intermediate `@desc_uniq` array and slice
explicitly, sidestepping the `>{ ... }` stage.

A future fix should bind `@_` to the LHS items when the LHS is a list,
matching Perl-block expectations. Today, this is a quiet sharp edge that
costs every new user a debug session.

Severity: **bug**.


## BUG-210 — `return` inside `eval { ... }` returns from the eval, not from the enclosing sub

```sh
$ s -e 'sub g { eval { return 42 }; 99 } print g(), "\n"'
99
$ perl -e 'sub g { eval { return 42 }; 99 } print g(), "\n"'
42
```

Perl's `return` inside `eval { BLOCK }` unwinds the call frame
through the eval back to the enclosing sub, so `g()` should return
42. Stryke treats the eval block as a regular block and `return`
exits only the eval, letting the enclosing sub fall through to the
trailing `99`.

Affects any code pattern of the form:

```perl
sub fetch_or_die {
    eval { return cache_get($key) if defined cache_get($key) };
    return compute();
}
```

— in stryke, the eval-block return is silently lost and `compute()`
always runs even on a cache hit.

Pinning test: `return_inside_eval_returns_from_eval_not_enclosing_sub`
in `tests/suite/error_handling_pin.rs`.

Severity: **bug** (parity gap vs Perl 5 semantics).


## BUG-211 — `"42 at FILE line N." + 0` numerifies to `1`, not `42`

```sh
$ s -e 'eval { die 42 }; print $@ + 0, "\n"'
1
$ perl -e 'eval { die 42 }; print $@ + 0, "\n"'
42
```

Perl's numeric-context coercion of a string consumes the leading
numeric prefix (`"42 at -e line 1.\n"` → `42`). Stryke's coercion
returns `1` — apparently treating the whole non-numeric tail as
significant and degrading the result to a boolean-style 1.

This breaks the common Perl idiom of `if ($@ == ERRNO)` for error-code
dispatch when the die payload is an integer.

Pinning test: indirect via `die_with_integer_payload_stringified_with_location_suffix`
in `tests/suite/error_handling_pin.rs` (pins the string-prefix shape;
numeric coercion gap tracked here for future fix).

Severity: **bug** (parity gap; affects classic Perl error-dispatch).


## BUG-212 — AOP `around` advice does not fire when target is invoked inside `eval { ... }`

```sh
$ s -e '
    fn foo($x) { $x * 2 }
    fn caller_fn($x) { eval { foo($x) } }
    mysync $count = 0;
    around "foo" { $count++; proceed(@INTERCEPT_ARGS) }
    caller_fn(1); caller_fn(2); caller_fn(3);
    print "count=$count\n"
'
count=0

$ s -e '
    fn foo($x) { $x * 2 }
    mysync $count = 0;
    around "foo" { $count++; proceed(@INTERCEPT_ARGS) }
    foo(1); foo(2); foo(3);
    print "count=$count\n"
'
count=3
```

When the AOP-wrapped function is invoked directly, the `around` body
fires correctly. When the same function is invoked from inside an
`eval { BLOCK }` (anywhere in the call chain — directly in the eval, or
inside another function called from within the eval), the AOP dispatch
is bypassed entirely. Counter stays at zero. The function body still
runs, but observers, latency tracking, and retry counters all silently
disappear.

This is load-bearing: any defensive code path using `eval` to swallow
expected exceptions silently loses every form of instrumentation
attached via `around` / `before` / `after`. Worked around in
`examples/job_queue.stk` by returning `+{ ok => 0, error => ... }`
hashrefs from the worker instead of `die`-ing, so the caller never
needs to `eval`.

Root cause likely: the `eval` block lowering installs its own call
frame that doesn't route through the AOP dispatch table — the VM jumps
straight to the bytecode for the called function.

Severity: **bug** (silent observability hole; should be a P1 fix).

**Update (round-11):** A second manifestation of the same root cause —
AOP `around` advice on a recursive function fires only for the outermost
invocation, never for self-recursive sub-calls. Discovered in
`examples/expression_parser.stk`: the AOP-wrapped `eval_ast` counter
reads `12` (one per top-level evaluation) when the actual recursive
call count is several times higher. Either internal call-site bytecode
short-circuits the AOP dispatch table, or AOP intentionally suppresses
re-entrancy. Either way the surface is wrong for observability use
cases.


## BUG-214 — `$\`` and `$'` (pre-match / post-match) variables not supported

```sh
$ s -e '"abc123def" =~ /(\d+)/; print "pre=[", $`, "] post=[", $'\'', "]\n"'
Expected variable name after $ at -e line 1.

$ perl -e '"abc123def" =~ /(\d+)/; print "pre=[$`] post=[$\047]\n"'
pre=[abc] post=[def]
```

Stryke parser rejects `$\`` (pre-match) and `$'` (post-match) variables
outright. Scripts that use these idiomatic Perl regex helpers must
derive pre/post manually from `$-[0]` / `$+[0]` offsets (also not
verified to be supported).

Workaround: use `(?:before)(target)(?:after)` capture groups instead.

Severity: **bug** (parity gap).


## BUG-215 — `$+{name}` named-backref interpolation broken in s/// replacement

```sh
$ s -e 'my $s = "alice=30"; $s =~ s/(?<k>\w+)=(?<v>\d+)/$+{v} -> $+{k}/; print "$s\n"'
$+{v} -> $+{k}

$ perl -e 'my $s = "alice=30"; $s =~ s/(?<k>\w+)=(?<v>\d+)/$+{v} -> $+{k}/; print "$s\n"'
30 -> alice
```

Inside an `s///` replacement string, `$+{name}` is not interpolated and
appears verbatim in the output. The numeric form `$1`, `$2` does work,
so this is specifically about hash-syntax interpolation in replacement
context.

Workaround: use numbered backrefs `$1`, `$2`, ... even with named-group
patterns. Pin: `substitution_with_named_backref_via_numeric_form` in
`tests/suite/regex_capture_pin.rs`.

Severity: **bug** (parity gap).


## BUG-216 — No autovivification on deep-write or `push`

```sh
$ s -e 'my %h; $h{a}{b}{c} = "x"; print $h{a}{b}{c}, "\n"'
Can't use arrow deref on non-hash-ref at -e line 1.

$ s -e 'my $r = +{}; push @{$r->{list}}, "first"; print scalar(@{$r->{list}}), "\n"'
push argument is not an ARRAY reference at -e line 1.

$ perl -e 'my %h; $h{a}{b}{c} = "x"; print $h{a}{b}{c}, "\n"'
x
$ perl -e 'my $r = +{}; push @{$r->{list}}, "first"; print scalar(@{$r->{list}}), "\n"'
1
```

Perl autovivification is the language feature that makes `$h{a}{b}{c} = X`
silently create the chain of intermediate hashes. Stryke does NOT
autoviv — every level must be created explicitly:

```stryke
my %h;
$h{a}    = +{};
$h{a}{b} = +{};
$h{a}{b}{c} = "x";

my $r = +{};
$r->{list} = [];
push @{$r->{list}}, "first";
```

Affects every Perl idiom that incrementally builds nested structures
(grouping hashes, parser AST construction, recursive descent state).

Pin: `autoviv_requires_explicit_intermediate_construction` and
`autoviv_requires_explicit_arrayref_before_push` in
`tests/suite/hashref_deep_pin.rs`.

Severity: **bug** (large parity gap; major Perl idiom blocker).


## BUG-218 — Regex with interpolated variable `/^$re$/` caches result across calls in a loop

```sh
$ cat > /tmp/probe.stk <<'EOF'
fn pm($pat, $topic) {
    my $re = $pat;
    $re =~ s/\./\\./g;
    $re =~ s/\*/[^.]+/g;
    my $r = $topic =~ /^$re$/ ? 1 : 0;
    printf "pat=[%s] re=[%s] r=%d\n", $pat, $re, $r;
    return $r;
}
my $topic = "user.created";
for my $pat ("user.*", "order.placed", "order.*") {
    pm($pat, $topic);
}
EOF
$ s --no-interop /tmp/probe.stk
pat=[user.*] re=[user\.[^.]+] r=1
pat=[order.placed] re=[order\.placed] r=1     # WRONG, should be 0
pat=[order.*] re=[order\.[^.]+] r=1            # WRONG, should be 0
```

When a regex is built via variable interpolation (`/^$re$/` or
`qr/^$re$/`) inside a function called in a loop, the **result of the
first match is reused for every subsequent call**, regardless of the
new variable value. Reversing the call order flips the bug to
"first call returns 0 → all return 0".

The same regex form works correctly in isolation (single call) and in
direct testing outside the function. The bug surfaces only when the
function is called repeatedly with different variable values.

Most likely root cause: the regex literal `/^$re$/` is compiled once
at first execution and the compiled pattern is cached per call-site
program-counter, not re-compiled per dynamic value of `$re`.

Affects: pattern-matching dispatch tables, glob-style routing,
templated query builders, anything that varies a regex per iteration.
Workaround in `examples/event_dispatcher.stk`: use the `glob_match`
builtin instead of hand-rolled regex.

Severity: **bug** (P1; regex correctness; silent wrong-result hazard).


## BUG-219 — AOP advice body rejects multi-line `+{...}` hashref literals + multi-statement `if` modifiers

When writing an AOP `around`/`before`/`after` advice body, certain
constructs that work elsewhere in stryke trigger:

```
AOP around advice body for `NAME` could not be lowered to bytecode
(likely contains a construct unsupported by block lowering)
```

Reproducible patterns that hit the lowering wall:

1. **Multi-line hashref literal inside an advice statement**:

```stryke
around "foo" {
    push @$log, +{
        from => $a,
        event => $b,
    }
    proceed()
}
```

Workaround: build the hashref on one line, store in a local, then push.

```stryke
around "foo" {
    my $entry = +{ from => $a, event => $b }
    push @$log, $entry
    proceed()
}
```

2. **`$hash{key}++ if cond` postfix-modifier increment**:

```stryke
around "foo" {
    $count{$bucket}++ if defined $bucket
    proceed()
}
```

Workaround: lift to a full `if`-block with explicit `+= 1`.

3. **Literal `return` in advice body** (previously documented; same lowering pass):

The common thread is that the AOP lowering pass only handles a subset
of block-statement shapes. Real advice bodies often need this stuff,
so workarounds compound demo verbosity.

Discovered via `examples/state_machine.stk` and `examples/graph_bfs.stk`
during round-8/round-10 demo work.

Severity: **bug** (developer-experience friction; correctness if a
user assumes the advice fired when it didn't compile in).


## BUG-220 — `scalar(N:M)` of a colon-range returns the empty string

```sh
$ s -e 'my $n = scalar(1:100); print "n=[", $n, "]\n"'
n=[]

$ s -e 'my @r = (1:100); my $n = scalar(@r); print "len=$n\n"'
len=100

$ s -e 'my $n = len(1:100); print "n=$n\n"'
n=100
```

`scalar(N:M)` on a colon-range expression does not materialize the
range and returns an empty string instead of the element count. Two
workarounds work:

- `len(N:M)` — the stryke-idiomatic length.
- `my @arr = (N:M); my $n = scalar(@arr);` — materialise first.

Affects any code that tries `scalar(0:$n-1)` to derive an iteration
count without copying.

Pinning test: `range_via_len_returns_element_count` in
`tests/suite/range_iteration_pin.rs` (pins the working `len` form).

Severity: **bug** (minor parity gap; easy workaround).


## BUG-222 — AOP `around "Pkg::method"` advice does not fire on `$obj->method()` calls

```sh
$ cat > /tmp/probe.stk <<'EOF'
class Foo {
    n: Int = 0
    fn bump { $self->n($self->n + 1) }
}
mysync $count = 0
around "Foo::bump" {
    $count = $count + 1
    proceed(@INTERCEPT_ARGS)
}
my $f = Foo()
$f->bump
$f->bump
$f->bump
print "count=$count f->n=", $f->n, "\n"
EOF
$ s --no-interop /tmp/probe.stk
count=0 f->n=3
```

The method body runs (`f->n` becomes 3 after 3 bumps), but the `around`
advice registered against `"Foo::bump"` never increments `$count`.
AOP dispatch is bypassed entirely for method-call syntax.

Same root cause as BUG-212: AOP advice fires for direct symbol-table
calls but is skipped for any invocation path that doesn't route through
the AOP dispatch table — `eval { fn() }`, recursive self-calls,
**and now `$obj->method()` method calls**.

Affects every observability use case where the wrapped target is an
OOP method: hit-rate trackers on cache classes, latency tracing on
service classes, audit logs on persistence layers.

Worked around in `examples/lru_cache.stk` (the per-op t-digest reports
`NaN` because no samples ever flowed through the advice).

Workaround: wrap a free function that calls into the method, register
AOP on the free function. Verbose; defeats the purpose of `around`.

Severity: **bug** (P1 alongside BUG-212; together they make AOP
unreliable for observability on real codebases).


## BUG-223 — `zip(@a, @b)` pads to longer side instead of truncating to shorter

```sh
$ s -e 'my @r = zip([1, 2, 3, 4, 5], ["a", "b"]); print "n=", scalar(@r), "\n"; for my $p (@r) { print "  [", $p->[0], ",", $p->[1], "]\n" }'
n=5
  [1,a]
  [2,b]
  [3,]
  [4,]
  [5,]
```

Perl / `List::MoreUtils::zip` returns rows up to the shorter array's
length. Stryke pads the shorter side with empty values and continues
to the longer side, producing rows with empty-string second fields.

Affects any code that relies on `zip` to act as a "stop at shorter"
truncating iterator (the standard pairing semantic).

Pin: `zip_arrays_of_unequal_length_pads_to_longer` in
`tests/suite/iterators_pin.rs`.

Severity: **bug** (parity gap; affects iterator pipelines).


## BUG-224 — `chunk(N, LIST)` returns a single-element arrayref wrapping `N`

```sh
$ s -e 'my @g = chunk(3, 1, 2, 3, 4, 5, 6, 7, 8, 9); print scalar(@g), "\n"; for my $c (@g) { print "[", join(",", @$c), "]\n" }'
1
[1]

$ s -e 'my @g = chunked((1, 2, 3, 4, 5, 6, 7, 8, 9), 3); print scalar(@g), "\n"; for my $c (@g) { print "[", join(",", @$c), "]\n" }'
3
[1,2,3]
[4,5,6]
[7,8,9]
```

`chunk(N, LIST)` returns `[[N]]` (a single arrayref containing N as the
sole element) instead of the expected N-sized groups. Same for
`chunk_n(LIST, N)` and `ai_chunk([...], N)` — all return wrong shape.

The `chunked((...), N)` form (parens around the LIST, N second) works
correctly. Use that form in fresh code.

Pin: `chunked_3_splits_into_groups_of_three` in
`tests/suite/iterators_pin.rs`.

Severity: **bug** (BUG-058 marked some chunk variants — this is the
remaining set).


## BUG-226 — `mysync $x = t_digest(N)` mid-script silently corrupts the sketch type tag

```stryke
# Top of file:
mysync $hll = hll(14)
mysync $tk  = topk(3)

# After some code runs:
mysync $global_lat = t_digest(100)
td_add($global_lat, 42)   # → "td_add: expected TDigestSketch operand"
```

When `mysync $x = t_digest(N)` is declared **after** other `mysync`
declarations + intervening code, subsequent `td_add($x, ...)` errors
out with "expected TDigestSketch operand". Switching to plain `my $x`
declaration works; declaring all `mysync` sketches at the top of the
script also works.

Manifested in `examples/json_lines_log.stk` (round 13) — mid-script
`mysync` of a t-digest after parsing log records corrupted the type.
Workaround applied: use `my` for sketches that don't need cross-closure
write-back.

Severity: **bug** (silent type-tag corruption; surface is non-obvious).


## BUG-227 — `mysync $count = $count + 1` inside `pfor` races (lost updates)

```sh
$ s -e 'mysync $count = 0; pfor { $count = $count + 1 } (1:100); print "count=$count\n"'
count=75
```

`mysync` provides shared visibility across closure boundaries but does
NOT make read-modify-write atomic. Under `pfor` workload, observed
final counter values consistently fall below the iteration count due
to lost updates (worker reads `$count`, increments, writes back —
between read and write another worker has the same stale value).

In `examples/job_queue.stk` and other earlier demos the workaround was
to switch to sequential `map`; round-15's concurrency_pin file pins
the buggy observed behavior (`$count <= iteration_count` rather than
`$count == iteration_count`) so a future atomic-increment fix is a
deliberate decision.

Affects: counters, rate limiters, hit/miss counters inside `pfor`,
anything that does `$x = $x + delta` from worker code.

Sketch operations (hll_add, td_add, topk_add, bloom_add) appear to
use internal atomic state and survive `pfor` reasonably well, though
with reduced counts under contention.

Pin: `pfor_counter_increment_races_under_contention` in
`tests/suite/concurrency_pin.rs`.

Severity: **bug** (correctness; should be a P1 fix — race-free
counters are table stakes for any parallel framework).


## BUG-228 — `my ($a, $b) = each %h` in expression context unsupported

```sh
$ s -e 'my %h = (a => 1, b => 2); while (my ($k, $v) = each %h) { print "$k=$v\n" }'
VM compile error (unsupported): my/our/state/local in expression context with multiple or non-scalar decls
```

Stryke's VM rejects multi-variable `my` declarations in expression
context, even though this is a core Perl idiom for hash iteration with
`each` and the canonical pattern for "while loop over hash". Single-
variable `my $x = ...` works fine.

Workaround: declare separately, or rewrite using `for my $k (keys %h)`.
The for-keys form is more idiomatic stryke regardless.

Pin: `while_each_via_separate_my_declarations` in
`tests/suite/hashref_iteration_pin.rs`.

Severity: **bug** (parity gap; affects the `each` idiom).


## BUG-229 — `around` advice without `proceed()` still runs the function body

```sh
$ s -e '
    fn foo() { die "body_ran\n" }
    around "foo" { "swallowed" }
    my $r = eval { foo() };
    print "r=[$r] err=[$@]\n"
'
r=[] err=[body_ran
]
```

In standard AOP semantics, `around` advice can choose to skip
`proceed()` entirely, replacing the wrapped call. In stryke, the
function body runs regardless of whether the advice body calls
`proceed()` or not. `around` is effectively a `before` + `after`
shorthand rather than a true around.

Affects: pre-conditions, caching wrappers (return cached value
without calling underlying), feature flags (suppress real call when
disabled). All require an explicit `return` from the advice — which
also doesn't work per BUG-210.

Pin: `around_advice_does_NOT_block_body_when_proceed_omitted` in
`tests/suite/aop_composition_extra_pin.rs`.

Severity: **bug** (semantic divergence from canonical AOP).


## BUG-230 — Multiple `around` registrations on same target: only first fires

```sh
$ s -e '
    fn f($x) { $x + 1 }
    mysync $outer = 0;
    mysync $inner = 0;
    around "f" { $outer = $outer + 1; proceed(@INTERCEPT_ARGS) }
    around "f" { $inner = $inner + 1; proceed(@INTERCEPT_ARGS) }
    f(10); f(20);
    print "outer=$outer inner=$inner\n"
'
outer=2 inner=0
```

Registering a second `around` for the same target is silently ignored.
The first registered advice fires for every call; the second never
fires. Same root cause as BUG-069 (multiple around does not compose),
but pinned with explicit call counts for clarity.

Affects: layered AOP usage like a logger + a metrics tracer on the
same fn. Workaround: combine both concerns into a single around block.

Pin: `multiple_around_only_first_registered_fires` in
`tests/suite/aop_composition_extra_pin.rs`.

Severity: **bug** (silent drop; composability gap).


## BUG-232 — `count { BLOCK } LIST` returns first matched element value, not the count

```sh
$ s -e 'my $n = count { _ > 0 } (1, 2, -1, 3, 4); print $n, "\n"'
1
$ s -e 'my $n = scalar(grep { _ > 0 } (1, 2, -1, 3, 4)); print $n, "\n"'
4
```

The `count { BLOCK } LIST` builtin is documented to return the number
of list items for which BLOCK returns true, but actually returns the
*value* of the first item that matched (`1` in this example, because
`1 > 0` is true).

Workaround: use the Perl idiom `scalar(grep { BLOCK } LIST)` which
returns the correct count. Pin:
`count_via_scalar_grep_idiom` in `tests/suite/list_builtins_pin.rs`.

Severity: **bug** (silent wrong-result; affects rollups and percent-
match patterns).


## BUG-233 — Bare `{ ... }` block with `my` clobbers outer scope variable to undef

```sh
$ s -e '
    my $x = 10;
    my $r;
    {
        my $x = 20;
        $r = $x;
    }
    print "r=$r x=", defined($x) ? $x : "(undef)", "\n"
'
r=20 x=(undef)
```

In Perl, an inner `my $x` inside a `{...}` block creates a fresh local
binding that shadows the outer `$x`; after the block, the outer `$x`
returns to its original value (10 here). Stryke's behavior leaves the
outer `$x` as undef after the block exits — the inner declaration
appears to bind to the outer slot rather than create a fresh inner.

Affects any pattern that uses bare blocks for temporary scoping (a
common Perl idiom for `local`-like behavior, helper-table init, or
RAII-style cleanup). The fix likely lives in the block-lowering pass.

Pin: `my_in_inner_block_shadow_value_seen_inside_only` in
`tests/suite/scope_pin.rs`.

Severity: **bug** (Perl-parity gap; affects idiomatic block-scoping).


## BUG-234 — `\$` literal in `s/// replacement` is silently dropped

```sh
$ s -e '
    my $s = "price 50";
    $s =~ s/price/\$/;
    print "[$s]\n"
'
[ 50]

$ perl -e '
    my $s = "price 50";
    $s =~ s/price/\$/;
    print "[$s]\n"
'
[$ 50]
```

`\$` inside a s/// replacement string is intended to emit a literal
dollar character (the alternative to interpolating `$var`). Stryke
drops it silently — the literal `$` is replaced by an empty string,
giving `[ 50]` instead of `[$ 50]`.

Workaround: insert via `chr(36)` and concat into the replacement
variable form:

```stryke
my $d = chr(36);
$s =~ s/price/$d/;
```

Note: as an additional gotcha, the *expected output literal* `"$ 50"`
interpolates `$ ` as a special variable (empty string), so the
comparison string also needs `chr(36) . " 50"`.

Pin: `s_replacement_dollar_literal_via_chr` in
`tests/suite/regex_substitution_pin.rs`.

Severity: **bug** (parity gap; silent corrupted output for any
dollar-aware text — prices, shell-script generation, regex docs).


## BUG-236 — `delete @h{LIST}` slice form rejected with "delete requires hash or array element"

```sh
$ s -e '
    my %h = (a => 1, b => 2, c => 3);
    delete @h{qw(a c)}
'
delete requires hash or array element at -e line 3.

$ perl -e '
    my %h = (a => 1, b => 2, c => 3);
    delete @h{qw(a c)};
    print join(",", sort keys %h), "\n"
'
b
```

The slice form `delete @h{LIST}` for batch-removing multiple keys is
explicitly rejected at runtime. Only single-key `delete $h{K}` is
accepted.

Workaround: loop over the key list and delete each key individually:

```stryke
for my $k (qw(a c)) {
    delete $h{$k};
}
```

Pin: `delete_per_key_workaround_for_batch_delete` in
`tests/suite/hash_slice_pin.rs`.

Severity: **bug** (parity gap; affects bulk cleanup patterns).


## BUG-237 — `split /(?<=...)\s/` ignores lookbehind, splits on every whitespace

```sh
$ s -e '
    my $s = "Hi. How are you? I am fine.";
    my @parts = split /(?<=[.!?])\s/, $s;
    print "n=", scalar(@parts), "\n";
    for my $p (@parts) { print "  [$p]\n" }
'
n=7
  [Hi.]
  [How]
  [are]
  [you?]
  [I]
  [am]
  [fine.]

$ perl -e '
    my $s = "Hi. How are you? I am fine.";
    my @parts = split /(?<=[.!?])\s/, $s;
    print scalar(@parts), "\n"
'
3
```

The lookbehind assertion `(?<=[.!?])` in a `split` pattern is silently
ignored — the regex splits on every whitespace regardless. In Perl,
the same form correctly splits only on whitespace that follows a
sentence-ending punctuation character (so it preserves the punctuation
with its sentence).

Direct `=~ /(?<=...)X/` matches DO honor lookbehind correctly (per
`regex_lookaround_pin.rs` other tests); the issue is specifically in
the split-pattern compilation path.

Workaround: split on the punctuation directly via `[.!?]+` and post-
filter empty matches.

Pin: `split_lookbehind_does_not_constrain_correctly` in
`tests/suite/regex_lookaround_pin.rs`.

Severity: **bug** (parity gap; affects sentence-splitting and any
boundary-preserving tokenization).


## BUG-238 — `when ($_ < N)` arithmetic clause smart-matches instead of boolean-evaluating

```sh
$ s -e '
    my $x = 50;
    given ($x) {
        when ($_ < 10)  { print "low\n" }
        when ($_ < 100) { print "mid\n" }
        default         { print "high\n" }
    }
'
high
```

In Perl 5.10+, `when ($_ < 100)` is treated as a boolean expression
(distinct from value smart-match) and the clause fires when the
expression is truthy. Stryke smart-matches the value of `$_ < 100`
(which is `1` for true) against `$_` (which is 50), getting no match
— so neither `when` clause fires and `default` always wins.

Affects every range-style dispatch idiom. Workaround: use literal-
value clauses only, and fall back to `if/elsif` inside `default` for
threshold checks.

Pin: `given_when_arithmetic_clause_falls_through_to_default` in
`tests/suite/given_when_pin.rs`.

Severity: **bug** (parity gap; affects every value-bucketing pattern).


## BUG-239 — `return` inside `given/when` block errors at compile-time

```sh
$ s -e '
    fn ca($cmd) {
        given ($cmd) {
            when ("start") { return "starting" }
            default        { return "unknown" }
        }
    }
    print ca("start"), "\n"
'
unexpected control flow in tree-assisted opcode at -e line 3.
```

`return` from inside a `given/when` body fails to lower in the
tree-assisted opcode pass. Stryke compile-time rejects the program.

Affects: any state-machine / classifier function that wants to
return early per branch. Forces the user to assign each branch's
result to a local variable and return it after the block exits.

Workaround:

```stryke
fn classify($n) {
    my $r;
    given ($n) {
        when (0) { $r = "zero" }
        default  { $r = "other" }
    }
    return $r
}
```

Pin: `given_when_threshold_via_local_variable` in
`tests/suite/given_when_pin.rs`.

Severity: **bug** (parity gap + workaround friction).


## BUG-240 — CSV `from_csv` does not unescape doubled-double-quote `""` in quoted fields

```sh
$ s -e '
    my $csv = qq{name,quip\n"bob","he said ""hi"""\n};
    my $back = from_csv($csv);
    print "quip=[", $back->[0]->{quip}, "]\n"
'
quip=[he said ""hi]

$ perl -MText::CSV -e '
    use Text::CSV;
    my $csv = Text::CSV->new();
    open(my $fh, "<", \qq{name,quip\n"bob","he said ""hi"""\n});
    $csv->getline($fh);   # header
    my $row = $csv->getline($fh);
    print "quip=[", $row->[1], "]\n"
'
quip=[he said "hi"]
```

The CSV standard (RFC 4180) and Perl's `Text::CSV` both unescape `""`
inside a quoted field to a literal `"`. Stryke's `from_csv` does not
perform this unescaping — the result contains the raw double-quotes
and loses the closing pair.

Affects: any CSV containing quoted text fields with embedded quotes
(common for product descriptions, error messages, JSON-in-CSV
embeddings). Workaround: post-process the parsed values with a
`s/""/"/g` substitution.

Pin: `from_csv_escaped_quote_partial_unescape` in
`tests/suite/csv_codec_pin.rs`.

Severity: **bug** (correctness; affects data import from spreadsheets).


## BUG-243 — Heredoc not accepted as function argument or in ternary

```sh
$ s -e 'fn echo($s) { $s } print echo(<<END)
hello
END'
... parse error: Expected RParen, got Ident ...
```

Stryke's parser only accepts heredoc bodies in *statement* contexts —
assignment, top-level expression. Passing `<<TAG` directly as a
function argument or as a ternary branch fails parsing.

Workaround: assign to a `my` variable first, then pass.

```stryke
my $body = <<END;
hello
END
print echo($body);
```

Pin: `heredoc_in_var_then_passed_to_fn`,
`heredoc_in_ternary_via_temp_var` in `tests/suite/heredoc_pin.rs`.

Severity: **polish** (workaround is one extra line; no semantic loss).


## BUG-244 — `mysync` inside `fn` body reinitialises on each call

```sh
$ s -e '
fn counter() {
    mysync $n = 0;
    $n = $n + 1;
    return $n
}
print counter(), " ", counter(), " ", counter(), "\n"'
1 1 1
```

`mysync` was intended as cross-closure shared state; inside a top-level
fn body it does not act as a "static" variable that persists across
calls — each invocation reinitialises `$n` to `0`. The closest stryke
idiom for static-like persistence is the closure-factory pattern:

```stryke
my $counter = do {
    my $n = 0;
    sub { $n = $n + 1 }
};
print $counter->(), " ", $counter->(), " ", $counter->(), "\n";
# 1 2 3
```

Pin: `mysync_inside_fn_reinit_per_call_not_static` in
`tests/suite/local_scope_pin.rs`.

Severity: **polish** (clear closure-factory workaround; design decision
on whether `mysync` should imply per-fn persistence is open).


## BUG-245 — Coderefs stringify as `CODE(__ANON__)` instead of `CODE(0x<addr>)`

```sh
$ s -e 'my $c = sub { 1 }; print "$c\n"'
CODE(__ANON__)
```

Perl stringifies anonymous coderefs as `CODE(0x<hexaddr>)`, with the
hex address identifying that particular closure instance. Stryke
returns the literal string `CODE(__ANON__)` for every anonymous
coderef, which prevents using string comparison to distinguish two
distinct closures.

Pin: `coderef_string_form_is_code_anon_not_hex_addr` in
`tests/suite/string_interpolation_pin.rs`.

Severity: **polish** (no semantic loss; affects only debug-print
output and identity-by-string-form patterns).


## BUG-246 — `$$ref` does not deref inside double-quoted string

```sh
$ s -e 'my $x = 7; my $r = \$x; print "val=$$r\n"'
val=SCALAR(0x...)
```

In Perl, `"$$r"` inside a qq-string evaluates the scalar deref
`$$r` and inserts the value (`7`). Stryke instead interpolates `$r`
as the ref's stringification, leaving the result as
`SCALAR(0x...)`-style output.

Workaround: use the `${\ EXPR }` form, which always works:

```stryke
my $x = 7;
my $r = \$x;
print "val=${\ $$r }\n";   # val=7
```

Pin: `scalar_ref_double_dollar_does_not_deref_in_interp` (broken
form) and `scalar_ref_deref_works_via_backslash_block` (working
idiom) in `tests/suite/string_interpolation_pin.rs`.

Severity: **bug** (P2; common Perl idiom silently produces wrong
output instead of erroring; workaround exists but is non-obvious).


## BUG-247 — `length($str)` returns byte-count, not char-count

```sh
$ s -e 'my $s = "snowman:\x{2603}"; print length($s), "\n"'
11
```

The string is 9 characters (`snowman:` = 8 chars + ☃ = 1 char). Stryke
returns 11 (the UTF-8 byte length: 8 + 3). Perl with `use utf8` returns
9; without `use utf8` returns the byte length.

Stryke has no equivalent of `use utf8` — string lengths are always
byte-counted. For char-count, the user needs an explicit codepoint
iterator (no first-class helper exists yet).

Pin: `unicode_interp_length_is_byte_count` in
`tests/suite/string_interpolation_pin.rs`.

Severity: **parity** (matches Perl's *default* behavior without
`use utf8`; documented here so users don't expect `use utf8` semantics).


## BUG-248 — `caller(N)` returns wrong package and line

```sh
$ s -e '
package Demo::P1;
sub here { my @c = caller(0); print "pkg=$c[0] line=$c[2]\n"; }
package Demo::P2;
sub call_p1 { Demo::P1::here() }
package main;
Demo::P2::call_p1();'
pkg=main line=3
```

In Perl, `caller(0)` inside `here` would report `pkg=Demo::P2` (the
calling sub's package) and the line of the `Demo::P1::here()` call
site within `call_p1` (line 5). Stryke reports `pkg=main` and `line=3`
(the line where `caller(0)` itself was invoked).

Both fields are observable but neither matches Perl. The current
shape is pinned so any future fix is deliberate; downstream code that
inspects caller info for stack traces or AOP attribution will give
the wrong attribution today.

Pin: `caller_package_always_main_per_bug_248`,
`caller_line_is_callee_site_not_invocation_site` in
`tests/suite/caller_stack_pin.rs`.

Severity: **bug** (P1; stack-walking is wrong on two of three fields;
affects logging, AOP, error-reporting code paths).


## BUG-249 — `caller(N)` never returns empty list

```sh
$ s -e 'my @c = caller(0); print "len=", scalar(@c), "\n"'
len=3

$ s -e 'sub f { my @c = caller(99); print "deep=", scalar(@c), "\n" } f()'
deep=3
```

Perl returns an empty list when `caller(N)` is called at the top
level (no caller) or past the bottom of the stack. Stryke always
returns a 3-tuple (`main`, file, line), making it impossible to
detect "no caller" by checking list length.

`scalar(caller(0))` further returns the field count (3) rather than
the package, breaking the common Perl idiom `if (caller()) { ... }`.

Pin: `caller_at_top_level_returns_non_empty`,
`caller_past_stack_depth_returns_non_empty`,
`caller_scalar_context_is_field_count_not_package` in
`tests/suite/caller_stack_pin.rs`.

Severity: **bug** (P2; breaks "am I being called as main script?"
guard pattern in Perl scripts).


## BUG-250 — `chomp` ignores `local $/` (input record separator)

```sh
$ s -e 'local $/ = "END"; my $s = "dataEND"; chomp($s); print "[$s]\n"'
[dataEND]
```

In Perl, `chomp` strips whatever string `$/` holds, so `local $/ = "END"`
makes `chomp("dataEND")` strip the trailing `"END"`. Stryke `chomp`
always strips a trailing `\n` regardless of `$/`, so the assignment to
`$/` has no effect on the operation.

This breaks the common Perl record-stream idiom:

```perl
local $/ = "---END---";
while (my $rec = <$fh>) {
    chomp $rec;     # would strip "---END---" in Perl; stryke leaves it
    ...
}
```

Workaround: explicit `$s =~ s/\Q$sep\E\z//` substitution.

Pin: `chomp_does_not_honor_local_record_separator_per_bug_250` in
`tests/suite/chomp_chop_pin.rs`.

Severity: **bug** (P2; quietly produces wrong output on every
record-mode parser written in Perl style; workaround exists).


## BUG-257 — `$\`` and `$'` regex pre/post-match vars not parseable

```sh
$ s -e 'my $s = "abc"; $s =~ /b/; print $`'
Expected variable name after $ at -e line 1.
```

In Perl, `$\`` (prematch) and `$'` (postmatch) are special punctuation
variables populated after a successful regex match. Stryke's lexer
doesn't recognize `\`` or `'` as valid variable-name characters, so
attempting to use them is a parse-time syntax error.

Workaround: use the Perl 5.18+ named forms, which stryke DOES support:

```stryke
$s =~ /middle/;
my $pre  = ${^PREMATCH};    # works
my $post = ${^POSTMATCH};   # works
my $whole = ${^MATCH};      # works (or `$&`)
```

Pin: `prematch_via_caret_prematch_form`,
`postmatch_via_caret_postmatch_form` in
`tests/suite/regex_match_vars_pin.rs`.

Severity: **polish** (modern named forms work; only legacy
punctuation form is missing).


## NOT-A-BUG observations (pinned, but documented as deliberate)

These are known design choices, listed here so a future contributor doesn't
"fix" them:

- **`succ`/`pred` are numeric-only.** `succ("b")` returns `1`, not `"c"`.
  See test `succ_on_string_numifies_to_zero_plus_one`. The Perl-magic form
  is reachable only through `++`, which is governed by PARITY-001 above.

- **Many short names are stryke builtins** (`fact`, `factorial`, `id`,
  `squared`, `cubed`, `f`, etc.). Outside `--compat`, `fn name { ... }`
  for any of these is a parse-time rejection. Tests cover `id` and
  `squared`. Note that `neg` is *not* a builtin — calling `neg(7)` raises
  `Undefined subroutine &neg`, so the unary-minus role still belongs to
  the `-` operator.

- **`p` of an arrayref/hashref prints `ARRAY(0x...)` / `HASH(0x...)`.**
  This matches Perl's `print` semantics for refs. To dump structure, use
  the appropriate dump helper.


## How to add to this file

When you find a new behavior worth tracking:

1. Add a numbered section (continue PARITY-NNN / BUG-NNN / POLISH-NNN).
2. Show the minimal reproducer with `stryke -e '...'` and the observed
   output. If applicable, contrast with `perl -e '...'`.
3. Add a pinning test in `tests/suite/behavior_pin_2026_05.rs` (or a
   dated successor, e.g. `behavior_pin_2026_06.rs` once this file fills).
4. Cite the test name(s) in the BUGS.md entry so they stay linked.

When a bug is fixed, remove its entry from this file and flip the
pinning test from "current buggy output" to "correct output" — the test
is the regression guard going forward. Numeric IDs are not reused.
