# Expect-style Interactive Automation for Stryke

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
