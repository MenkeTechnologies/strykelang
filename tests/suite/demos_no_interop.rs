//! Local `cargo test` pin: every public demo under `examples/*.stk` must run
//! clean under `stryke --no-interop`. CI runs the same coverage via
//! `examples/run_all_ci.stk` (single orchestrator in the `stryke-test` job).
//!
//! Rationale per CLAUDE.md: `--no-interop` is the bot firewall — it
//! enforces stryke idioms at parse time (no `scalar`, no `length`,
//! no `reverse`, no `$a`/`$b` magic). Demos are the public face of
//! the language; they must showcase idiomatic stryke, not Perl
//! transliterations.
//!
//! Tests are gated on the presence of a built binary (`target/debug/s`
//! preferred; falls back to `target/release/s`). If neither exists
//! the assertion is skipped — local-dev workflows that haven't run
//! `cargo build` yet aren't penalized.

use std::path::PathBuf;
use std::process::Command;

use crate::common::GLOBAL_FLAGS_LOCK;

fn stryke_binary() -> Option<PathBuf> {
    // Prefer the freshest of {target/debug/s, target/release/s}.
    // `s` is the short binary alias the demos invoke via shebang.
    let cands = [
        "target/release/s",
        "target/debug/s",
        "target/release/stryke",
        "target/debug/stryke",
    ];
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for cand in cands {
        let p = PathBuf::from(cand);
        if let Ok(meta) = std::fs::metadata(&p) {
            if let Ok(m) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| m > *t) {
                    best = Some((p, m));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

fn run_demo(path: &str) -> Result<(), String> {
    let bin = match stryke_binary() {
        Some(b) => b,
        None => return Ok(()), // skip: no built binary
    };
    let _guard = GLOBAL_FLAGS_LOCK.read();
    let out = Command::new(&bin)
        .args(["--no-interop", path])
        .output()
        .map_err(|e| format!("spawn {bin:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "demo {path} failed under --no-interop (exit={:?}):\nstderr:\n{}\nstdout-tail:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .rev()
                .take(10)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(())
}

macro_rules! demo_runs_no_interop {
    ($fn_name:ident, $path:expr) => {
        #[test]
        fn $fn_name() {
            if let Err(e) = run_demo($path) {
                panic!("{}", e);
            }
        }
    };
}

demo_runs_no_interop!(demo_kvstore_basics, "examples/kvstore_basics.stk");
demo_runs_no_interop!(demo_kvstore_cache, "examples/kvstore_cache.stk");
demo_runs_no_interop!(demo_kvstore_namespace, "examples/kvstore_namespace.stk");
demo_runs_no_interop!(demo_sketch_algebra, "examples/sketch_algebra.stk");
demo_runs_no_interop!(demo_sketches_tier2, "examples/sketches_tier2.stk");
demo_runs_no_interop!(demo_numerical_ids, "examples/numerical_ids.stk");
demo_runs_no_interop!(demo_shell_repl, "examples/shell_repl.stk");
demo_runs_no_interop!(demo_pipe_forward, "examples/pipe_forward.stk");
demo_runs_no_interop!(demo_thread_macro, "examples/thread_macro.stk");
demo_runs_no_interop!(demo_implicit_params, "examples/implicit_params.stk");
demo_runs_no_interop!(demo_parallel_primitives, "examples/parallel_primitives.stk");
demo_runs_no_interop!(demo_reflection_hashes, "examples/reflection_hashes.stk");
demo_runs_no_interop!(demo_oop_classes, "examples/oop_classes.stk");
demo_runs_no_interop!(demo_algebraic_match, "examples/algebraic_match.stk");
demo_runs_no_interop!(demo_aop_intercepts, "examples/aop_intercepts.stk");
demo_runs_no_interop!(demo_regex_three_tier, "examples/regex_three_tier.stk");
demo_runs_no_interop!(demo_glob_qualifiers, "examples/glob_qualifiers.stk");
demo_runs_no_interop!(demo_string_coordinates, "examples/string_coordinates.stk");
demo_runs_no_interop!(demo_iterator_ops, "examples/iterator_ops.stk");
demo_runs_no_interop!(demo_file_streams, "examples/file_streams.stk");
demo_runs_no_interop!(demo_crypto, "examples/crypto.stk");
demo_runs_no_interop!(demo_codecs, "examples/codecs.stk");
demo_runs_no_interop!(demo_async_tasks, "examples/async_tasks.stk");
demo_runs_no_interop!(demo_datetime, "examples/datetime.stk");
demo_runs_no_interop!(demo_run_source, "examples/run_source.stk");
demo_runs_no_interop!(demo_uniq_idioms, "examples/uniq_idioms.stk");
demo_runs_no_interop!(demo_ai_session_memory, "examples/ai_session_memory.stk");
demo_runs_no_interop!(demo_etl_pipeline, "examples/etl_pipeline.stk");
demo_runs_no_interop!(demo_parallel_ai_batch, "examples/parallel_ai_batch.stk");
demo_runs_no_interop!(demo_log_analyzer, "examples/log_analyzer.stk");
demo_runs_no_interop!(demo_aop_metrics, "examples/aop_metrics.stk");
demo_runs_no_interop!(demo_signed_documents, "examples/signed_documents.stk");
demo_runs_no_interop!(demo_word_freq_stream, "examples/word_freq_stream.stk");
demo_runs_no_interop!(demo_web_jobs, "examples/web_jobs.stk");
demo_runs_no_interop!(demo_retry_backoff, "examples/retry_backoff.stk");
demo_runs_no_interop!(demo_markov_chain, "examples/markov_chain.stk");
demo_runs_no_interop!(demo_stream_merge, "examples/stream_merge.stk");
demo_runs_no_interop!(demo_graph_bfs, "examples/graph_bfs.stk");
demo_runs_no_interop!(demo_sql_dsl, "examples/sql_dsl.stk");
demo_runs_no_interop!(demo_job_queue, "examples/job_queue.stk");
demo_runs_no_interop!(demo_cache_layered, "examples/cache_layered.stk");
demo_runs_no_interop!(demo_text_search, "examples/text_search.stk");
demo_runs_no_interop!(demo_event_dispatcher, "examples/event_dispatcher.stk");
demo_runs_no_interop!(demo_csv_pivot, "examples/csv_pivot.stk");
demo_runs_no_interop!(demo_state_machine, "examples/state_machine.stk");
demo_runs_no_interop!(demo_url_router, "examples/url_router.stk");
demo_runs_no_interop!(demo_expression_parser, "examples/expression_parser.stk");
demo_runs_no_interop!(demo_rate_limiter, "examples/rate_limiter.stk");
demo_runs_no_interop!(demo_lru_cache, "examples/lru_cache.stk");
demo_runs_no_interop!(demo_priority_queue, "examples/priority_queue.stk");
demo_runs_no_interop!(demo_json_lines_log, "examples/json_lines_log.stk");
demo_runs_no_interop!(demo_inventory_system, "examples/inventory_system.stk");
demo_runs_no_interop!(demo_markov_text_gen, "examples/markov_text_gen.stk");
demo_runs_no_interop!(demo_circuit_breaker, "examples/circuit_breaker.stk");
demo_runs_no_interop!(demo_anagram_groups, "examples/anagram_groups.stk");
demo_runs_no_interop!(
    demo_observability_dashboard,
    "examples/observability_dashboard.stk"
);
demo_runs_no_interop!(demo_dependency_graph, "examples/dependency_graph.stk");
demo_runs_no_interop!(demo_banking_ledger, "examples/banking_ledger.stk");
demo_runs_no_interop!(demo_url_normalizer, "examples/url_normalizer.stk");
demo_runs_no_interop!(demo_diff_patch, "examples/diff_patch.stk");
demo_runs_no_interop!(demo_spam_classifier, "examples/spam_classifier.stk");
demo_runs_no_interop!(demo_k_means_clustering, "examples/k_means_clustering.stk");
demo_runs_no_interop!(demo_log_redaction, "examples/log_redaction.stk");
demo_runs_no_interop!(demo_scheduler, "examples/scheduler.stk");
demo_runs_no_interop!(demo_feature_flags, "examples/feature_flags.stk");
demo_runs_no_interop!(demo_word_count, "examples/word_count.stk");
demo_runs_no_interop!(demo_maze_solver, "examples/maze_solver.stk");
demo_runs_no_interop!(demo_json_diff, "examples/json_diff.stk");
demo_runs_no_interop!(demo_csv_validator, "examples/csv_validator.stk");
demo_runs_no_interop!(demo_trie_completer, "examples/trie_completer.stk");
demo_runs_no_interop!(demo_conway_life, "examples/conway_life.stk");
demo_runs_no_interop!(demo_dijkstra_path, "examples/dijkstra_path.stk");
demo_runs_no_interop!(demo_api_health_check, "examples/api_health_check.stk");
demo_runs_no_interop!(demo_bloom_dedup, "examples/bloom_dedup.stk");
demo_runs_no_interop!(demo_tic_tac_toe, "examples/tic_tac_toe.stk");
demo_runs_no_interop!(demo_leaderboard, "examples/leaderboard.stk");
demo_runs_no_interop!(demo_text_stats, "examples/text_stats.stk");
demo_runs_no_interop!(demo_heap_sort, "examples/heap_sort.stk");
demo_runs_no_interop!(demo_quiz_grader, "examples/quiz_grader.stk");
demo_runs_no_interop!(demo_sudoku_solver, "examples/sudoku_solver.stk");
demo_runs_no_interop!(demo_url_shortener, "examples/url_shortener.stk");
demo_runs_no_interop!(demo_genome_analysis, "examples/genome_analysis.stk");
demo_runs_no_interop!(demo_order_processor, "examples/order_processor.stk");
demo_runs_no_interop!(demo_dns_zone_check, "examples/dns_zone_check.stk");
demo_runs_no_interop!(demo_calculator_repl, "examples/calculator_repl.stk");
demo_runs_no_interop!(demo_file_diff_dir, "examples/file_diff_dir.stk");
demo_runs_no_interop!(demo_email_parser, "examples/email_parser.stk");
demo_runs_no_interop!(demo_board_game_sim, "examples/board_game_sim.stk");
demo_runs_no_interop!(demo_clipboard_manager, "examples/clipboard_manager.stk");
demo_runs_no_interop!(demo_snake_game_sim, "examples/snake_game_sim.stk");
demo_runs_no_interop!(demo_inverted_index, "examples/inverted_index.stk");
demo_runs_no_interop!(demo_ip_address_calc, "examples/ip_address_calc.stk");
demo_runs_no_interop!(demo_social_graph, "examples/social_graph.stk");
demo_runs_no_interop!(demo_markdown_parser, "examples/markdown_parser.stk");
demo_runs_no_interop!(demo_payroll_calc, "examples/payroll_calc.stk");
demo_runs_no_interop!(demo_syslog_analyzer, "examples/syslog_analyzer.stk");
demo_runs_no_interop!(demo_recipe_planner, "examples/recipe_planner.stk");
demo_runs_no_interop!(demo_dependency_resolver, "examples/dependency_resolver.stk");
demo_runs_no_interop!(demo_calendar_planner, "examples/calendar_planner.stk");
demo_runs_no_interop!(demo_dna_alignment, "examples/dna_alignment.stk");
demo_runs_no_interop!(demo_load_balancer, "examples/load_balancer.stk");
demo_runs_no_interop!(demo_morse_code, "examples/morse_code.stk");
demo_runs_no_interop!(demo_currency_converter, "examples/currency_converter.stk");
demo_runs_no_interop!(demo_vending_machine, "examples/vending_machine.stk");
demo_runs_no_interop!(demo_elf_inventory, "examples/elf_inventory.stk");
demo_runs_no_interop!(demo_prime_sieve, "examples/prime_sieve.stk");
demo_runs_no_interop!(demo_huffman_coding, "examples/huffman_coding.stk");
demo_runs_no_interop!(demo_genetic_algorithm, "examples/genetic_algorithm.stk");
demo_runs_no_interop!(demo_barcode_scanner, "examples/barcode_scanner.stk");
demo_runs_no_interop!(demo_bingo_simulator, "examples/bingo_simulator.stk");
demo_runs_no_interop!(demo_port_scanner_sim, "examples/port_scanner_sim.stk");
demo_runs_no_interop!(demo_tax_calculator, "examples/tax_calculator.stk");
demo_runs_no_interop!(demo_crc32_checksum, "examples/crc32_checksum.stk");
demo_runs_no_interop!(demo_mandelbrot_ascii, "examples/mandelbrot_ascii.stk");
demo_runs_no_interop!(demo_knapsack_optimizer, "examples/knapsack_optimizer.stk");
demo_runs_no_interop!(demo_pythagorean_triples, "examples/pythagorean_triples.stk");
demo_runs_no_interop!(demo_tortoise_hare_cycle, "examples/tortoise_hare_cycle.stk");
demo_runs_no_interop!(demo_gray_code, "examples/gray_code.stk");
demo_runs_no_interop!(demo_rule_30_automaton, "examples/rule_30_automaton.stk");
demo_runs_no_interop!(demo_n_queens_count, "examples/n_queens_count.stk");
demo_runs_no_interop!(demo_langtons_ant, "examples/langtons_ant.stk");
demo_runs_no_interop!(demo_aliquot_sequences, "examples/aliquot_sequences.stk");
demo_runs_no_interop!(demo_de_bruijn_sequence, "examples/de_bruijn_sequence.stk");
demo_runs_no_interop!(demo_knights_tour, "examples/knights_tour.stk");
demo_runs_no_interop!(demo_mini_forth, "examples/mini_forth.stk");
demo_runs_no_interop!(demo_kaprekar_routine, "examples/kaprekar_routine.stk");
demo_runs_no_interop!(demo_braille_transcoder, "examples/braille_transcoder.stk");
demo_runs_no_interop!(demo_phyllotaxis_spiral, "examples/phyllotaxis_spiral.stk");
demo_runs_no_interop!(demo_prime_factorization, "examples/prime_factorization.stk");
demo_runs_no_interop!(demo_bit_set_arith, "examples/bit_set_arith.stk");
demo_runs_no_interop!(
    demo_maze_recursive_backtrack,
    "examples/maze_recursive_backtrack.stk"
);
demo_runs_no_interop!(demo_dragon_curve, "examples/dragon_curve.stk");
demo_runs_no_interop!(demo_heronian_triangles, "examples/heronian_triangles.stk");
demo_runs_no_interop!(demo_look_and_say, "examples/look_and_say.stk");
demo_runs_no_interop!(demo_skyline_silhouette, "examples/skyline_silhouette.stk");
demo_runs_no_interop!(demo_convex_hull, "examples/convex_hull_no_interop.stk");
demo_runs_no_interop!(demo_lzw_codec, "examples/lzw_codec_no_interop.stk");
demo_runs_no_interop!(demo_manacher_pal, "examples/manacher_pal_no_interop.stk");
demo_runs_no_interop!(demo_hungarian, "examples/hungarian_no_interop.stk");
demo_runs_no_interop!(demo_hyperloglog, "examples/hyperloglog_no_interop.stk");
demo_runs_no_interop!(demo_gaussian_elimination, "examples/gaussian_elimination_no_interop.stk");
demo_runs_no_interop!(demo_eulerian_circuit, "examples/eulerian_circuit_no_interop.stk");
demo_runs_no_interop!(demo_two_sat, "examples/two_sat_no_interop.stk");
demo_runs_no_interop!(demo_karatsuba, "examples/karatsuba_no_interop.stk");
demo_runs_no_interop!(demo_wavelet_tree, "examples/wavelet_tree_no_interop.stk");
demo_runs_no_interop!(demo_edmonds_karp, "examples/edmonds_karp_no_interop.stk");
demo_runs_no_interop!(demo_chinese_remainder, "examples/chinese_remainder_no_interop.stk");
demo_runs_no_interop!(demo_patience_sort_lis, "examples/patience_sort_lis_no_interop.stk");
demo_runs_no_interop!(demo_smith_waterman, "examples/smith_waterman_no_interop.stk");
demo_runs_no_interop!(demo_stoer_wagner, "examples/stoer_wagner_no_interop.stk");
demo_runs_no_interop!(demo_closest_pair, "examples/closest_pair_no_interop.stk");
demo_runs_no_interop!(demo_suffix_automaton, "examples/suffix_automaton_no_interop.stk");
demo_runs_no_interop!(demo_hopcroft_dfa, "examples/hopcroft_dfa_no_interop.stk");
demo_runs_no_interop!(demo_mo_algorithm, "examples/mo_algorithm_no_interop.stk");
demo_runs_no_interop!(demo_splay_tree, "examples/splay_tree_no_interop.stk");
demo_runs_no_interop!(demo_tonelli_shanks, "examples/tonelli_shanks_no_interop.stk");
demo_runs_no_interop!(demo_lcp_kasai, "examples/lcp_kasai_no_interop.stk");
demo_runs_no_interop!(demo_min_cost_max_flow, "examples/min_cost_max_flow_no_interop.stk");
demo_runs_no_interop!(demo_regex_thompson_nfa, "examples/regex_thompson_nfa_no_interop.stk");
demo_runs_no_interop!(demo_karger_min_cut, "examples/karger_min_cut_no_interop.stk");
demo_runs_no_interop!(demo_strassen, "examples/strassen_no_interop.stk");
demo_runs_no_interop!(demo_rotating_calipers, "examples/rotating_calipers_no_interop.stk");
demo_runs_no_interop!(demo_boyer_moore_majority, "examples/boyer_moore_majority_no_interop.stk");
demo_runs_no_interop!(demo_lazy_segment_tree, "examples/lazy_segment_tree_no_interop.stk");
demo_runs_no_interop!(demo_bsgs, "examples/bsgs_no_interop.stk");
demo_runs_no_interop!(demo_xor_basis, "examples/xor_basis_no_interop.stk");
demo_runs_no_interop!(demo_ntt, "examples/ntt_no_interop.stk");
demo_runs_no_interop!(demo_edmonds_blossom, "examples/edmonds_blossom_no_interop.stk");
demo_runs_no_interop!(demo_adam_optimizer, "examples/adam_optimizer_no_interop.stk");
demo_runs_no_interop!(demo_chinese_postman, "examples/chinese_postman_no_interop.stk");
demo_runs_no_interop!(demo_chu_liu_edmonds, "examples/chu_liu_edmonds_no_interop.stk");
demo_runs_no_interop!(demo_runge_kutta_4, "examples/runge_kutta_4_no_interop.stk");
demo_runs_no_interop!(demo_tarjan_articulation, "examples/tarjan_articulation_no_interop.stk");
demo_runs_no_interop!(demo_brent_root_finding, "examples/brent_root_finding_no_interop.stk");
demo_runs_no_interop!(demo_adaptive_simpson, "examples/adaptive_simpson_no_interop.stk");
demo_runs_no_interop!(demo_boruvka_mst, "examples/boruvka_mst_no_interop.stk");
demo_runs_no_interop!(demo_walsh_hadamard, "examples/walsh_hadamard_no_interop.stk");
demo_runs_no_interop!(demo_li_chao_tree, "examples/li_chao_tree_no_interop.stk");
demo_runs_no_interop!(demo_berlekamp_massey, "examples/berlekamp_massey_no_interop.stk");
demo_runs_no_interop!(demo_ukkonen_suffix_tree, "examples/ukkonen_suffix_tree_no_interop.stk");
demo_runs_no_interop!(demo_prolog_engine, "examples/prolog_engine_no_interop.stk");
demo_runs_no_interop!(demo_chess_engine, "examples/chess_engine_no_interop.stk");
demo_runs_no_interop!(demo_hindley_milner, "examples/hindley_milner_no_interop.stk");
