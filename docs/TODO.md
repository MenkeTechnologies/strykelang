  Bugs Documented/Worked Around (not fixable without major changes)

  1. Arrays don't share state in closures - Arrays captured by closures are copied, not shared. Use
      arrayref ($tokens = []) instead.

  2. AOP advice bodies require their final statement to be an expression (same constraint
     as `map { }` block lowering). A literal `for`/`while`/`if` block as the final form, or
     a literal `return`, fails advice firing with a runtime error. Rewrite to use an
     expression form (`sum(@xs)` instead of `for ...`, ternary instead of `return`-guarded).
     Loosening this requires a custom advice-body lowering pass (current path reuses the
     existing `try_compile_block_region`, which is shared with map/grep).



        Think about what's still on the table. stryke beat LuaJIT with:

  - Not all ops lowered yet
  - No Cranelift JIT — still interpreting bytecodes
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
  │ Cranelift JIT for hot loops │ 5-50x on tight computation — native machine │ Medium           │
  │                             │  code, no dispatch                          │                  │
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
