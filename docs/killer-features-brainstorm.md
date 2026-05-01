# Stryke: Killer Features Brainstorm

## The Question

What killer features do other languages have that Stryke can pull in to become the ultimate, endgame language?

## Philosophy

- "Buck the trend, what about this?" — don't accept arbitrary limitations
- Every limitation is a design bug, not a fact of life
- Kitchen sink language that is NATIVE fast
- More than one way to do it, in parallel
- Terse AND readable (not APL line noise)

## Already Covered (as of 2026-04-26)

Most "killer features" from other languages are already in Stryke:

| Feature | Source Lang | Stryke Status |
|---------|-------------|---------------|
| Array ops on whole collections | APL/J/K | ✓ Native builtins |
| Implicit field splitting | Awk | ✓ Autosplit |
| Thread macro pipelines | Clojure | ✓ `t`, `->`, `->>` |
| Pipe operator | F#/Elixir | ✓ `\|>` |
| Regex everywhere | Perl | ✓ Native |
| Shell integration | Bash | ✓ Backticks, pipes |
| Process control | Shell | ✓ Built in |
| Distributed dispatch | Erlang-ish | ✓ Cluster dispatch |
| Coroutines/async | Lua/Go | ✓ Present |
| Pattern matching | ML/Rust | ✓ Regexes + match |
| Expect-style PTY automation | Tcl/Expect | ✓ `pty_spawn`/`expect`/`send` (perl_pty.rs) |
| AI primitives (`ai`, MCP, agents) | None — world first | ✓ ai.rs, mcp.rs |
| Web framework (Rails-shaped) | Rails | ✓ stryke_web |
| Package manager (Cargo-shaped) | Cargo | ✓ pkg/ — path deps + workspaces wired |
| Embeddable | Lua/Tcl | ⏳ TBD — host-language embedding API not yet exposed |
| Sigils for compression | Perl | ✓ `$@%` |
| Implicit `$_` | Perl | ✓ Plus `_` shorthand |
| Range with step | Python-ish | ✓ BETTER — 10 types! |

## The (Former) Gap: Expect-style Interactive Automation — **SHIPPED**

**Source:** Tcl/Expect (1990, still unmatched until now)

PTY/expect runtime fully wired in `strykelang/perl_pty.rs`. Builtins: `pty_spawn`, `pty_send`, `pty_read`, `pty_expect`, `pty_expect_table`, `pty_buffer`, `pty_alive`, `pty_eof`, `pty_close`, `pty_interact`. Method-form sugar via `require "perl_pty_class.stk"`.

```stryke
my $h = pty_spawn("ssh user@host");
pty_expect($h, qr/password:/, 30);
pty_send($h, "hunter2\n");
pty_expect($h, qr/\$ /, 30);
pty_send($h, "uptime\n");
my $out = pty_expect($h, qr/\$ /, 30);
pty_close($h);

# Combined with cluster dispatch — parallel SSH automation across N hosts
my $cluster = cluster(["host1:8", "host2:8", "host3:8"]);
pmap_on $cluster @hosts -> $host {
    my $h = pty_spawn("ssh $host");
    pty_expect($h, qr/password:/, 10);
    pty_send($h, "$passwords{$host}\n");
    pty_expect($h, qr/\$ /, 30);
    pty_send($h, "apt update && apt upgrade -y\n");
}
```

**Enterprise value:** infra automation + cluster dispatch combo realized.

See `docs/expect-feature-idea.md` for the full phase log.

## Other Features to Consider

### Hot Code Reload (Erlang)
- Push new code to running nodes without dropping connections
- Probably overkill for Stryke's scripting focus
- Maybe useful for long-running agent processes?

### Logic Programming / Backtracking (Prolog)
- "Find all X where these constraints hold"
- Pattern matching on steroids
- Niche but powerful for certain problems
- Could be a builtin: `solve { constraints }`?

### Metatables / DSL Creation (Lua)
- Override operators per-object
- Create mini-languages trivially
- Stryke has `tie` from Perl — similar?

### Lazy Evaluation (Haskell)
- Infinite lists that compute on demand
- Stryke has iterators — how lazy are they?

### Actor Model / Supervision Trees (Erlang/Elixir)
- "Let it crash" — supervisors restart failed processes
- Could enhance the cluster agent model
- Master supervises agents, restarts on failure

### Full Macro System (Lisp)
- Code as data, transform AST at compile time
- Thread macro is a taste of this
- Full macros = users can add new syntax
- Dangerous but powerful

### Process Substitution (Bash)
- `diff <(cmd1) <(cmd2)` — treat command output as file
- Stryke may have this?

### Here-strings (Bash/Zsh)
- `<<<` to pass string as stdin
- Terser than echo pipe

## The Business Angle

### Revenue Stack
1. **Books** — "Learning Stryke", "Stryke Oneliners", etc.
2. **Conferences** — StrykeConf, workshops, corporate training
3. **Enterprise tooling** — The real money:
   - Cluster load testing
   - Agent orchestration
   - Dashboards
   - Compliance features
   - Support SLAs

### Model
- Language: FOSS (adoption/marketing)
- Books: $40
- Training: $2k/seat
- Enterprise license: $50k/year

### Competitors in Load Testing
- JMeter: Java, slow, XML hell
- Gatling: Scala, JVM overhead
- k6: Go, limited scripting
- Locust: Python, can't saturate anything
- LoadRunner: $$$$$ and ancient

Stryke position: "Fast as Go, scriptable as Python, distributed out of the box"

## The Master/Agent Architecture

```
Stryke Master REPL
       │
       ├── Agent 1 (pins cores, controls resources)
       ├── Agent 2 
       ├── Agent 3
       └── Agent N
       
- Stress tests infrastructure
- Compute/memory pinning
- Distributed load generation
- Metrics back to master
```

This is the $$$ maker. Language is free, enterprise cluster tooling is paid.

## What's Left to Examine?

Languages not yet fully mined for ideas:
- **Nim** — Compiles to C, macros, syntax
- **Zig** — Comptime, no hidden allocations
- **Crystal** — Ruby syntax, compiled
- **Raku** — Perl 6, grammars, hyperoperators
- **Red/Rebol** — Dialects, parse DSL
- **Factor** — Stack-based, quotations
- **Io** — Prototype-based, message passing
- **Smalltalk** — Everything is message send
- **OCaml** — Module system, functors
- **Racket** — Language-oriented programming

## Session Notes

This brainstorm came from the question: "What killer features do other languages have that Stryke can't touch?"

Original answer (2026-04-26): Not many. The main gap was Expect-style interactive automation — now closed.

Subsequent work (2026-04 → 2026-05) shipped the AI primitives surface (`ai`, MCP client/server, agents, vision/audio/image, multi-provider, citations, files API), the Rails-shaped web framework (`stryke_web` with full scaffold generator), the Cargo-shaped package manager (`s install/add/remove/update/outdated/audit/vendor/run/install -g/...` plus `[workspace]` member globbing), and **the full zsh glob qualifier set imported from zshrs** — every qualifier from `man zshexpn` (`(/)`, `(.)`, `(@)`, `(*)`, `(L±N)`, `(om[N])`, `(N)`, `(D)`, `(F)`, `(f<bits>)`, `(d<N>)`, `(e'CMD')`, `(P…)`, `(Q…)`, `(:s/…/…/)`, `^`, `-`, `,`) wired into every glob entry-point (`glob`, `glob_par`, `slurp`/`c`/`cat`, `pwatch`, `<…>`, `par_find_files`). World-first: no other scripting language has this. Each one was its own "world's first AND world's fastest in category" lever.

The polymorphic range system (10 types, forward/reverse, custom step) remains a world-first that no other language has.

## Guiding Principle

> "My secret is say buck the trend, what about this. Why stop at ints/chars. We can range on every imaginable item. It's rebellion."

Keep asking "why does this have limits?" instead of "how do I work within the limits?"
