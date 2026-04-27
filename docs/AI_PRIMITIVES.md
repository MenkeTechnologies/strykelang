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

### Phase 0 — Walking Skeleton (months 0-2)

- `prompt` and `stream_prompt` builtins, single-shot Anthropic calls.
- `embed` builtin with sqlite-vec backend for local vector search.
- TOML config for provider/model/key.
- Result cache (in-memory, file-backed).
- Cost tracking.

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
