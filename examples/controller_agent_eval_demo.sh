#!/usr/bin/env bash
# Runnable demo: `stryke controller` + 2 × `stryke agent` + the `@CODE` /
# `eval CODE` REPL verb.
#
# What you'll see:
#   * `status` listing two connected agents (node-01, node-02)
#   * `@1+41`  — one-shot eval ships to both agents IN PARALLEL
#   * `@sub greet { ... } greet()`  — defines a sub on each agent's VM
#   * `@$main::counter += 5; $main::counter`  — cross-frame state persists
#   * `@p "a"; p "b"; p "c"; 0`  — multi-line output, each line is tagged
#     `[agent-name/ok]` so the output stays grep-able by agent
#   * `@die "boom"`  — error path tags lines with `[agent-name/ERR]`
#
# All output flows through the controller process's stdout. Each
# `[name/tag]` row is the agent's reply; output ordering is alphabetical by
# agent name (sorted before dispatch so transcripts diff cleanly between
# runs — HashMap iteration order would shuffle otherwise).
#
# Run:
#   ./examples/controller_agent_eval_demo.sh
#
# Cleanup happens via trap: controller, agents, temp config files, and the
# control fifo are all removed on exit (normal or interrupted).

set -euo pipefail

PORT=${PORT:-9999}
CTRL_FIFO=$(mktemp -u -t stryke_ctrl_in.XXXXXX)
AGENT_01_TOML=$(mktemp -t stryke_agent_01.XXXXXX)
AGENT_02_TOML=$(mktemp -t stryke_agent_02.XXXXXX)

mkfifo "$CTRL_FIFO"

cleanup() {
    # Send shutdown if the fifo writer is still open (script interrupted before
    # the explicit `shutdown` line below ran).
    if { exec 3>&-; } 2>/dev/null; then :; fi
    # Kill anything we spawned in this process group.
    pkill -P $$ 2>/dev/null || true
    rm -f "$CTRL_FIFO" "$AGENT_01_TOML" "$AGENT_02_TOML"
}
trap cleanup EXIT

# Agent config files (agent reads its name from the TOML — there is no
# `--agent-name` CLI flag at the moment, so a one-line TOML per agent is the
# canonical way to give each instance a distinguishable name).
cat > "$AGENT_01_TOML" <<EOF
[controller]
host = "127.0.0.1"
port = $PORT

[agent]
name = "node-01"
EOF
cat > "$AGENT_02_TOML" <<EOF
[controller]
host = "127.0.0.1"
port = $PORT

[agent]
name = "node-02"
EOF

echo "▶ starting controller on 127.0.0.1:$PORT" >&2
stryke controller --bind 127.0.0.1 --port "$PORT" < "$CTRL_FIFO" &
# Hold the fifo's writer side open in THIS script via fd 3 — otherwise the
# pipe EOFs as soon as the heredocs above finish, the controller's REPL sees
# EOF on stdin, and exits before we get to send any commands.
exec 3> "$CTRL_FIFO"

sleep 1   # let controller bind

echo "▶ starting node-01 and node-02 agents" >&2
stryke agent -c "$AGENT_01_TOML" &
stryke agent -c "$AGENT_02_TOML" &

sleep 1   # let agents connect + handshake

# ── REPL transcript ───────────────────────────────────────────────────────

# Each `>&3` writes one line into the controller's stdin via the fifo.
# `sleep` between lines just keeps the printed output legible — the protocol
# itself doesn't need pacing.

cat <<'NOTE' >&2
▶ sending commands via the controller REPL. Output below is the controller's
  stdout; lines starting with [name/ok|ERR] are agent replies.
NOTE

echo "status" >&3 ; sleep 1
echo "@1 + 41" >&3 ; sleep 1
echo '@sub greet { "hello from " . $main::ENV{USER} } greet()' >&3 ; sleep 1
echo '@$main::counter = 0' >&3 ; sleep 1
echo '@$main::counter += 5; $main::counter' >&3 ; sleep 1
echo '@p "a"; p "b"; p "c"; 0' >&3 ; sleep 1
echo '@die "boom"' >&3 ; sleep 1
echo "shutdown" >&3

wait
