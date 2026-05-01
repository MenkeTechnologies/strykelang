# Stryke AI Primitives — Design Doc

> *`ai` is to stryke what `print` is to every other language: a builtin, two letters, ubiquitous, unlimited power.*

Stryke is designed as an AI-native language. AI is not a library, not a framework, not a third-party crate — it is a primitive of the language, the same way `print`, regex, and arrays are primitives. The dream pipeline:

```stryke
~> "summarize codebase into BOOK.pdf then open it" ai
```

That single expression: an agent loop that reads the codebase, generates a structured summary, renders it to PDF, and hands the file to the OS to open. No imports. No SDK setup. No `langchain` boilerplate. The language *is* the framework.

## Design Principles

1. **`ai` is a builtin, not a library.** No imports. Always available. Two letters because it gets typed thousands of times per program.
2. **Short and sweet, unlimited power.** The simple form is `ai $prompt`. The complex form composes from the same primitive — no separate "advanced API."
3. **Tools are functions.** Any stryke function with `tool` in its declaration becomes available to the agent loop with zero extra ceremony. Signature → JSON schema, docstring → description, function body → tool implementation.
4. **MCP-native.** Connecting to an MCP server is one line. Exposing your own MCP server is a single block.
5. **Provider-agnostic, Anthropic-first.** The same `ai` call hits Claude, GPT, Gemini, or a locally-linked llama.cpp model. Provider chosen by config, swappable at runtime.
6. **Local fallback always works.** Stryke binaries can ship with a small quantized model linked statically. `ai` works offline; quality scales with the configured backend.
7. **Cost-aware by default.** Caching, batching, parallelism, hard cost ceilings — built into the runtime, not bolted on.
8. **Deterministic in tests.** `ai_mock` blocks freeze responses for unit tests; flakiness is a config bug, not a fact of life.
9. **Composes with the rest of stryke.** Web framework handlers, package manager, cluster dispatch, effects (future), capabilities (future) — all touch the same `ai` primitive.

## The `ai` Builtin

Three forms, same primitive underneath:

```stryke
ai "summarize this", $document                # function call
~> $document ai "summarize this"              # thread macro (stryke design lineage)
$document |> ai "summarize this"              # pipe
```

Default semantics:

- Argument is a prompt string (and optional context value).
- All in-scope `tool fn` declarations are auto-registered as tools.
- All connected MCP servers are auto-attached.
- Agent loop runs to completion: tool call → tool result → next call → ... → final answer.
- Returns: `Str` in scalar context, `Stream<Str>` in iter context, typed value when assigned to a typed binding.

```stryke
my $r : Str = ai "what is 2+2"                # "4"
for my $chunk in ai "write a story" { ... }   # streaming
my $book : Book = ai "extract info", $pdf     # auto-schema from type, validated parse
```

Configuration (defaults documented; everything overridable per call):

```stryke
ai "summarize", $doc,
    model: "claude-opus-4-7",
    system: "You are concise.",
    max_turns: 10,
    cache: true,
    timeout: 30
```

## AI Collection Builtins

Treating AI as a control-flow primitive, not just an API call:

```stryke
@docs.ai_filter "is about cooking"
@articles.ai_map "summarize in one sentence"
@candidates.ai_sort by: "relevance to backend engineering"
@items.ai_classify into: ["urgent", "normal", "ignore"]
@names.ai_dedupe "treat misspellings as the same person"

if $email ai_match "spam" { discard }
elsif $email ai_match "urgent customer support" { route_to_team }
```

Each of these compiles to a single batched LLM call across the collection where possible (one prompt, list of items, list of judgments back). Cost-conscious by construction — `ai_filter @[1000]` is one call, not 1000 calls.

Use sparingly. Each call costs money. The compiler tracks predicted cost statically when constants are involved and warns on hot loops. `ai_*` inside a `for` loop in production code is a lint.

## Lower-Level AI Builtins

When the agentic `ai` is too much, drop down:

```stryke
my $r = prompt "explain quantum", model: "claude-opus-4-7"            # single shot, no tools
my $r = stream_prompt "write a story"                                  # streaming generator
my @vec = embed "hello world"                                          # single embedding
my @vecs = embed @docs                                                 # batched
my $resp = chat $messages, model: "claude-opus-4-7"                    # explicit message list
```

These are the building blocks `ai` itself is built on. Available when you want explicit control over the LLM interaction.

## Tool Functions

Mark a function as agent-callable with `tool`:

```stryke
tool fn weather($city: Str) -> Str "Get current weather for a city" {
    fetch "https://api.weather.com/" . uri_encode($city)
}

tool fn search_kb($query: Str, $limit: Int = 10) -> List<Doc> "Search the knowledge base" {
    sql%{ SELECT * FROM kb WHERE content @@ to_tsquery($query) LIMIT $limit }
}
```

At build time:

1. Signature → JSON schema (parameters with types and constraints).
2. Docstring → tool description.
3. Function → invocable tool entry.
4. All `tool fn` definitions in the current scope are auto-registered.

Compare to current TypeScript/Python practice (define function → re-define schema by hand → re-define description in the schema → wire into `tools=[...]`): stryke collapses that ritual to one declaration.

## MCP Servers (Declarative)

Expose stryke functions as an MCP server with one block:

```stryke
mcp_server "filesystem" {
    transport :stdio                       # or :ws port: 3000, :http port: 3000

    tool read_file($path: Str) -> Str        "Read file contents" {
        slurp $path
    }

    tool list_dir($path: Str) -> List<Str>   "List directory entries" {
        readdir $path
    }

    resource "file://*" -> Str {
        slurp $self.uri.path
    }

    prompt "summarize"                       "Summarize text" {
        args $text: Str
        "Summarize this concisely:\n#{$text}"
    }
}
```

Compiles to a spec-compliant MCP server. The `s build --mcp-server` flag emits a standalone binary exposing the server, separate from your main app binary.

## MCP Clients

Connect, discover, call:

```stryke
my $fs = mcp_connect "stdio:/usr/local/bin/fs-mcp"
my $gh = mcp_connect "https://api.github.com/mcp"
my $pg = mcp_connect "ws://localhost:9000"

my @tools     = $fs.tools
my @resources = $fs.resources
my @prompts   = $fs.prompts

my $contents = $fs.tool(:read_file, path: "/tmp/x")
my $config   = $fs.resource("file:///etc/app.toml")
my $msg      = $fs.prompt(:summarize, text: $long_text)
```

Connected MCP servers are visible to subsequent `ai` calls automatically — no re-registration step.

## Agents (Composed)

For explicit control over the agent loop:

```stryke
my $agent = ai_agent
    .mcp("filesystem", "stdio:/usr/local/bin/fs-mcp")
    .mcp("github",     "https://api.github.com/mcp")
    .tools(&internal_search, &slack_post)
    .system("You are a senior backend engineer reviewing changes")
    .max_turns(20)
    .max_cost_usd(0.50)

my $review = $agent.run("review PR 4523 against our coding standards")
```

The bare `ai $prompt` is `ai_agent.run($prompt)` with sensible defaults. Same primitive, different ergonomics.

## Provider Architecture

Configuration in `stryke.toml`:

```toml
[ai]
provider     = "anthropic"             # "anthropic" | "openai" | "google" | "local"
model        = "claude-opus-4-7"
api_key_env  = "ANTHROPIC_API_KEY"
cache        = true
max_cost_run = 1.00                    # USD hard ceiling per program run
fallback     = "local"                 # used when API unreachable

[ai.local]
model_path   = "embedded:claude-haiku-quantized"
threads      = "all"
gpu          = "auto"

[ai.openai]
api_key_env  = "OPENAI_API_KEY"
model        = "gpt-4o"

[ai.routing]
embed        = "anthropic"             # different providers per operation
classify     = "local"                 # cheap ops go local
```

The runtime picks the provider per call based on config. All providers expose a uniform interface; provider-specific extensions (Anthropic prompt caching, OpenAI streaming function calls, etc.) are accessible through provider-namespaced options when needed.

**Local fallback** uses a llama.cpp-equivalent linked statically into the binary. The default ships a small quantized model so `ai` always works, even with no API key, even offline. Quality scales when a remote provider is configured.

## Cost & Latency

| Concern | Mechanism |
|---|---|
| Repeated identical calls | Result cache keyed on `(provider, model, prompt, system, tools, params)` |
| Repeated system prompts | Provider-side prompt caching where supported (Anthropic) |
| Many small calls | Automatic batching for `ai_map`/`ai_filter`/`ai_classify` |
| Streaming UX | `stream_prompt` / `ai` returns `Stream<Str>` in iter context |
| Parallelism | `@docs.pmap |$d| { ai "summarize", $d }` runs N in parallel up to provider rate limit, automatic backpressure |
| Cost ceiling | `max_cost_run` aborts the program before an expensive call |
| Cost introspection | `ai_cost` returns running USD spent in current scope |
| Token estimation | `tokens_of($text)` for pre-flight token counts |

## Determinism in Tests

```stryke
test "summarize trims to 50 words" {
    ai_mock {
        prompt "summarize", _ => "Lorem ipsum dolor sit amet, consectetur..."
    } {
        is words(summarize($doc)).count, 50
    }
}
```

`ai_mock` intercepts every AI primitive in scope. Patterns match prompts (regex, exact, glob, predicate); responses can be strings, structured values, or generator functions. Tests are deterministic, fast, and free.

CI runs `s test` with `STRYKE_AI_MODE=mock-only` set — any unmatched live AI call fails the build. Live AI in tests requires `STRYKE_AI_MODE=live` explicitly.

## Composition with the Rest of Stryke

**With the web framework:**

```stryke
class ChatController < Controller {
    fn stream() {
        sse_stream { |stream|
            for my $chunk in stream_prompt $params.prompt {
                stream.send($chunk)
            }
        }
    }

    fn ask() {
        my $answer = ai $params.q,
            tools: [&search_db, &fetch_docs],
            system: "You are our product expert."
        render :json, answer: $answer
    }
}
```

**With the package manager:**

A package can mark itself as MCP-exposable:

```toml
# stryke.toml
[mcp]
expose_module = "lib::api"             # all `tool fn`s in this module become MCP tools
```

```bash
s build --mcp-server                    # → target/release/myapp-mcp (standalone server)
s build --release                       # → target/release/myapp (regular app)
```

Every stryke library can publish itself as an MCP server with one flag.

**With cluster dispatch:**

```stryke
my @summaries = cluster_dispatch @docs |$d| { ai "summarize", $d }
```

AI calls fanout across cluster nodes; each node uses its own provider config; results aggregate back. Combined with the cost ceiling, this is rate-limit-aware distributed AI work.

**With effects (when shipped):**

`ai` becomes `Effect::AI`. Effect handlers control model/cache/retry/cost in one place:

```stryke
handle Effect::AI |op, k| {
    log "ai call:", op.prompt
    when op.cost_estimate > 0.10 { return k(cached_or_skip(op)) }
    return k(default_handler(op))
} {
    ai "summarize", $doc
    ai "translate to French", $r
}
```

**With capabilities (when shipped):**

`ai` requires `AICap`. A library can't make AI calls unless given the capability:

```stryke
fn process_doc($doc, $ai_cap: AICap) {
    ai_cap.run "summarize", $doc
}
```

Stops compromised packages from quietly running up an LLM bill.

## Implementation Phases

### Phase 0 — Walking Skeleton — **SHIPPED**

Lives in `strykelang/ai.rs`. Wired through `builtins.rs`. Builtins:

| Name | Status |
|---|---|
| `ai($prompt, opts...)` / `prompt($prompt, opts...)` | Single-shot, no tools yet (agent loop is Phase 1) |
| `stream_prompt($prompt, opts)` | Returns full text in v0; real `Stream<Str>` is Phase 5 |
| `chat($messages, opts...)` | Message-list with role=system/user/assistant |
| `embed($text)` / `embed(@texts)` | Voyage AI default, OpenAI alt |
| `tokens_of($text)` | char/4 heuristic (good-enough pre-flight) |
| `ai_cost()` | `+{usd, input_tokens, output_tokens, embed_tokens, cache_hits, cache_misses}` |
| `ai_cache_clear()` / `ai_cache_size()` | In-process result cache, sha256-keyed on `(provider, model, system, prompt)` |
| `ai_mock_install($pattern, $response)` / `ai_mock_clear()` | Regex-keyed mock interceptor; first-match-wins |
| `ai_config_get($key)` / `ai_config_set($key, $val)` | Read/write of the loaded `[ai]` table |
| `STRYKE_AI_MODE=mock-only` | Errors any unmocked call — for `s test` |

Providers actually wired: **Anthropic**, **OpenAI** (Messages + Chat
Completions), **Voyage** + **OpenAI** for embeddings. TOML config from
`./stryke.toml`; falls back to env vars. Pricing table embedded for
the 4 major model families so `ai_cost()` is meaningful without a
provider invoice round-trip.

What's intentionally NOT in Phase 0:
- Tool / agent loop (`tool fn` keyword needs parser work — Phase 1)
- MCP client / server (Phase 2)
- Collection builtins (`ai_filter`, `ai_map`, …) (Phase 3)
- llama.cpp local backend (Phase 4)
- Real `Stream<Str>` streaming (Phase 5)
- Auto-tool-attachment of MCP servers (Phase 2 prerequisite)

### Phase 1 — Agent loop — **SHIPPED (without `tool fn` keyword)**

The agent loop runtime is in place. Without parser work for the `tool
fn` declaration, tools are passed explicitly as a hashref list:

```stryke
my $report = ai "research X across our docs and Hacker News",
    tools => [
        +{ name        => "kb_search",
           description => "Search internal knowledge base",
           parameters  => +{ q => "string", limit => "int" },
           run         => sub { search_kb($_[0]->{q}, $_[0]->{limit}) } },
        +{ name        => "fetch_url",
           description => "Fetch a URL and return text",
           parameters  => +{ url => "string" },
           run         => sub { fetch($_[0]->{url}) } },
    ],
    max_turns => 10,
    system    => "You are a senior engineer."
```

Bare `ai $prompt` keeps the Phase 0 single-shot semantics; `ai $prompt,
tools => [...]` auto-routes to the agent loop. Both Anthropic
(`tool_use`/`tool_result`) and OpenAI (`tool_calls`/`tool` role)
protocol shapes are wired. Mock mode short-circuits the loop and
returns a mocked final string for tests.

`tool fn name(...) -> Type "doc" { ... }` (auto-schema from signature
+ docstring, auto-registration of all in-scope tools) is the parser
extension — still Phase 1 to-do.

### Tool registry (Phase 1 sugar) — **SHIPPED**

For people who don't want to wait on the `tool fn` parser keyword,
register tools at runtime:

```stryke
ai_register_tool(
    "weather", "Get weather for a city",
    +{ city => "string" },
    sub { fetch("https://api.weather.com/" . uri_encode($_[0]->{city})) }
);

# Bare `ai($prompt)` now auto-routes to the agent loop and sees this tool:
my $r = ai("what's the weather in SF?");
```

| Builtin | Behavior |
|---|---|
| `ai_register_tool($name, $desc, +{params}, sub { ... })` | Add an always-on tool; idempotent re-register |
| `ai_unregister_tool($name)` | Remove |
| `ai_clear_tools()` | Wipe registry |
| `ai_tools_list()` | Inspect what's registered |

Bare `ai($prompt)` auto-routes to the agent loop when registered tools
are non-empty OR `tools => [...]` is passed OR `auto_mcp => 1` (default)
and an MCP server is attached.

### Memory / RAG — **SHIPPED**

Sqlite-backed embedding store. Save text + embedding, recall by cosine
similarity. In-memory by default; persistent with `path => "memory.db"`.

| Builtin | Behavior |
|---|---|
| `ai_memory_save("id", "content", $metadata?, $path?)` | Embed + insert (idempotent on id) |
| `ai_memory_recall("query", top_k => N)` | Re-embed query, return top-k by cosine |
| `ai_memory_forget("id")` / `ai_memory_count()` / `ai_memory_clear()` | Maintenance |

Mock-mode hash-embeds deterministically so tests round-trip without
the network. Verified: 4 docs saved, query "fast systems-level" picks
"Stryke is the fastest Perl-5 interpreter" at score 0.7679 over the
other three.

### Streaming with `on_chunk` — **SHIPPED**

Real Anthropic SSE parsing. Pass `on_chunk => sub { … }` and the
callback fires once per delta chunk; the full text is also returned at
the end:

```stryke
my $state = +{ buf => "" };
my $full = stream_prompt("write a haiku",
    on_chunk => sub { $state->{buf} .= $_[0]; print $_[0] }
);
```

Stryke gotcha: closures capture *scalars* by value. Mutate state
through a hashref/arrayref (heap-shared) so the outer scope sees it.
String concat on `my $buf` inside the closure won't propagate; pushing
into `@{$state->{chunks}}` will.

### Structured output — **SHIPPED**

```stryke
my $r = ai("Extract user info from: Alice, 30, active",
    schema => +{ name => "string", age => "int", active => "bool" });
# $r is a hashref: { name => "Alice", age => 30, active => 1 }
```

`ai($p, schema => +{...})` auto-routes to `ai_extract`. The schema hashref
maps field names to coercion types (`string`/`int`/`number`/`bool`/`array`/
`object`). Builds a JSON-only prompt, walks the response for the first
balanced `{...}`, parses, validates + coerces to the schema. Returns a
real stryke hashref ready for field access.

### Anthropic prompt caching — **SHIPPED**

```stryke
my $r = ai("question", system => $long_system_prompt, cache_control => 1);
# Subsequent calls with the same system block read from cache at ~10%
# of normal input cost.
```

The runtime sets `cache_control: { type: "ephemeral" }` on the system
block when `cache_control => 1` is set. `ai_cost()` now also returns
`cache_creation_tokens` / `cache_read_tokens` so spend is accurate
(creation +25%, reads -90% vs normal input).

### Extended thinking — **SHIPPED**

```stryke
my $answer = ai("hard math problem",
    thinking => 1, thinking_budget => 8000);
my $reasoning = ai_last_thinking();   # full thinking trace
```

When `thinking => 1` is set the request includes the Anthropic
extended-thinking block; the model's reasoning is captured separately
from the answer and surfaced via `ai_last_thinking()`.

### PDF / document input — **SHIPPED**

```stryke
my $summary = ai("summarize this contract", pdf => "/path/to/contract.pdf");
my $extract = ai("extract terms",        pdf => "https://example.com/doc.pdf");
my $direct  = ai("read",                  pdf => $raw_bytes);
```

`ai($p, pdf => $path|$url|$bytes)` auto-routes to `ai_pdf`, which builds
an Anthropic `document` content block (base64-inlined PDF, up to
32MB / 100 pages).

### Scoped budget — **SHIPPED**

```stryke
ai_budget(0.50, sub {
    my @summaries = ai_map(\@long_docs, "summarize");
    my @ranked = ai_sort(\@summaries, "by relevance to backend engineering");
    return \@ranked;
});
# Errors if total spend during the block exceeds $0.50.
```

Per-block USD cap. Enforces by snapshotting current cost on entry,
raising the global ceiling to `snapshot + cap` for the duration, and
checking spend on exit. Restores the prior global cap unconditionally.

### Convenience wrappers — **SHIPPED**

| Builtin | Behavior |
|---|---|
| `ai_summarize($text, words => 50)` | Concise summary at target length |
| `ai_translate($text, to => "Spanish")` | Translation |
| `ai_extract($prompt, schema => +{...})` | Structured JSON output (also auto-routed via `ai($p, schema => ...)`) |

### Built-in tools — **SHIPPED**

Drop-in tool specs ready for the agent loop, no `run` coderef needed
because they route through a native registry:

```stryke
my $r = ai("research the latest stryke release notes",
    tools => [
        web_search_tool(),    # uses BRAVE_SEARCH_API_KEY if set, else DDG
        fetch_url_tool(),     # HTTP GET, returns body text
        read_file_tool(),     # local FS read
        run_code_tool(),      # python3 subprocess, 10s timeout
    ]);
```

The `run_code_tool` shells to `python3` so a Python interpreter must
be on the path; works fine on every modern Linux/macOS dev box.

| Tool | Implementation |
|---|---|
| `web_search_tool` | Brave Search API (auth via `$BRAVE_SEARCH_API_KEY`) → DuckDuckGo HTML scrape fallback |
| `fetch_url_tool` | `ureq` GET with 30s timeout |
| `read_file_tool` | `std::fs::read_to_string` |
| `run_code_tool` | `python3` subprocess, 10s timeout, returns stdout+stderr |

### Conversational sessions — **SHIPPED**

```stryke
my $s = ai_session_new(system => "Be terse", model => "claude-haiku-4-5");
ai_session_send($s, "what's 2+2?");
ai_session_send($s, "and times 3?");
my $hist = ai_session_history($s);   # arrayref of {role, content}
ai_session_reset($s);                 # clear history but keep config
ai_session_close($s);                 # drop session
```

Multi-turn chat that auto-tracks role=user / role=assistant turns.
Provider/model picked at session creation, can be overridden per
`send` call.

### Prompt templates — **SHIPPED**

```stryke
my $p = ai_template("hi {name}, age {age}", name => "Alice", age => 30);
# → "hi Alice, age 30"

ai_template("escaped {{lit}}, real {key}", key => "yes");
# → "{lit}}, real yes"  ({{ → literal {, missing keys pass through)
```

Pure string substitution. No code execution. Use as the prompt arg to
`ai`/`prompt`/`chat`.

### Retry / backoff — **SHIPPED**

Anthropic calls (single-shot AND streaming AND vision AND PDF) auto-
retry on `429` / `500` / `502` / `503` / `504` with exponential
backoff (1s → 2s → 4s → 8s → 16s, capped at 30s). 4 attempts total
before giving up. Transport errors (network blips) also retry.

### Routing actually honored — **SHIPPED**

`ai_routing_set("embed", "openai")` now actually switches embedding
calls to OpenAI's `text-embedding-*` endpoint instead of the default
Voyage. The route table is consulted before falling back to the
`[ai.embed]` TOML config or the `embed_provider` default.

### CLI — **SHIPPED**

```bash
stryke ai "summarize the linux kernel in 50 words"
echo "rough idea: ..." | stryke ai --model claude-haiku-4-5 --system "Be concise"
stryke ai "long thinking task" --stream
stryke ai "structured" --json    # emit {response, usd, input_tokens, output_tokens}
```

`stryke ai PROMPT` reads from argv or stdin, calls the configured
model, prints to stdout. Honors `--model`, `--system`, `--stream`,
`--json`. Useful as a UNIX filter or one-shot from terminal.

### Vision (multimodal images) — **SHIPPED**

```stryke
my $caption = ai("describe this image", image => "/path/to/photo.jpg");
my $alt = ai("describe", image => "https://example.com/img.png");
my $hex = ai("describe", image => $raw_bytes);
```

Routes to `ai_vision`, which builds an Anthropic content array with a
base64-inlined image block (URLs fetched first, paths read, raw bytes
encoded directly). Mime-type guessed from extension. Cost tracking
runs through the same path as text calls.

### MCP server (programmatic) — **SHIPPED**

The declarative `mcp_server "name" { ... }` parser block is still
deferred (needs the same parser work as `tool fn`), but the runtime is
fully wired. Stand up a server with one builtin call:

```stryke
mcp_server_start("stryke-srv", +{
    tools => [
        +{ name => "echo", description => "Echo input",
           parameters => +{ text => "string" },
           run => sub { $_[0]->{text} } },
        +{ name => "uppercase", description => "Uppercase text",
           parameters => +{ text => "string" },
           run => sub { uc($_[0]->{text}) } },
    ]
});
```

Runs a stdio JSON-RPC loop on stdin/stdout, exposes
`initialize` / `tools/list` / `tools/call`. Verified end-to-end with
a stryke client connecting to a stryke server: tools enumerate, calls
round-trip, results return. The same binary that runs your stryke
script can now BE an MCP server — pair with `s_web build` to ship a
self-contained MCP server binary.

### MCP HTTP transport — **SHIPPED**

```stryke
my $gh = mcp_connect("https://api.github.com/mcp");
my $tavily = mcp_connect("https://mcp.tavily.com/mcp");
```

Speaks the streamable-HTTP MCP transport: POST per request, accept
`application/json` OR `text/event-stream` (SSE) responses, carry
`mcp-session-id` across calls when the server sets one. Reads
bearer auth from `$MCP_BEARER_TOKEN` if set.

### OpenAI streaming — **SHIPPED**

`stream_prompt($p, on_chunk => sub { ... }, provider => "openai")`
now parses OpenAI's SSE delta format too. Same callback contract as
the Anthropic path — fires once per text-delta chunk.

### Anthropic batch API — **SHIPPED**

```stryke
my $results = ai_batch(\@prompts,
    model    => "claude-haiku-4-5",
    system   => "Be terse",
    poll_secs    => 5,
    max_wait_secs => 1800);
# 50% of normal cost; trades a few minutes of wall time.
```

Submits the batch, polls `processing_status` until `ended`, downloads
JSONL results, reorders by `custom_id`. Cost tracking applies the
~50% batch discount automatically. Falls back to sequential calls if
the batch endpoint errors (region/account-gated) or
`STRYKE_AI_BATCH=sync` is set.

### Cluster fanout — **SHIPPED**

```stryke
my $cluster = cluster(["host1:8", "host2:8"]);
my @summaries = @{ ai_pmap(\@docs, "summarize",
    cluster => $cluster, model => "claude-haiku-4-5") };
```

Splits items into N shards (N = cluster slot count), runs `ai_map` on
each shard via the existing `pmap_on` plumbing, concatenates results
in order. Without a `cluster => ...` arg, falls back to a single local
`ai_map` call (one batched LLM request).

### Phase 2 — MCP client — **SHIPPED (server side still pending)**

Lives in `strykelang/mcp.rs`. Speaks JSON-RPC line-delimited over
stdio. Builtins:

| Builtin | Behavior |
|---|---|
| `mcp_connect("stdio:CMD ARGS...", $name?)` | Spawn subprocess, run `initialize` + `notifications/initialized` handshake, return handle |
| `mcp_tools($h)` / `mcp_resources($h)` / `mcp_prompts($h)` | Cached `*/list` results |
| `mcp_call($h, $name, +{...args})` | `tools/call` |
| `mcp_resource($h, $uri)` | `resources/read` |
| `mcp_prompt($h, $name, +{...args})` | `prompts/get` |
| `mcp_close($h)` | Kill subprocess, drop registry slot |
| `mcp_attach_to_ai($h)` / `mcp_detach_from_ai($h)` | Mark a handle as auto-attachable so the agent loop can pull its tools |
| `mcp_attached()` | List of currently-attached handles |

Smoke-tested against a 100-line Python fake-server implementing
`initialize`, `tools/list`, `tools/call`, `resources/list`,
`resources/read`, `prompts/list`, `prompts/get`. Handshake +
caching + every method round-trip works.

Transports NOT yet wired:
- `ws://...` (WebSocket — needs a tungstenite dep)
- `http://...` (streaming HTTP — needs SSE)

The **server** side — declarative `mcp_server "name" { tool foo … }`
DSL — needs the same parser extension as `tool fn`. Not in this pass.

### Phase 3 — Collection builtins — **SHIPPED**

Each one builds a single batched prompt asking the model for a JSON
array of judgments, then parses the response. One LLM call per
collection, not N.

| Builtin | Shape | Returns |
|---|---|---|
| `ai_filter(\@items, "criterion")` | Boolean per item | Filtered arrayref |
| `ai_map(\@items, "instruction")` | String per item | Mapped arrayref |
| `ai_classify(\@items, "label hint", into => [\"a\",\"b\"])` | Label per item | Arrayref of labels |
| `ai_match($item, "criterion")` | Single boolean | 0 or 1 |
| `ai_sort(\@items, "criterion")` | Index array (best-first) | Reordered arrayref |
| `ai_dedupe(\@items, "hint")` | Group of indexes per cluster | Deduped arrayref |

JSON-array extraction is forgiving: walks the response looking for the
first balanced `[ ... ]` so the model can wrap output in prose without
breaking the parse.

### Retrieval / vector ops — **SHIPPED**

| Builtin | Returns |
|---|---|
| `vec_cosine(\@a, \@b)` | Cosine similarity in `[-1, 1]` |
| `vec_search(\@query, \@candidates, top_k => N)` | Arrayref of `+{idx, score}`, ranked best-first |
| `vec_topk(\@scores, $k)` | Indexes of top-k scalars |

Verified on the unit basis: `cos([1,0,0],[1,0,0])=1.0000`,
`cos([1,0,0],[0,1,0])=0.0000`, `cos([1,0,0],[-1,0,0])=-1.0000`. `vec_search`
ranks `[1,0,0]` against four candidates as `1 (id), 2 (45°), 0 (orth)`
with scores `1.000, 0.707, 0.000`.

### Cost / routing / history — **SHIPPED**

| Builtin | Behavior |
|---|---|
| `ai_estimate($prompt, model => "...", out_tokens => N)` | Pre-flight USD estimate from token heuristic + price table |
| `ai_routing_get($op)` / `ai_routing_set($op, $provider)` | Per-operation provider override (advisory; embed honors it) |
| `ai_history()` | Arrayref of last 100 calls — `+{provider, model, prompt, response_chars, input_tokens, output_tokens, usd, cache_hit, unix_time}` |
| `ai_history_clear()` | Reset history |

### Phase 1 — Tools and Agents (months 2-4)

- `tool fn` declaration with schema generation.
- `ai` builtin: agent loop using local `tool fn`s.
- `ai_mock` for tests.
- OpenAI provider added.

### Phase 2 — MCP (months 4-6)

- `mcp_connect` client.
- `mcp_server` declarative DSL.
- `s build --mcp-server` flag.
- Auto-attachment of connected MCP servers to `ai`.

### Phase 3 — Collection AI Builtins (months 6-8)

- `ai_filter`, `ai_map`, `ai_classify`, `ai_sort`, `ai_match`, `ai_dedupe`.
- Automatic batching.
- Predicted-cost static analysis.
- Hot-loop lints.

### Phase 4 — Local Models and Multi-Provider (months 8-10)

- llama.cpp linked, embedded model shipped.
- Local fallback when API unreachable.
- Routing config (`[ai.routing]` table).
- Google/Gemini provider added.

### Phase 5 — Composition (months 10-12)

- Web framework integration polished (streaming SSE handlers, structured-output endpoints).
- Cluster dispatch over `ai` calls.
- Cost ceilings and budgets.
- Public benchmark suite (latency per provider, tokens/sec, cost-per-1K-calls).

## Non-Goals

- LangChain compatibility. Stryke is the framework; we don't wrap a Python framework.
- Vendor-specific exhaustive APIs. Each provider exposes its full feature surface through namespaced options, but core primitives (`ai`, `prompt`, `embed`, `chat`) stay uniform.
- Hosted vector DB. Local sqlite-vec ships in-binary; users bring their own remote vector DB (pgvector, Pinecone, etc.) through normal SQL/HTTP.
- Auto-prompt-engineering. `ai` runs the prompt as written. No hidden rewrites, no "we'll improve your prompt for you" surprises. Reproducibility over cleverness.
- Visual agent builders / no-code UIs. Stryke is a programming language; the agent IS the code.

## Open Questions

1. **Sigil syntax for AI calls.** Should `ai` always be called as a function, or should there be a sigil form (e.g. `&"summarize this"`) for the most-common case? Trade-off: terseness vs. parser ambiguity. Default position: function form only, `~>` thread-macro is the terse path.
2. **Streaming default.** Should `ai` return `Stream<Str>` by default and require collection (`ai $p .collect`) for `Str`, or return `Str` by default? Default position: context-sensitive (scalar context = `Str`, iter context = `Stream<Str>`).
3. **Effect type granularity.** Is `Effect::AI` one effect, or split into `Effect::LLM`, `Effect::Embed`, `Effect::ToolCall`? Default position: one effect with a discriminator on the operation, handlers can pattern-match.
4. **Local model packaging.** Embed in every binary unconditionally (~2-4GB), or opt-in via `[ai.local].embed = true`? Default position: opt-in; small dev binaries by default, full local-capable binaries for offline use cases.
5. **Cost model honesty.** Should `ai_cost` be wall-clock USD as billed, or token-counted estimate? Default position: estimate based on token counts, reconciled with provider invoice when the run completes.

## Resolved Decisions

- **`ai` is the builtin name.** Two letters, ubiquitous, used like `print`. Resolved 2026-04-26.
- **Three invocation forms.** Function call, thread macro `~>`, pipe `|>`. All compile to the same primitive. Resolved 2026-04-26.
- **`tool fn` for marking agent-callable functions.** Build-time schema/description generation, no manual JSON schema writing. Resolved 2026-04-26.
- **MCP-native.** `mcp_server` block + `mcp_connect` for clients. Connected MCP servers auto-attach to `ai`. Resolved 2026-04-26.
- **Provider-agnostic, Anthropic-first.** Uniform interface across providers; provider-specific options through namespaced extensions. Local llama.cpp fallback as a first-class option. Resolved 2026-04-26.
- **Cost-aware by construction.** Caching, batching, parallelism, ceilings, introspection — runtime concerns, not user concerns. Resolved 2026-04-26.

## The Pitch on One Line

> *Every other language ships AI as a library. Stryke ships AI as a primitive. Two letters, unlimited power, single-binary deployment. The language designed for the work that matters in this era.*
