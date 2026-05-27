  # Stryke TODO — Known Limitations + Optimization Backlog

  This file tracks (1) shipped-but-imperfect surface that has documented workarounds
  and (2) the optimization menu that's still on the table now that stryke beats LuaJIT
  on loop/array/regex without any of the heavy techniques applied.

  Feature shipping status lives in ROADMAP.md, AI_PRIMITIVES.md, PACKAGE_REGISTRY.md,
  and WEB_FRAMEWORK.md. This document is for crash-known bugs and JIT/perf work.

  ## Bugs Documented/Worked Around (not fixable without major changes)

  1. Arrays don't share state in closures - Arrays captured by closures are copied, not shared. Use
      arrayref ($tokens = []) instead.

  2. AOP advice bodies require their final statement to be an expression (same constraint
     as `map { }` block lowering). A literal `for`/`while`/`if` block as the final form, or
     a literal `return`, fails advice firing with a runtime error. Rewrite to use an
     expression form (`sum(@xs)` instead of `for ...`, ternary instead of `return`-guarded).
     Loosening this requires a custom advice-body lowering pass (current path reuses the
     existing `try_compile_block_region`, which is shared with map/grep).

  3. Dynamic regex `qr/$var/` / `/$var/` caches across iterations — when the
     source pattern is interpolated from a variable that changes each iteration,
     the first iteration's compiled regex is reused for all subsequent ones.
     See `docs/BUGS.md` BUG-300. Workaround: walk segments by hand
     (`examples/http_router_middleware.stk`) or use string ops like `index`
     (`examples/pubsub_message_bus.stk`).

  4. `pmap_reduce { } { }` silently absorbs the following statement as an
     optional third block argument unless terminated with `;`. See BUG-301.
     Three call sites in examples document the workaround in-line.

  5. Array slice past the end fills with `undef` instead of clamping
     (`@arr[0:N]` for N >= len gives `len` elements plus `(N+1 - len)`
     undefs). See BUG-302. Clamp the upper bound at the caller.

  6. AOP `after { ... $? ... }` advice sees the global `$?` (whatever the
     last `system` left there) instead of the wrapped sub's return value.
     See BUG-044. Use `around { proceed() }` and inspect the return value
     directly when you need the actual value.

  ## Distributed-compute API (congregation/pray/annex) — Tier 5 deferrals

  These limitations live alongside the 28-verb scriptable distributed API
  (see README §0x10c). All are real architectural changes, not workaround
  candidates. Tracking here so future sessions don't re-discover them.

  7. **`EVAL_RESULT` carries no `petition_id`** — agent reply queue is FIFO
     per-agent, so a chant fired on agent join leaves a stale "ok" reply
     in the socket buffer that the next `pray + annex` consumes instead
     of its own result. Workaround in scripts: drain with a discard
     scatter before the real readback. Fix requires bumping
     `AGENT_PROTO_VERSION` and adding a `petition_id` field to
     `EvalCommand`/`EvalResult` so `gather` can demux by id. See
     `tests/suite/scriptable_controller_pin.rs::chant_fires_at_new_joiners_*`
     for the workaround pattern.

  8. **Cathedral is in-process only (no `stryked` daemon yet)** —
     `ordain($name)` registers `name → endpoint` in a process-local
     `OnceLock<Mutex<HashMap>>`, so `profess($name)` only resolves
     congregations ordained in the SAME OS process. Tier 5 promotes the
     cathedral to a standalone `stryked` daemon for cross-host name
     resolution. Workaround: pass the endpoint explicitly via env var
     until then.

  9. **`divine($handler)` is a no-op marker** — registers the closure but
     the agent's EVAL loop doesn't yet dispatch through it. Workaround:
     master sends EVAL code that calls `$divine_handler->($petition)`
     explicitly. Tier 5 splits the agent's frame handler to consult the
     registered handler first when present.

  10. **`recant(@keys)` returns intended-delete count, doesn't actually
      delete** — current implementation reports how many keys it would
      delete; caller wraps with `for my $k (@keys) { delete $main::soul{$k} }`
      to make it real. Tier 5 wires a Rust interpreter handle so the
      delete is atomic from inside the builtin.

  ### Two stryke language bugs fixed (2026-05-27)

  Surfaced during the Tier 3 lick/peruse work; fixed at the VM level
  rather than papered over. Listed here for the archeology.

  - **`\%our-hash` derefed as empty.** `promote_hash_to_shared(name)` /
    `promote_array_to_shared(name)` compared the raw `name` against
    canonical entry keys without stripping the `main::` prefix → fell
    through to empty Arc. Fixed in `scope.rs` by canonicalizing at
    function entry.
  - **`our %hash` didn't persist across EVAL boundaries.** Two parts:
    (a) `declare_hash` always stored in `frames.last_mut()` regardless
    of package qualification — fixed by routing package-qualified names
    to `frames.first_mut()`. (b) `Op::DeclareHash` clobbered with
    empty for the no-initializer form (`our %h;` compiles to
    `LoadUndef + DeclareHash`) — fixed by detecting undef value AND
    `n.contains("::")` to preserve existing data (lexical `my %h;`
    still clobbers per iteration as required by loop-local semantics).



  ## What's still on the table

  stryke beat LuaJIT with:

  - Not all ops lowered yet
  - Cranelift JIT lands hot bytecode regions but not every op is JIT-eligible
  - No profile-guided optimization of the fused superinstructions
  - No inline caching for method dispatch
  - No type specialization (everything goes through Value coercions)

  LuaJIT is Mike Pall's finished masterpiece. A decade of hand-tuned assembly, custom allocator, trace
   compiler, register allocation. That's his ceiling. stryke is already at that ceiling while leaving
  massive optimizations untouched.

  What beta unlocks:

  ┌─────────────────────────────┬─────────────────────────────────────────────┬──────────────────┐
  │        Optimization         │                   Impact                    │    Difficulty    │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Cranelift JIT — full coverage│ 5-50x on tight computation — native machine│ Medium           │
  │ (shipped, partial)          │  code, no dispatch. Hot-loop path live;     │                  │
  │                             │  remaining bytecodes still go through       │                  │
  │                             │  the interpreter dispatch.                  │                  │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Type specialization (int    │ 2-3x on arithmetic — skip Value::to_int()   │ Easy             │
  │ fast path)                  │ coercion entirely                           │                  │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Inline caching for builtins │ 1.5-2x on builtin-heavy code — direct       │ Easy             │
  │                             │ function pointer, no lookup                 │                  │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Register allocation in JIT  │ 2-3x — values stay in CPU registers, not on │ Hard (Cranelift  │
  │                             │  heap stack                                 │ helps)           │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Escape analysis             │ Eliminate Arc<String> allocation for        │ Hard             │
  │                             │ short-lived strings                         │                  │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ More fused                  │ Each new pattern detected is another loop   │ Ongoing          │
  │ superinstructions           │ that runs as one op                         │                  │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ SIMD string ops             │ 2-10x on string matching, split, join       │ Medium           │
  ├─────────────────────────────┼─────────────────────────────────────────────┼──────────────────┤
  │ Dead code elimination       │ Skip unreachable branches at compile time   │ Easy             │
  └─────────────────────────────┴─────────────────────────────────────────────┴──────────────────┘
