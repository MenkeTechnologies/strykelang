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
