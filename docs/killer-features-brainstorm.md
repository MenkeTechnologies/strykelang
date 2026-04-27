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
| Embeddable | Lua/Tcl | ? TBD |
| Sigils for compression | Perl | ✓ `$@%` |
| Implicit `$_` | Perl | ✓ Plus `_` shorthand |
| Range with step | Python-ish | ✓ BETTER — 10 types! |

## The Gap: Expect-style Interactive Automation

**Source:** Tcl/Expect (1990, still unmatched)

**What it does:** Automate interactive CLI sessions — spawn process, wait for pattern in output, send input, repeat.

**Use cases:**
- SSH with password prompts
- Interactive installers
- CLI tools that prompt
- Database REPLs
- Network equipment (routers, switches)
- MFA prompts
- Legacy systems

**Why it's missing everywhere:**
- Python has `pexpect` — clunky
- Ruby has `expect` gem — meh
- Go has nothing good
- Node has nothing good
- Everyone just shells out to actual Expect

**Proposed Stryke syntax:**
```stryke
my $s = spawn "ssh user@host"
$s ~ /password:/ >> "hunter2\n"
$s ~ /\$/ >> "ls -la\n"
p $s ~ /\$/
$s.close

# Or thread-style
t spawn("ssh host") expect(/pass:/) send("pw\n") expect(/\$/) send("ls\n") expect(/\$/)
```

**Enterprise value:** Huge for infra automation + cluster dispatch combo.

See `docs/expect-feature-idea.md` for full design.

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

Answer: Not many. The main gap identified is **Expect-style interactive automation**.

The polymorphic range system implemented tonight (10 types, forward/reverse, custom step) is itself a world-first that no other language has.

## Guiding Principle

> "My secret is say buck the trend, what about this. Why stop at ints/chars. We can range on every imaginable item. It's rebellion."

Keep asking "why does this have limits?" instead of "how do I work within the limits?"
