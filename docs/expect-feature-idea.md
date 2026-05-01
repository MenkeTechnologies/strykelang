# Expect-style Interactive Automation for Stryke — **PHASES 1-4 SHIPPED**

**Status:** PTY/expect runtime is fully wired in `strykelang/perl_pty.rs`. `pty_spawn`, `pty_send`, `pty_read`, `pty_expect`, `pty_expect_table`, `pty_buffer`, `pty_alive`, `pty_eof`, `pty_close`, `pty_interact` all dispatch through `builtins.rs`. Method-form sugar (`PtyHandle::spawn`/`->expect`/`->send`/`->branch`/`->interact`/`->close`) lives in `examples/` — `require "perl_pty_class.stk"` in your script. Phase 5 (cluster fanout polish + Windows ConPTY) remains deferred.

## The Gap

Tcl/Expect is still the best tool for automating interactive CLI sessions. Nothing has really replaced it. Stryke has cluster dispatch for running commands on remote hosts, but not the interactive spawn/expect/send loop.

## Use Cases

- SSH into a box, wait for password prompt, send password, wait for shell, run commands
- Automate interactive installers
- Script CLI tools that prompt for input
- Interactive database clients (psql, mysql, redis-cli)
- Network equipment (routers, switches that use interactive CLIs)
- Any REPL-style interaction

## Proposed Syntax (terse, stryke-style)

```stryke
# Basic spawn/expect/send
my $s = spawn "ssh user@host"
$s.expect(/password:/)
$s.send("hunter2\n")
$s.expect(/\$/)
$s.send("ls -la\n")
my $output = $s.expect(/\$/)
$s.close

# Or more golf-friendly
my $s = spawn "ssh user@host"
$s ~ /password:/ >> "hunter2\n"
$s ~ /\$/ >> "uptime\n"
p $s ~ /\$/

# Thread-macro style?
t spawn("ssh host") expect(/pass:/) send("pw\n") expect(/\$/) send("ls\n") expect(/\$/) close
```

## Core Primitives

| Function | Description |
|----------|-------------|
| `spawn CMD` | Start process with PTY, return handle |
| `expect REGEX` or `~ REGEX` | Wait for pattern in output, return matched text |
| `expect REGEX, TIMEOUT` | With timeout (seconds) |
| `send STR` or `>> STR` | Send string to process stdin |
| `interact` | Hand control to user (for debugging) |
| `close` | Kill process, clean up |

## Timeout Handling

```stryke
$s.expect(/ready/, 30) or die "timeout waiting for ready"

# Or with timeout block
timeout 30 {
    $s.expect(/ready/)
} or {
    die "gave up"
}
```

## Multiple Patterns (like Expect's expect block)

```stryke
$s.expect(
    /password:/ => { $s.send("pw\n") },
    /yes\/no/   => { $s.send("yes\n") },
    /\$/        => { "got shell" },
    timeout(30) => { die "timed out" },
)
```

## Enterprise Value

This slots directly into the distributed load testing / infrastructure automation story:

- Automate SSH key deployment across cluster
- Script interactive provisioning tools
- Handle MFA prompts programmatically
- Automate legacy systems with interactive CLIs
- Network device configuration at scale

Combined with cluster dispatch:
```stryke
# Run interactive automation across 100 hosts in parallel
pfor @hosts -> $h {
    my $s = spawn "ssh $h"
    $s ~ /password:/ >> $passwords{$h}
    $s ~ /\$/ >> "apt update && apt upgrade -y\n"
    $s ~ /\$/ >> "exit\n"
    $s.close
}
```

## Implementation Notes

- Need PTY allocation (Rust `pty` crate or similar)
- Non-blocking reads with regex matching
- Handle ANSI escape sequences (terminal control codes)
- Buffer management for partial matches
- Process lifecycle management

## Why This Matters

Expect is from 1990 and still has no real successor. Python has `pexpect` but it's clunky. Nothing matches Expect's ergonomics.

Stryke could be the first language with native expect-style primitives in terse, golf-friendly syntax. Combined with the existing cluster dispatch, it's a complete infrastructure automation story.

---

## Phases 1–4 — SHIPPED

Status: Unix-only. ConPTY (Windows) deferred. Implementation lives in
`strykelang/perl_pty.rs` (~700 LoC) plus a thin wrapper class at
`strykelang/perl_pty_class.stk` (~40 LoC of stryke).

### Final syntax (NOT the operator-overloaded proposal)

The original sketch with `~` for expect and `>>` for send was rejected
because the parser already triple-uses `~` (BitNot, range separator,
char-index suppression via `parser.rs:131 suppress_tilde_range`) and
`>>` is bitwise shift + open-mode token. Adding a fourth meaning would
add another parser counter and a typo trap with `~>` (thread-macro).

Bare-builtin form (canonical):

```stryke
my $h = pty_spawn("ssh user@host")
pty_expect($h, qr/password:/, 30)
pty_send($h, "hunter2\n")
pty_expect($h, qr/\$ /, 30)
pty_send($h, "ls -la\n")
my $output = pty_expect($h, qr/\$ /, 30)
pty_close($h)
```

Method form (sugar via `class PtyHandle`):

```stryke
require "perl_pty_class.stk"
my $h = PtyHandle::spawn("ssh user@host")
$h->expect(qr/password:/, 30)
$h->send("hunter2\n")
$h->close()
```

Table form (the actually-novel ergonomic — Tcl's `expect { ... }`
block, in stryke):

```stryke
my $tag = $h->branch([
    +{ re => qr/password:/, do => sub { $h->send($pw . "\n"); "got_pass" } },
    +{ re => qr/yes\/no/,   do => sub { $h->send("yes\n");  "got_confirm" } },
    +{ re => qr/denied/,    do => sub { die "auth failed" } },
], 30)
```

### Phase 1 — `pty_spawn` / `pty_send` / `pty_read` / `pty_close`

`nix::pty::openpty()` + raw `fork()` + `setsid` + `ioctl(TIOCSCTTY)` +
`dup2` + `execvp` for the child. Parent gets the master fd
non-blocking. Master/slave wrapped in a global registry keyed by
`__pty_id__`. SIGTERM → 200ms wait → SIGKILL on close.

### Phase 2 — `pty_expect(handle, qr/.../, timeout_secs)`

Timeout-aware loop: try regex on accumulated buffer, else `select()`
with remaining budget, drain non-blocking into buffer, retry. Returns
matched substring on hit, `undef` on timeout/EOF. Strips Perl-style
`(?^:...)` qr-stringification before compiling via the `regex` crate.

### Phase 3 — `pty_expect_table(handle, [+{re=>…, do=>…}], timeout)`

Walks every branch on every read tick; first match wins. Returns
`+{matched=>"...", action=>$cv}` so the wrapper class can either run
the closure (method form) or hand the pair back as-is (bare builtin
form). Avoids calling stryke from inside a free Rust builtin.

### Phase 4 — `pty_interact(handle)`

`tcgetattr` save → `cfmakeraw` apply (only when stdin is a tty) →
`select()` on stdin + master with no timeout → forward both
directions until either side EOFs or stdin sees `0x1d` (Ctrl-]).
Restores tty mode on exit. Verified to exit cleanly when stdin is
piped (no tty) so it works in unattended test harnesses.

### Smoke

```
P1 (spawn/send/expect/close): ok (phase1)
P2 (timeout):                 ok
P3 (table form):              ok (pass_branch)
P4 (interact graceful):       ok
```

Run via `stryke` against a fake-login shell script (`username:` →
`password: hunter2` → `welcome, $u`); cluster fanout via `pfor` lands
in Phase 5.

### Phase 5 — cluster + Windows — ⏳ PARTIAL

- ⏳ ConPTY (Windows) — separate code path, ~1 week. Skipped per project rule (Unix-only).
- ⏳ `pfor @hosts -> $h { my $p = pty_spawn("ssh $h"); ... }` benchmark vs Ansible — `pmap_on cluster` already works today with `pty_spawn` inside the body, so cross-host PTY automation is functional via the cluster surface; the deferred work is the dedicated benchmarking pass.
- ✅ `pty_after_eof($h, "callback_name")` async reaper — spawns a watcher thread that flips a fired flag once the PTY closes. Drain via `pty_pending_events()` in the main loop.
- ✅ ANSI strip (`pty_strip_ansi $text`) — VT100/xterm CSI/OSC/ESC removal. Pure logic; not a full terminal emulator but covers prompts, banners, and progress bars.

---

## Session Context (2026-04-26)

This came out of a "what killer features do other languages have that Stryke doesn't?" brainstorm. Most features from other languages are already covered:

- Array ops (APL-ish) ✓
- Field splitting (awk-ish) ✓  
- Coroutines/async ✓
- Thread macro (Clojure) ✓
- Shell integration ✓
- Process control ✓
- Pattern matching ✓
- Distributed cluster dispatch ✓

Expect-style interactive automation is the one clear gap that's still genuinely useful and missing.

## Also Completed This Session

- Polymorphic ranges with step (world first — 10 types):
  - Integers, Strings, Floats, Roman numerals, ISO dates, Year-months, Times (HH:MM), Weekdays, Month names, IPv4 addresses
  - All support forward AND reverse with custom step
  - `"I":"X":1`, `"X":"I":-1`, `"2026-01-01":"2026-12-31":7`, `"Mon":"Fri":1`, etc.

- Magic string decrement (world first — Perl only has increment)
  - `"z":"a":-1` works

- Module import shadowing rules fixed
  - Modules can shadow builtins
  - User code cannot (unless --compat)
  - --no-interop blocks all shadowing
