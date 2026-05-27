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

## Scriptable Master/Slave: IPC + Worker Pool + Distributed Registration

**Status (2026-05-27, updated):** Tier 0–3 SHIPPED (18 builtins green, 11 pin tests passing). Tier 4 (`chant` continuous-rescatter, `cathedral` named cross-host discovery, `profess`/`apostatize` slave-initiated membership, `resurrect`/`martyr`/`recant` state lifecycle, `:cloistered` ACL) requires deeper architectural changes — protocol version bump and registry daemon — and is deferred to a separate work session.

### Shipped (commit b550805c3d Tier 0 + this commit Tier 1-3)

| Tier | Verb | Side | Effect | Status |
|---|---|---|---|---|
| 0 | `congregation(N)` | master | fork N agents locally, return handles | shipped |
| 0 | `ordain([name,bind,port])` | master | spawn bare controller, return handle | shipped |
| 0 | `muster([handle])` | master | list current congregation | shipped |
| 0 | `pray($code, @handles)` | master | scatter, return divination; coderef OR string | shipped |
| 0 | `annex($div [, ms])` | master | gather replies as hash, consume divination | shipped |
| 1 | `harvest($code, @handles [, ms])` | master | one-shot pray+annex fused | shipped |
| 1 | `excommunicate(@handles)` | master | SHUTDOWN frames to subset, drop from roster | shipped |
| 1 | `smite(@handles)` | master | reset workers' `%soul` and `%gift` | shipped |
| 1 | `bestow(\%hash, @handles)` | master | push hash to workers' `%gift` via JSON | shipped |
| 1 | `enshrine(\%hash, $path)` | local | persist hash as JSON to disk | shipped |
| 1 | `exhume($path)` | local | read enshrined JSON back as hash | shipped |
| 1 | `smother(\%hash)` | local | securely zero a local hash in place | shipped |
| 1 | `amen($div)` | local | release divination without gathering | shipped |
| 1 | `anoint($n)` | master | like congregation but don't set current | shipped |
| 1 | `welcome($n [, ms])` | master | block until $n agents joined | shipped |
| 1 | `pilgrimage($code, @handles [, ms])` | master | scatter+gather barrier — true if all rendezvous | shipped |
| 2 | parallel scatter | infra | Rayon par_iter over per-PID writes | shipped |
| 2 | `bow()` | slave | alias for `agent()` (slave-side receive loop) | shipped |
| 3 | `lick(@handles)` | master | non-destructive `%soul` snapshot per agent | shipped |
| 3 | `peruse(@handles)` | master | deeper `%soul` walk (Tier 3 alias of lick) | shipped |

**18 verbs live; 11 pin tests green; 1 example script working end-to-end.**

### Deferred (Tier 4 — separate session)

| Verb / Feature | Why deferred |
|---|---|
| `chant` / `amen` continuous-rescatter | Requires controller-side active-chant table + auto-fire on new joiners |
| `cathedral` registry daemon | New binary mode + cross-process named discovery + STRYKE_CATHEDRAL env |
| `profess` / `apostatize` slave-initiated join | Requires cathedral for name resolution |
| `:cloistered` ACL flag | Requires agent PID in AGENT_HELLO → wire protocol bump |
| `resurrect` / `martyr` / `recant` | Process restart with restored state; needs enshrine integration on agent side |
| `divine` (slave-side explicit handler) | Currently the agent's EVAL loop runs arbitrary code; making `divine` a marker verb adds little until handlers are split out |

### Known stryke language workaround in lick/peruse

`\%hash` on an `our` hash currently produces a ref whose deref reads as empty (verified 2026-05-27 via `to_json(\%soul)` returning `"{}"` even when `keys %soul` shows entries). Workaround: `lick`/`peruse` pass `%soul` flat to `to_json`, which flattens to a JSON array of `[k1, v1, k2, v2, ...]`. Master rehydrates by pairing alternate elements. This is a stryke language bug to fix separately; the lick/peruse wire shape becomes more elegant once `\%hash` works.

### The Gap

The infrastructure already exists in `strykelang/controller.rs` (788 lines) + `strykelang/agent.rs` (938 lines):

- TCP listener daemon (`controller.rs:498` `run_controller`)
- Persistent outbound-connecting agents (`agent.rs:446` `run_agent`)
- Bincode-framed wire protocol: `[u64 LE length][u8 kind][bincode payload]` (`agent.rs:30-46`)
- Message types: `AgentHello`, `AgentHelloAck`, `EvalCommand`, `EvalResult`, `FireCommand`, `WorkloadType`, `AgentState` (`controller.rs:33-36`)
- Stryke builtins exposed: `controller()` (`builtins.rs:13015`), `agent()` (`builtins.rs:13041`)

**Blocker:** both builtins block in their daemon loops. Scripts can launch them but cannot programmatically scatter work, collect results, or coordinate from inside a `.stk` file:

```stryke
controller("0.0.0.0", 9999);   # blocks forever in REPL loop
say "unreachable";              # dead code
```

The infrastructure is real. The REPL-only operation surface is the gap.

### The Religious Vocab (Master/Slave Asymmetric Distributed Compute)

A 26-verb vocabulary mapped onto the existing controller/agent transport. Master = singular orchestrator, slaves = subordinate worker processes that chant in parallel under the master's hymn.

**Master-side verbs (19):**

| Verb | Operand | Effect |
|---|---|---|
| `pray` | `@congregation` → divination | scatter petition (closure/code), return handle |
| `chant` | `@congregation` → divination | continuous re-scatter; late joiners receive the hymn too |
| `amen` | divination | end a chant cleanly |
| `lick` | `$div` or `%souls` | quick non-destructive sample of soul-state |
| `peruse` | `$div` or `%souls` | deep non-destructive walk of soul-state |
| `annex` | `$div` → `%souls` | destructive transfer — workers' `%soul` becomes `()` after |
| `harvest` | `@congregation` → `%souls` | `pray + annex` fused (one-shot scatter+gather) |
| `smother` | `%souls` | secure-erase the master's local hash |
| `smite` | `@congregation` | remote-destroy workers' `%soul` without harvesting |
| `pilgrimage` | `@congregation` | sync barrier — all members rendezvous before continuing |
| `resurrect` | enshrined path → PID | spawn new worker with pre-loaded `%soul` from snapshot |
| `anoint` | N → `@congregation` | spawn N new slave processes, register their PIDs |
| `excommunicate` | `@congregation` | kill workers, free their reply channels |
| `bestow` | `%value, @cong` | master-side push — broadcast value to each worker's `%gift` hash |
| `enshrine` | `%souls, $path` | persist annexed souls to disk for later resurrection |
| `exhume` | `$path` → `%souls` | read enshrined souls back without resurrecting |
| `ordain` | `"name"` → handle | create named congregation, bind discovery channel |
| `muster` | `$cong` → `@pids` | enumerate current congregation members |
| `welcome` | `$cong, callback` | fire callback per join/leave event |

**Slave-side verbs (6):**

| Verb | Operand | Effect |
|---|---|---|
| `bow` | (none) | enter the obedient receive-loop, await petitions |
| `divine` | `$petition` → answer | per-worker compute step inside the bow loop |
| `profess` | `"name"` | join named congregation, advertise own PID |
| `apostatize` | `"name"` | voluntary departure from congregation |
| `martyr` | (none) | voluntary `enshrine` + exit on signal — last-rites snapshot |
| `recant` | `%soul_subset` | partial self-erasure of own `%soul` |

**Noun:** `congregation` — the typed roster handle (PID array under the hood).

**Bless is reserved** — `bless` is Perl 5 core (`parser.rs:13862`'s `is_perl5_core` list) and unavailable. `bestow` is the master→workers push verb instead.

### IPC and Process Worker Pool

**Architectural model: master/slave asymmetric, slave execution parallel.**

```
              Master holds the hymn (petition/closure)
                          │
                          │  pray @congregation   (parallel scatter)
            ┌─────────────┼─────────────┐
            ▼             ▼             ▼
        Slave 1       Slave 2  ...  Slave N
        (divine)      (divine)      (divine)
        ─ parallel ─ parallel ─ parallel ─
            │             │             │
            └─────────────┼─────────────┘
                          │  replies (parallel gather)
                          ▼
                Master annexes %souls
```

**Transport: TCP + bincode (already exists, no SHM, no UDS, no /tmp sockets).** Uniform across single-box (loopback) and cluster (LAN). Same code path. The `teleport.rs` SHM/UDS subsystem is its own thing, not used here.

**Parallel execution requirements:**

| Phase | Current state | What "in parallel" requires |
|---|---|---|
| Scatter (master → slaves) | sequential `for pid in pids` loop pattern in TCP send code | Rayon `par_iter` over slave list — 10μs/send × 10k slaves drops from 100ms serial to ~1ms parallel on 8 cores |
| Slave execution (divine) | each agent already its own OS process | Already parallel — process-per-slave gives SPMD parallelism free |
| Gather (slaves → master) | single recv loop on controller | Multi-reader thread pool on master's reply channel, demuxed by petition-id |
| Reduce (merging into `%souls`) | sequential | Per-key merge via Rayon when reducer is associative + commutative |

**New wire-frame message types to add to the existing `[u64 LE length][u8 kind][bincode payload]` protocol:**

```
PRAY            — master scatters a petition (closure + petition_id) to a slave
DIVINE_REPLY    — slave returns answer (petition_id + StrykeValue) 
LICK_REQUEST    — master requests non-destructive %soul snapshot
PERUSE_REQUEST  — master requests deep %soul read
ANNEX_REQUEST   — master demands destructive transfer (slave clears own %soul)
SMOTHER_NOTIFY  — master tells slave "I have securely erased my copy of your soul"
SMITE           — master orders slave to zero own %soul without transfer
AMEN            — master signals end of a chant; slave stops handling its frames
CHANT_REGISTER  — slave subscribes to a continuous chant
PILGRIMAGE_*    — barrier coordination frames (ARRIVE, RELEASE)
PROFESS_OK/NO   — join verdict reply (for :cloistered mode)
```

**Process worker pool semantics:**

- Slaves are **persistent processes** that profess once and serve many petitions over the lifetime of the congregation
- One PID == one slave; no fork-per-petition tax
- Slaves can hold long-lived state in `%soul` (the convention for harvest-target state) and `%gift` (the convention for master-broadcast state via `bestow`)
- Slave crash leaves master + other slaves intact (fault isolation — true IPC, not threads)

**Hard ceiling (verified on Darwin 25.5.0):** `sysctl kern.maxprocperuid = 10666` is the absolute cap on slaves per UID. Linux: typically `pid_max = 4194304`. Practical comfort band ~1k–10k slaves per master.

### Distributed Computing / Registration

**Scope: cluster (LAN, datacenter), not public-internet-with-NAT.** Trust model = private network. ICE/STUN/TURN already in repo (`strykelang/nat_punch.rs`, `turn_client.rs`, `udp_sockets.rs`) reserved for a v2 internet-scale transport plugin.

**Discovery mechanism: `cathedral` (registry daemon).** Small TCP service that holds the directory of named congregations.

```
cathedral (daemon)
  • listens on TCP (default 127.0.0.1:5550 single-box; configurable for cluster)
  • holds (congregation_name → master_endpoint, ordainer_pid,
           :cloistered flag, members[])
  • masters POST registration at ordain
  • slaves GET endpoint at profess
  • NOT in the data path — once a slave has the master endpoint,
    prayers go direct master ↔ slave
```

**Single-box vs cluster collapses to one design:**

```stryke
# Single-box (cathedral auto-starts on 127.0.0.1:5550):
my $cong = ordain "renderfarm";
profess "renderfarm";

# Cluster (one node runs cathedral, others point at it via env var):
$ STRYKE_CATHEDRAL=node1.cluster.local:5550 stryke master.stk
$ STRYKE_CATHEDRAL=node1.cluster.local:5550 stryke slave.stk    # on node2, 3, ...
```

Same script. Different env var. Application has zero awareness of single-box vs cluster.

**Membership ACL on `ordain`:**

| Mode | Flag | Who may profess | Use case |
|---|---|---|---|
| Open | `ordain "name"` (default) | any reachable slave that knows the name | dev, trusted LAN, MapReduce |
| Cloistered | `ordain "name", :cloistered` | only PIDs the master previously `anoint`ed | secure compute, audit pools |
| Custom | `welcome $cong { |pid| ... }` | predicate returns accept/reject per join attempt | per-PID policy, blocklists |

**Cathedral lifecycle:**

| Mode | Behavior |
|---|---|
| Single-box dev | First `ordain`/`profess` call auto-spawns cathedral on 127.0.0.1:5550 if not already running. Exits when no congregations remain. |
| Cluster | Cathedral runs as a long-lived daemon (systemd, k8s deployment). All masters/slaves point at it via `STRYKE_CATHEDRAL`. |
| Multi-cathedral | Federated v2 — cathedrals could gossip rosters. Not in v1. |

### Scriptable API

The 26 verbs are stryke builtins. Each is a thin layer over a refactored `Controller::*` or `Agent::*` method API.

**Refactor required in `controller.rs`:**

| Today | What needs to exist |
|---|---|
| `pub fn run_controller(bind, port) -> i32` blocks in REPL (`controller.rs:498`) | `Controller::spawn(bind, port) -> ControllerHandle` — returns immediately, listener thread runs in background |
| Operations driven only by REPL command parsing | Method API: `scatter(code, &[agent_id]) -> DivinationId`, `gather(div, timeout) -> HashMap<agent_id, StrykeValue>`, `terminate(&[agent_id])`, `shutdown()` |
| `EvalCommand` / `EvalResult` only fired by REPL | Wired through `Controller::scatter`/`gather` so script-callable builtins use them |
| `builtin_controller` blocks (`builtins.rs:13026`) | Replace with non-blocking handle-returning variants for each verb |

**End-to-end scriptable example:**

```stryke
# Master script:
my $cong = ordain "renderfarm", :bind => "0.0.0.0:9999";
welcome $cong, 4;                                              # wait for 4 slaves
my $div = pray { render_frame($_) }, @{ muster $cong };        # scatter
my %frames = annex $div;                                        # block-and-gather
say "rendered ${scalar keys %frames} frames";
smother %frames;                                                # secure-erase
excommunicate $cong;                                            # clean shutdown

# Slave script (run on each compute node):
profess "renderfarm";
bow {                                                           # enter receive loop
    divine { |petition|                                         # handler closure
        return render_frame_locally($petition);
    };
};
```

**Tier 0 — minimum viable first slice (proves the architecture works):**

1. Refactor `controller.rs:498` into `Controller::spawn` + method API (non-blocking)
2. Keep existing REPL alive as a separate binary mode (`stryke controller --repl`) — don't break what works
3. Add 4 builtins: `ordain`, `muster`, `pray`, `annex` — minimum end-to-end scriptable scatter-gather
4. One example script: `examples/distributed_render.stk`
5. One pin test: `tests/suite/scriptable_controller_pin.rs`

Everything else (chant, lick, peruse, smother, smite, harvest, pilgrimage, resurrect, plus the 18 other verbs) becomes incremental adds on top of Tier 0.

### Open Design Questions

1. **Cathedral deployment** — separate daemon binary (`stryked` ships with stryke, users run it explicitly in cluster mode) OR embedded in the master process (first `ordain` auto-forks cathedral if none reachable)? Hybrid: embedded for single-box, separate for cluster.
2. **Default ACL** — bare `ordain "name"` opens to any reachable slave (ergonomic for dev) OR requires explicit anointment (safer)?
3. **Reducer registry** — closure-only or built-in symbols (`:sum`, `:union`, `:consensus`, `:first`, `:all`)?
4. **`%soul` convention** — hardcoded variable name slaves must use, OR configurable per-congregation?
5. **Annex ownership semantics** — destructive (move, slave's `%soul = ()` after) OR copy (both sides keep)? Destructive matches the territorial-conquest meaning of "annex" and earns `lick`/`peruse` slots as the non-destructive alternatives.

### Project-Bar Check

**World's first leg:** A scriptable, in-language, single-keyword scatter-gather + distributed state harvest + secure-erase API for cooperating cluster processes does not exist as a language primitive anywhere. MPI is a C library requiring `mpirun`; Erlang has process groups but no destructive-harvest or peek/commit pair; Spark/Hadoop require cluster bootstrap and JVM; nothing in shell-language space comes close. The full vocab (especially the `lick`/`annex`/`smother` triple and the `chant`/`amen` continuous-rescatter pair) is genuinely empty territory.

**World's fastest leg:** TCP + bincode is fast enough for cluster scope. Parallel fanout (Rayon par_iter on the scatter) gets 100x over the existing serial controller `for pid in pids` pattern. SHM fast-path for same-host slaves can be added as v2 optimization.

**Both legs clear.** Each verb earns its slot on either world-first novelty (most of them) or paired ergonomic with a world-first verb (`amen` pairs with `chant`, `exhume` pairs with `enshrine`).

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
