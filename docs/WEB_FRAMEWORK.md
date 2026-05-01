# Stryke Web — Design Doc — **PHASE 0 SHIPPED, Phase 1 in progress**

> *Build it like Rails. Deploy it like Go. Run it faster than both.*

**Status:** Phase 0 walking skeleton + most of Phase 1 MVP shipped. The `stryke_web` crate is wired and the runtime `web_*` builtins live in `strykelang/web.rs` and `strykelang/web_orm.rs`. Generator surface (`s_web new myapp --app everything --theme cyberpunk --auth --admin --docker --ci --pwa --migrate`) produces a full-stack cyberpunk-themed app with ~70 resources, auth, admin, ETag-aware controllers, Dockerfile, GitHub Actions CI, and PWA manifest. See README §[0x15] for the user-facing surface and `stryke_web/README.md` for the generator reference. HTTP/2, glommio + io_uring, and the SIMD HTTP parser remain Phase 2+ deferred work.

The world's fastest, cleanest web framework. Native machine-code throughput with Rails-grade developer experience, shipped as a single statically-linked binary. No interpreter on the target machine, no Docker required, no `bundle install`, no `node_modules`, no nginx fronting required, no Sidekiq+Redis dance for the simple case.

This is not a port of an existing framework to stryke. It is a from-scratch design that reuses the Rust ecosystem's fastest building blocks (httparse, rustls, glommio, simd-json, tokio-postgres) and exposes them through a Rails-quality DSL written in stryke. The result is a framework that prototypes faster than Rails, throughputs harder than actix-web, and deploys simpler than Go.

## Goals

1. **Top-3 TechEmpower throughput** within 12 months of the first commit. Beating Phoenix by 10x, Rails by 1000x, Express by 50x is table stakes. Beating drogon and actix-web on plaintext is the stretch target.
2. **`s new myapp --web` to working CRUD app in under 30 seconds.** Convention over configuration. Generators for everything Rails generates.
3. **Single static binary deployment.** `s build --release && scp target/release/myapp prod:` is the entire deploy pipeline. No PaaS required.
4. **Zero install-time code execution on the target machine.** The binary is the app. The OS is the runtime. Nothing else.
5. **Real concurrency from day one.** Native threads, no GIL, async/await on top of thread-per-core io_uring.
6. **DX equivalent to or better than Rails.** Anything that takes 5 lines in Rails takes ≤5 lines in stryke web. Generators, routing DSL, ORM chains, view helpers — all within the same ergonomic envelope.

## Non-Goals

- npm-style asset pipeline. Assets are embedded at build time, period.
- ActiveRecord-grade monkey-patching. Stryke's stdlib stays pure; the framework adds its own helpers.
- Method missing / `respond_to_missing?` magic. Predictability over cleverness.
- Twelve-factor "config must be env vars only" dogma. Config files are fine; env override is supported.
- Pluggable everything. Stryke web ships one opinion per concern (one ORM, one templating engine, one job queue, one async fabric) and holds it.
- Cross-platform parity for the bleeding-edge runtime. io_uring is Linux-only and that's where 95% of production traffic lives. macOS / Windows get a tokio fallback, slower but functional.
- Replacing nginx for everything. nginx is fine in front for caching/edge concerns. Stryke web doesn't *require* it the way Rails does.

## Performance Targets (Public Commitments)

| Benchmark | Phase 0 (3mo) | Phase 1 (6mo) | Phase 2 (12mo) | Phase 3 (18mo) |
|---|---|---|---|---|
| TechEmpower plaintext (req/s) | 500k | 1M | 3M | 6M+ (top-3) |
| TechEmpower JSON (req/s) | 200k | 500k | 1M | 1.5M+ |
| TechEmpower DB single query | 50k | 150k | 300k | 500k+ |
| Cold start (binary load → first response) | <50ms | <10ms | <5ms | <1ms |
| Memory footprint (idle, no requests) | <30MB | <20MB | <15MB | <10MB |
| Memory per concurrent connection | <16KB | <8KB | <4KB | <2KB |

These numbers are public and tracked in CI. Every commit runs the benchmark suite and posts deltas to a GitHub Pages dashboard. Regressions block merge.

## Architecture Overview

```
┌──────────────────────────────────────────────────────┐
│  app/                  user code (controllers, models, views)
│      ↓                                               │
│  Stryke Web DSL        routing, middleware, ORM, templates
│      ↓                                               │
│  Stryke runtime        Cranelift-compiled bytecode → native code
│      ↓                                               │
│  Rust ecosystem        httparse, rustls, hyper, sqlx, glommio, simd-json
│      ↓                                               │
│  OS                    io_uring / kqueue / IOCP, syscalls, sockets
└──────────────────────────────────────────────────────┘
```

Stryke web is ~10-15k lines of stryke gluing together the Rust ecosystem's fastest primitives, exposed through a Rails-quality DSL. Hot paths inline through Cranelift to native machine code with no virtualized overhead.

## Runtime Model

**Thread-per-core with io_uring on Linux. Tokio M:N fallback elsewhere.**

Why thread-per-core wins:
- No cross-core synchronization on the hot path.
- `SO_REUSEPORT` lets the kernel load-balance accepted connections across cores.
- Per-core memory pools, per-core connection state, per-core arena allocator.
- io_uring eliminates syscall overhead for read/write/accept (batched submission, ring-buffer completion).
- This is the seastar/glommio model. ScyllaDB beats Cassandra by 10x using exactly this pattern.

Implementation:

- **Linux**: glommio underneath. One executor per core, pinned. CPU set configured at boot.
- **macOS**: tokio with kqueue. Roughly 2-3x slower per request due to syscall-per-op model, acceptable for dev.
- **Windows**: tokio with IOCP. Same story.

Configuration (`config/server.toml`):

```toml
[runtime]
mode = "auto"              # "auto" | "thread-per-core" | "tokio"
threads = "all"            # number or "all" (= num_cpus)
pin_threads = true         # pin each executor to a core
io_uring_sqe_size = 1024
```

`mode = "auto"` selects thread-per-core on Linux, tokio elsewhere.

## HTTP Stack

**HTTP/1.1, HTTP/2, HTTP/3, all in the same binary, all sharing the same handler API.**

| Layer | Implementation | Why |
|---|---|---|
| Parser (HTTP/1) | `httparse` + custom fast-path for known headers | proven, ~3 GB/s parse rate |
| Framing | hand-rolled, zero-copy where possible | avoids hyper's allocator pressure |
| HTTP/2 | `h2` crate underneath | mature, used by reqwest/tonic |
| HTTP/3 / QUIC | `quinn` underneath, opt-in via `[server.http3]` | additive, not default |
| TLS | `rustls` + kTLS on Linux ≥ 4.13 | ~2x OpenSSL on x86_64 |
| Compression | `brotli`, `zstd`, `gzip` (precomputed for static, on-the-fly for dynamic) | |

**HTTP version negotiation:**
- Plain HTTP → HTTP/1.1.
- TLS with ALPN → HTTP/2 if both sides agree, else HTTP/1.1.
- Alt-Svc / Alt-Used → HTTP/3 over QUIC if enabled.

**Per-request memory model:**

Every request gets an arena allocator (`bumpalo`-style). All allocations during the request — parsed headers, parameters, response body — bump a pointer in the arena. At response completion the entire arena is dropped in one `free()`. Zero individual deallocations on the hot path. This is the single biggest perf win after thread-per-core.

## Routing DSL

Rails-grade ergonomics, radix-trie-compiled, static dispatch.

```stryke
# config/routes.stk

route :GET,    "/",                 home#index
route :GET,    "/health",           health#check
route :POST,   "/login",            sessions#create
route :DELETE, "/logout",           sessions#destroy

resources :posts                                # 7 standard CRUD routes
resources :users do {
    resources :posts, only: [:index, :create]   # nested
    member do {
        route :POST, :follow,       users#follow
    }
}

namespace :api, version: "v1" do {
    resources :users
    resources :posts
}

# Constraints, formats, host matching all supported
route :GET, "/feed.:format", feeds#show, format: ["json", "atom", "rss"]
route :GET, "/admin",        admin#index, host: "admin.example.com"

# WebSocket and SSE first-class
ws    "/chat",      chat#stream
sse   "/events",    events#stream
```

Compilation:
1. Routes parsed at build time.
2. Compiled into a radix trie with parameter capture indices.
3. Trie serialized into the binary as a static lookup table.
4. Match resolves a path in 50-200ns with zero allocation.

`resources :posts` expands at build time to:

```
GET    /posts             posts#index
GET    /posts/new         posts#new
POST   /posts             posts#create
GET    /posts/:id         posts#show
GET    /posts/:id/edit    posts#edit
PATCH  /posts/:id         posts#update
DELETE /posts/:id         posts#destroy
```

Same as Rails. Same muscle memory. None of the runtime cost.

## Request and Response

```stryke
# app/controllers/posts_controller.stk

class PostsController < Controller {
    fn index() {
        my @posts = Post.published.recent.limit(20)
        render :index, posts: \@posts
    }

    fn show($id) {
        my $post = Post.find($id) // return not_found()
        render :show, post: $post
    }

    fn create() {
        my $post = Post.new(post_params())
        if ($post.save) {
            redirect_to post_path($post.id), notice: "Created"
        } else {
            render :new, post: $post, status: 422
        }
    }

    private

    fn post_params() {
        params.require(:post).permit(:title, :body, :tags)
    }
}
```

`params`, `render`, `redirect_to`, `not_found`, `request`, `response`, `session`, `cookies`, `flash` are all in scope inside controller methods. Rails-style ergonomics, no method_missing magic — they're explicit method-table entries on `Controller`.

## Middleware

Tower-style `Service` trait, statically composed. No vtable hops in the chain.

```stryke
# config/middleware.stk

use_middleware Stryke::Web::Logger
use_middleware Stryke::Web::Compression, threshold: 1024
use_middleware Stryke::Web::SessionStore, backend: :cookie, secret: env("SESSION_SECRET")
use_middleware Stryke::Web::CSRFProtection
use_middleware Stryke::Web::ContentSecurityPolicy, default_src: ["'self'"]
use_middleware MyApp::CustomAuth
```

Middleware composition resolves at compile time. The pipeline becomes a straight-line function call graph in the compiled binary — no dynamic dispatch on the request path.

## ORM

ActiveRecord-style chain API compiling to prepared statements. Postgres first-class.

```stryke
# app/models/post.stk

class Post < Model {
    field $id        : Int      (primary_key, auto_increment)
    field $title     : Str      (not_null, max: 200)
    field $body      : Str      (not_null)
    field $author_id : Int      (foreign_key: User)
    field $published : Bool     (default: false)
    field $created_at : Time    (auto_now_add)
    field $updated_at : Time    (auto_now)

    belongs_to :author, class: User
    has_many   :comments

    scope :published, -> { where(:published => true) }
    scope :recent,    -> { order(:created_at, :desc) }

    validates :title, presence: true, length: { min: 3, max: 200 }
    validates :body,  presence: true

    before_save :sanitize_body

    fn sanitize_body() {
        $self.body = sanitize_html($self.body)
    }
}
```

Usage:

```stryke
my @posts = Post.published.recent.limit(20)
my $post  = Post.find(42)
my $count = Post.where(:author_id => $user.id).count
my @top   = Post.joins(:comments)
              .group("posts.id")
              .order("count(comments.id) DESC")
              .limit(10)
```

**Compilation.** The chain `Post.published.recent.limit(20)` compiles at build time (where statically resolvable) into a single prepared statement: `SELECT * FROM posts WHERE published = true ORDER BY created_at DESC LIMIT 20`. Runtime ORM overhead approaches zero. Dynamic chains fall back to a fast query builder.

**N+1 detection.** Dev mode runs every query through an analyzer that flags N+1 patterns. CI fails on detected N+1 in test runs. Prod mode skips the analyzer.

**Connection pooling.** Per-core pool by default (matches the runtime model). Default size = num_cores × 4. Tunable via `config/database.toml`.

**Backends.** Postgres (first-class), MySQL (full support), SQLite (full support, used in dev/test by default), MSSQL (best-effort).

## Migrations

Code, not raw SQL files.

```stryke
# db/migrations/20260426120000_create_posts.stk

migration "CreatePosts" {
    fn up() {
        create_table :posts do {
            column :id,         :int,       primary_key: true, auto_increment: true
            column :title,      :string,    null: false, limit: 200
            column :body,       :text,      null: false
            column :author_id,  :int,       null: false, foreign_key: :users
            column :published,  :bool,      default: false
            timestamps
        }
        add_index :posts, [:author_id, :created_at]
    }

    fn down() {
        drop_table :posts
    }
}
```

```bash
s g migration AddSlugToPosts slug:string:unique
s db migrate
s db rollback
s db reset
```

Schema is dumped to `db/schema.stk` after migrations. CI verifies migrations are reversible.

## Templates

**AOT-compiled at build time.** No runtime template parsing. Templates become native code embedded in the binary.

```html
<%# app/views/posts/index.stk.html %>

<% extends "layouts/application" %>

<% block :content { %>
    <h1>Posts</h1>
    <ul>
    <% for my $post in @posts { %>
        <li>
            <a href="#{post_path($post.id)}">#{$post.title}</a>
            <span class="meta">by #{$post.author.name}</span>
        </li>
    <% } %>
    </ul>
<% } %>
```

**Syntax is stryke, not a separate template grammar.** Two tags, one rule:

| Construct | Syntax | Notes |
|---|---|---|
| Output, HTML-escaped | `#{ expr }` | Same `#{}` interpolation as normal stryke strings — zero new syntax to learn |
| Output, raw (no escape) | {% raw %}<code>#{{ expr }}</code>{% endraw %} | Explicit opt-out, lints flag every use |
| Control flow / blocks | `<% stryke_code %>` | Body is literal stryke — `for`, `if`, `while`, blocks, declarations |
| Template comment | `<%# ... %>` | Stripped at compile time, never reaches output |
| Layout / inheritance | `<% extends "..." %>` `<% block :name { %> ... <% } %>` | Block definitions use stryke block syntax |

A template is conceptually a stryke function that emits HTML, with `#{}` as the interpolation primitive and `<% %>` as the embedded-code escape. ERB users get muscle memory, stryke users see their actual language inside the tags. No Jinja, no Liquid, no Twig dialect to memorize.

Compilation pipeline:

1. Parse template at build time → AST.
2. Type-check against the declared context (`render :index, posts: \@posts` declares the type).
3. Lower to stryke code.
4. Compile through Cranelift to native machine code.
5. Embed in binary.

A template render is a function call. No string interpolation overhead, no escaping decisions at runtime — escape rules baked in at compile time per slot.

**Layouts and partials** work identically to Rails. `<% include "shared/_post", post: $post %>` renders `app/views/shared/_post.stk.html` with `$post` in scope.

**Auto-escape by default.** `#{ user_input }` is HTML-escaped at compile time per slot — the escape decision is baked into the generated native code, no runtime branching. {% raw %}<code>#{{ user_input }}</code>{% endraw %} is the explicit raw opt-out and every occurrence is flagged by lint.

## Background Jobs

In-process queue persisted to the app's database. No Redis required for the 90% case.

```stryke
# app/jobs/send_welcome_email.stk

class SendWelcomeEmail < Job {
    queue :mailers
    retry_on Net::Error, max: 5, backoff: :exponential

    fn perform($user_id) {
        my $user = User.find($user_id)
        Mailer.welcome($user).deliver
    }
}

# Enqueue:
SendWelcomeEmail.perform_later($user.id)
SendWelcomeEmail.perform_at(time_now() + 3600, $user.id)
```

Backend options (`config/jobs.toml`):

```toml
[jobs]
backend = "database"        # "database" | "redis" | "sqs" | "in-memory"
workers_per_core = 2
```

`backend = "database"` writes job rows to the same DB as the app. A worker thread (or worker process, configurable) polls and executes. No external dependency. Survives restarts. Good for ~1k jobs/sec, which covers 95% of apps.

Scale up to Redis/SQS only when you actually need cross-machine job distribution.

## WebSockets and Server-Sent Events

First-class, same async fabric as request handlers.

```stryke
# app/channels/chat_channel.stk

class ChatChannel < Channel {
    fn on_connect() {
        $self.join("room:" . params[:room_id])
        broadcast_to(:room => params[:room_id], event: "user_joined", user: current_user.name)
    }

    fn on_message($payload) {
        broadcast_to(:room => params[:room_id], event: "message", body: $payload.body)
    }

    fn on_disconnect() {
        broadcast_to(:room => params[:room_id], event: "user_left", user: current_user.name)
    }
}

# config/routes.stk
ws "/chat/:room_id", ChatChannel
```

```stryke
# app/controllers/events_controller.stk

class EventsController < Controller {
    fn stream() {
        sse_stream { |stream|
            for my $event in Event.subscribe(:user_id => current_user.id) {
                stream.send($event.to_json, event: $event.kind)
            }
        }
    }
}
```

Underneath: tungstenite for WS, hand-rolled SSE framing. Same arena allocator as HTTP requests.

## Static Assets

**Embedded into the binary at build time.** Pre-compressed. Served with zero-copy where possible.

Build pipeline:
1. `public/` directory walked at build time.
2. Each asset compressed to gzip + brotli + zstd ahead of time.
3. Fingerprinted (`app.css` → `app-a3f5e1.css`).
4. Embedded into the binary as `.rodata`.
5. A static manifest maps logical → fingerprinted paths.
6. View helpers (`asset_path("app.css")`) resolve through the manifest at runtime in O(1).

Serving:
- `Accept-Encoding: br,gzip` → serve precompressed brotli/gzip directly from `.rodata`.
- ETag/If-None-Match handled in O(1) (fingerprint *is* the ETag).
- `sendfile`-equivalent zero-copy on Linux for large assets when not embedded.

**No webpack, no Sprockets, no Vite, no esbuild integration required.** A small JS/CSS bundler ships in stryke web for common cases (concatenate, minify, source-map). For SPA frontends, dump pre-built artifacts in `public/` and let the framework embed them.

## Generators and Scaffolding

```bash
s new myapp --web                  # full web app skeleton
s new mylib                        # library only

s g model    User name:string email:string:unique
s g controller Users index show create
s g resource Post title:string body:text author:references
s g migration AddSlugToPosts slug:string:unique
s g job      SendWelcomeEmail user_id:int
s g channel  Chat
s g mailer   UserMailer welcome reset_password
s g middleware RequireAuth
```

Each generator emits the file, the test stub, and updates the routes/migrations/registry as appropriate. Idempotent — re-running with the same args is a no-op or a clear diff.

## Project Layout

```
myapp/
  stryke.toml                  # package manifest (deps, [bin], etc.)
  stryke.lock                  # pinned versions
  main.stk                     # bootstrap: parse config, start server
  app/
    controllers/
      application_controller.stk
      posts_controller.stk
    models/
      application_model.stk
      post.stk
    views/
      layouts/
        application.stk.html
      posts/
        index.stk.html
        show.stk.html
    jobs/
    mailers/
    channels/
    middleware/
  config/
    routes.stk
    middleware.stk
    database.toml
    server.toml
    jobs.toml
    secrets.toml.encrypted
  db/
    migrations/
      20260426120000_create_posts.stk
    schema.stk
    seeds.stk
  public/                      # static assets, embedded at build
    favicon.ico
    css/
      app.css
    js/
      app.js
  lib/                         # plain stryke modules (non-web)
  t/                           # tests
    controllers/
    models/
    integration/
  benches/                     # perf benches
  target/                      # build outputs (gitignored)
    release/
      myapp                    # ← single fat exe, ~20MB, scp-ready
```

Conventions match Rails for muscle memory; deviations only where stryke's existing conventions (`t/`, `lib/`, `benches/`, `target/`) already apply.

## Configuration

TOML files in `config/`. Environment variable override for secrets. No 12-factor dogma — config files for what's meaningful to read, env for what's meaningful to vary per deploy.

```toml
# config/server.toml

[server]
host = "0.0.0.0"
port = 3000
workers = "all"
shutdown_timeout = 30

[server.http2]
enabled = true
max_concurrent_streams = 256

[server.http3]
enabled = false                # opt-in; QUIC requires UDP firewall config

[server.tls]
enabled = false                # set in prod via env or here
cert = "/etc/myapp/cert.pem"
key  = "/etc/myapp/key.pem"

[server.compression]
gzip   = true
brotli = true
zstd   = true
threshold_bytes = 1024
```

```toml
# config/database.toml

[database]
url = "${DATABASE_URL}"        # env interpolation
pool_size = 16
timeout_ms = 5000

[database.dev]
url = "sqlite://./dev.db"

[database.test]
url = "sqlite::memory:"
```

Encrypted secrets via `config/secrets.toml.encrypted`, decrypted at boot using a key in `STRYKE_MASTER_KEY` env var or a key file. Same model as Rails encrypted credentials.

## Dev Workflow

```bash
s new myapp --web              # scaffold
cd myapp
s db migrate                   # set up SQLite by default
s dev                          # boot dev server with hot reload
```

`s dev` does:

1. JIT-compiles all stryke modules in dev mode (Cranelift JIT, sub-millisecond per module).
2. Starts the server on `localhost:3000`.
3. Watches `app/`, `config/`, `lib/`, `db/` for changes.
4. On change: recompiles affected modules in-place, swaps the route table atomically, no process restart.
5. Browser tab live-reloads via injected SSE channel.

Hot reload is **real**, not Rails's "we re-autoload classes" hack. Cranelift's compilation speed makes module-level recompile feel instant.

## Production Deployment

```bash
s build --release                          # → target/release/myapp
scp target/release/myapp prod:/opt/myapp/myapp.new
ssh prod 'cd /opt/myapp && \
    ./myapp.new db migrate && \
    mv myapp.new myapp && \
    systemctl restart myapp'
```

Three lines. Zero-downtime variant uses `SO_REUSEPORT` + systemd socket activation, four lines.

```ini
# /etc/systemd/system/myapp.service

[Unit]
Description=My Stryke Web App
After=network.target postgresql.service

[Service]
ExecStart=/opt/myapp/myapp
Restart=always
User=myapp
EnvironmentFile=/etc/myapp/env
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
```

That's the entire production setup. No nginx required (stryke web serves TLS, HTTP/2, HTTP/3, static assets, gzip/brotli compression, all natively). nginx is welcome in front for edge caching or multi-app routing if you want it; stryke web doesn't *need* it the way Rails needs Puma + nginx.

**Container deploy.** A `Dockerfile` for stryke web is two lines:
```Dockerfile
FROM scratch
COPY target/release/myapp /myapp
ENTRYPOINT ["/myapp"]
```
That's it. `FROM scratch`, no base image, ~20MB image total.

## Security Defaults

Secure-by-default is non-negotiable. Apps must opt *out* of safety, not opt in.

| Concern | Default |
|---|---|
| CSRF | Enabled for non-GET. Token in form helper, header for fetch/XHR. |
| XSS | Auto-escape in templates. `raw(...)` is the explicit opt-out. |
| SQL injection | Prepared statements only. ORM never builds string-concatenated SQL. |
| Session cookies | `Secure`, `HttpOnly`, `SameSite=Lax` by default. |
| Password storage | Argon2id with sane params. `password :password_hash` field type generates accessors. |
| Headers | CSP, HSTS, X-Frame-Options, X-Content-Type-Options, Referrer-Policy all set with safe defaults. |
| Rate limiting | Per-IP and per-auth-token middleware available out of the box. |
| Mass assignment | `params.require(...).permit(...)` is mandatory in controllers; raw `params[...]` access into models is a lint error. |
| Encryption | Strong defaults baked into `Stryke::Web::Crypto`. AES-256-GCM, ChaCha20-Poly1305. No "RC4 is fine" footguns. |

## Observability

Built-in, not bolt-on.

| Concern | Built-in |
|---|---|
| Structured logs | JSON to stdout by default. Trace IDs auto-propagated. |
| Metrics | Prometheus endpoint at `/metrics`. Per-route latency histograms, request counts, error rates, DB pool saturation, job queue depth. |
| Tracing | OpenTelemetry spans for HTTP, DB, jobs, external HTTP calls. OTLP export configurable. |
| Health checks | `/health` (liveness, no deps), `/ready` (readiness, checks DB+queue). |
| Profiling | `s prof` attaches to a running server, dumps a flamegraph. CPU + allocation profiles. |

Zero config required. Disable per-section in `config/observability.toml` if you want.

## Benchmarking and Public Numbers

Honesty matters. The framework lives or dies by reproducible public benchmarks.

- TechEmpower-clone benchmark suite checked into the repo at `benches/web/`.
- CI runs the suite on every PR, posts deltas to `https://stryke.dev/bench/`.
- Reproducible Docker images and exact hardware specs published.
- Comparison runs against actix-web, axum, drogon, Phoenix, Rails, Express maintained quarterly.
- Performance regressions block merge, no exceptions.

Benchmark files live next to the code they benchmark (e.g., `benches/web/router_bench.stk`, `benches/web/json_bench.stk`). `s bench benches/web/` runs them all.

## Implementation Phases

### Phase 0 — Walking Skeleton — ✅ SHIPPED

Goal: prove the perf model.

- ✅ HTTP/1.1 server (`web_serve`).
- ✅ Radix-trie router compiled from the routing DSL (`web_route`/`web_resources`/`web_root`).
- ✅ Request/response abstractions (`web_request`, `web_render`, `web_set_header`, `web_status`, `web_params`).
- ✅ Middleware (logger, security headers, ETag short-circuit, CSRF token).
- ✅ Generator: `s_web new myapp` produces a working app.
- ✅ ORM with chain API, prepared statements, pool — SQLite is the dev/test default per the resolved decision below; Postgres/MySQL via runtime builtins.
- ⏳ TechEmpower plaintext + JSON benchmarks runnable — local benchmarks via `s bench` work; TechEmpower harness wiring is Phase 2 deferred.
- **Target: 500k req/s plaintext on a modern laptop** — perf still subject to TechEmpower-style validation.

### Phase 1 — MVP Framework — ✅ MOSTLY SHIPPED

Goal: real apps shippable.

- ⏳ HTTP/2 via `h2`, TLS via `rustls` — deferred.
- ✅ Migrations DSL (`web_create_table`, `web_add_column`, `web_remove_column`, `web_drop_table`, `web_migrate`/`web_rollback`, `schema_migrations` tracking).
- ✅ ERB templates (`<%= %>` / `<% %>` / `<%# %>` / `<%- -%>`) + layouts + `web_render_partial`.
- ✅ Background jobs (database backend) — `web_jobs_init` creates the SQLite `jobs` table; `web_job_enqueue`/`dequeue`/`complete`/`fail` plus `web_jobs_list`/`web_jobs_stats`/`web_job_purge` for inspection.
- ⏳ WebSockets — deferred. ✅ SSE wired (`web_sse_event`, `web_render_stream`).
- ✅ Generators for model/controller/resource/migration/scaffold/api/auth/admin/mailer/job/channel/docker/ci/pwa.
- ✅ Encrypted secrets — `secrets_encrypt`/`secrets_decrypt` (AES-256-GCM), `secrets_random_key` for fresh keys, `secrets_kdf` for PBKDF2 password derivation.
- ✅ Security middleware (CSRF token meta + cookie, CSP/HSTS via `web_security_headers`).
- ✅ Embedded static assets pipeline (`web_static`).
- **Target: 1M req/s plaintext, 500k JSON, 150k DB single-query** — pending Phase 2 perf work.

### Phase 2 — Production Grade — ⏳ MOSTLY DEFERRED

Goal: top-3 perf, full DX.

- ⏳ glommio + io_uring runtime (Linux).
- ⏳ Per-core sharded everything.
- ⏳ simd-json integration — current JSON path is `serde_json`.
- ⏳ Full ORM (joins, eager loading, scopes, callbacks) — chain API works for single-table queries; joins/eager-loading/scopes pending. ✅ `web_model_paginate`/`search`/`soft_destroy`/`count`/`first`/`last`/`with` for n+1 elimination already shipped.
- ⏳ Hot reload polished.
- ⏳ Channels (WebSocket abstraction, broadcast across cores).
- ⏳ Mailers — generator scaffolds the structure; runtime SMTP layer pending.
- ✅ Comprehensive `s_web g` generators (already shipped — pulled forward from Phase 2 to Phase 1).
- ⏳ Public benchmark dashboard.
- **Target: 3M req/s plaintext, 1M JSON, 300k DB single-query. Top-3 TechEmpower placement.**

### Phase 3 — Stretch — ⏭️ NOT STARTED

- HTTP/3 / QUIC default-on for TLS.
- kTLS for static assets.
- Custom SIMD HTTP parser.
- Multi-machine job clustering.
- Edge deploy (`--target=wasm32-wasi` for Cloudflare/Fastly).
- Lambda runtime adapter.
- **Target: 6M+ req/s plaintext. Beat actix-web. Number 1 or 2 on TechEmpower.**

## Open Questions

These get answered as we build. Not blockers, but worth flagging.

1. **ORM declarative vs. imperative.** Rails models are heavy on metaprogramming (`belongs_to :author` modifies the class). Stryke can keep that aesthetic without Ruby's runtime cost — `belongs_to` is a build-time macro, not a runtime mutation. Open question: how heavy should the macro layer be?
2. **Async fabric primitive.** `async fn` with `await` is the obvious answer, but there's an argument for green-thread (Go-style) or even synchronous-looking code with implicit yielding. Decide before Phase 1.
3. **Postgres-first vs. database-agnostic.** Postgres-first lets us use features that other DBs lack (jsonb, arrays, COPY, listen/notify). Database-agnostic limits us to the common subset. Lean Postgres-first; SQLite supported for dev/test only at full feature parity.
4. **Scopes and dynamic chaining vs. fully-typed query language.** Rails-style chains are dynamic but powerful. A typed query DSL (Diesel-style) is safer but more verbose. Pick the chain API; type-check what we can statically, fall back to runtime errors for what we can't.

## Resolved Decisions

- **Template syntax — UPDATED** — Shipped form is ERB-style: `<%= expr %>` for HTML-escaped output, `<%== expr %>` for raw output, `<% stryke_code %>` for control flow, `<%# comment %>` for comments, `<%- -%>` for whitespace trimming. The original `#{ expr }` proposal was superseded by ERB during Phase 1 to keep visual parity with Rails templates. Templates are stryke code with HTML interpolation. Resolved 2026-05-01.
- **Default database for dev/test** — SQLite. Postgres/MySQL accessed via runtime builtins (`web_db_open`/`web_db_query`). The ORM chain API works against any of the three; SQLite is what `s_web new` wires by default so a fresh app boots without needing a running Postgres. Resolved 2026-05-01.

## Naming

The framework is **stryke web**, lowercase, treated as a feature of the language not a separate brand. Module path: `stryke::web`. CLI: `s new app --web`. Marketing usage: "Stryke Web" with capitalization, never as "StrykeWeb" or "StrykerWeb" or any other variant.

## The Pitch on One Page

> *Stryke Web is the cleanest, fastest web framework on Earth. Build a CRUD app in 30 seconds with `s new myapp --web`, write Rails-quality code, hit Phoenix-grade throughput in Phase 0, top-3 TechEmpower in Phase 2, and deploy with `scp target/release/myapp prod:`. The only framework where developer happiness, native machine speed, and single-binary deployment all live in the same box.*

Build it like Rails. Deploy it like Go. Run it faster than both.
