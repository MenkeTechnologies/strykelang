# stryke_web

Rails-shaped web framework for the stryke language. The `s_web` CLI is a Rust
binary that generates `.stk` source files for a new app — same role as
`rails new` for Ruby, except the output is stryke instead of Ruby.

## Architecture

| Layer | Language | Where it lives |
|---|---|---|
| **Generator CLI** (`s_web new`, `g`, `s`, `db`, `routes`) | Rust | `stryke_web/` (this directory) |
| **Generated app code** (`config/routes.stk`, `app/controllers/*.stk`, `app/models/*.stk`, …) | Stryke | written into the user's app dir |
| **Framework runtime** (`route`, `render`, `Controller`, `Model`, `serve`, …) | Rust builtins | `strykelang/` (already lives there for `serve`; routing/render/ORM TBD) |

The `s_web` binary is host-language Rust because stryke itself is implemented
in Rust — same way the `rails` binary is Ruby because Rails is Ruby. Output
is stryke source because that's what runs at request time.

## Build

```sh
cd stryke_web
cargo build --release
# binary at: target/release/s_web
```

The crate explicitly opts out of the parent `strykelang` workspace via
`[workspace]` in its own `Cargo.toml`, so building it stays fast and
independent of the (much larger) interpreter build.

## Subcommands

| Command | Equivalent Rails | Status |
|---|---|---|
| `s_web new APP` | `rails new APP` | ✅ writes full directory tree |
| `s_web g controller NAME ACT…` | `rails g controller` | ✅ controller + per-action views, ERB-rendered at request time |
| `s_web g model NAME field:type…` | `rails g model` | ✅ model + create migration (ORM lands PASS 3) |
| `s_web g migration NAME field:type…` | `rails g migration` | ✅ schema-change migration (runner lands PASS 4) |
| `s_web g scaffold NAME field:type…` | `rails g scaffold` | ✅ model + migration + 7-action controller + 5 ERB views |
| `s_web s [-p PORT]` | `rails server` | ✅ shells out to `stryke bin/server` |
| `s_web routes` | `rails routes` | ⚠️ pending `Router::dump_table` builtin |
| `s_web db migrate` | `rails db:migrate` | ✅ runs all pending `up` blocks against `db/$ENV.sqlite3` |
| `s_web db rollback` | `rails db:rollback` | ✅ unwinds the latest applied migration via its `down` block |
| `s_web db seed` | `rails db:seed` | ✅ requires `db/seeds.stk` against the configured DB |
| `s_web db reset` | `rails db:reset` | ✅ deletes the sqlite file, re-migrates, re-seeds |
| `s_web console` | `rails console` | ⚠️ pending stryke `--repl` flag wired |

PASSes 1–5 of the framework runtime have shipped:

- **PASS 1** — routing + dispatch (`web_route`, `web_resources`, `web_root`, `web_boot_application`)
- **PASS 2** — ERB engine, layout wrapping, default-convention render
- **PASS 3** — SQLite ORM (`web_model_*` builtins + per-model static
  methods on the generated `class Article extends ApplicationRecord`)
- **PASS 4** — Migrator + schema DSL + `schema_migrations` tracking
- **PASS 5** — view helpers + auto-generated static methods on each
  model class so controllers say `Article::all` not `web_model_all("articles")`
- **PASS 6** — mega scaffold (`s_web g app PRESET` and `s_web new APP --app PRESET --migrate`)
  for one-line entire-app generation
- **PASS 7** — JHipster-style one-liner: `--theme {simple|dark|pico|bootstrap|tailwind}`,
  `--auth` (user model + sessions + signup/login/logout + password hashing),
  `--admin` (admin panel at `/admin`), `--api` (JSON-only mode), built-in
  `/health` endpoint, static file server for `public/`, cookie-based session
  + flash, strong params (`web_permit`), `s_web routes` prints the route
  table, generators for mailer / job / channel
- **PASS 8** — Rails/Django/Express parity: `web_validate` (presence /
  length / format / numericality / inclusion / confirmation), pagination
  (`web_model_paginate`), search (`web_model_search`), soft delete
  (`web_model_soft_destroy`), DB transactions (`web_db_begin/commit/rollback`),
  `web_before_action` / `web_after_action` filter chain, in-memory cache
  (`web_cache_get/set/delete/clear`), i18n (`web_t`, `web_load_locale`),
  signed payloads (`web_signed`/`web_unsigned`), CORS (set `cors_origin`
  in `web_application_config`), per-request log line written to
  `log/$ENV.log`, `web_set_header`, `web_status`, `web_uuid`, `web_now`,
  `web_model_count/first/last`
- **PASS 10 — monster mode** — JWT (`web_jwt_encode/decode`), rate
  limiting (`web_rate_limit`), TOTP/2FA (`web_otp_secret/generate/verify`),
  Markdown rendering (`web_markdown`), HTTP cache (`web_etag` with 304
  short-circuit), CSV export (`web_csv`), faker
  (`web_faker_name/email/sentence/paragraph/int`), counter caches
  (`web_model_increment`), eager loading (`web_model_with`), content
  blocks (`web_content_for/yield_content`), partials
  (`web_render_partial`), security headers (`web_security_headers`),
  OpenAPI 3.0 dump (`web_openapi`), signed time-limited tokens for
  password resets (`web_token_for/consume`), permission checks
  (`web_can`), plus `s_web g {docker,ci,pwa}` and matching `--docker
  --ci --pwa` flags on `s_web new`
- **PASS 11 — APIs + fat binary** — built-in `/openapi.json`,
  Swagger UI at `/docs`, Redoc at `/docs/redoc` (no app code needed
  for any of them — automatic from the live route table). JSON:API
  helpers (`web_jsonapi_resource/collection/error`). Bearer token
  extractor (`web_bearer_token` for JWT auth APIs). And **`s_web build`**
  — generates a Rust wrapper crate that `include_str!`s every `.stk`
  file, so `cargo build --release` produces a single self-contained
  binary that ships the entire framework + app + SQLite (statically
  linked) with no runtime deps beyond libc + libssl. Verified at
  136 MB on macOS arm64 — runs `./api_monster` and the embedded app
  boots, migrates, and serves all routes immediately.

Remaining work:

- **Mailer / Jobs / Channels generators** — `s_web g mailer`, `s_web g job`,
  `s_web g channel`. Stryke already has the parallel + channel primitives;
  the generators just write the boilerplate.
- **Strong params + form errors** — controllers currently pass the raw
  `web_params()` to the model. Need a `permit` helper to whitelist
  fields, plus a `$model->{errors}` slot the form templates can render.
- **`s_web routes`** — needs `web_routes_table` exposed via the CLI;
  currently the builtin exists but the subcommand still shells out
  empty-handed.
- **Encrypted credentials** — `config/credentials.stk.enc` + master.key.
- **`s_web new --api`** — skip views/helpers/layout for API-only apps.
- **Asset pipeline** — embed CSS/JS at build time (per WEB_FRAMEWORK.md).

## Quickstart

### Single resource

```sh
s_web new myblog
cd myblog
s_web g scaffold Post title:string body:text published:bool
# routes.stk — add `web_resources "posts"`
s_web db migrate
bin/server
# open http://localhost:3000/posts
```

### One-liner: full-stack app

```sh
s_web new myapp --app everything --theme dark --auth --admin --migrate
cd myapp && bin/server
# Open http://localhost:3000
```

That single command produces:

- **69 resources** (every preset combined — Posts, Products, Orders,
  Webhooks, Tickets, Opportunities, etc.)
- **70 controllers**, **70 models**, **349 ERB views**, **69 migrations**
- **71 SQLite tables**, schema migrated, `schema_migrations` populated
- **~490 working CRUD routes** (resources × 7 REST verbs + auth + admin)
- **Dark CSS theme** at `public/assets/application.css`
- **Auth flow** — signup/login/logout pages, session cookie, password
  hashing (`web_password_hash` / `web_password_verify`)
- **Admin panel** at `/admin` browsing every model
- **`/health` JSON endpoint**, **static file server** for `public/`

Wall time: ~0.45s scaffold + migrations.

### Flags

| Flag | What it does |
|---|---|
| `--app PRESET` | Bulk-scaffold from a preset (`blog`, `ecommerce`, `saas`, `social`, `cms`, `forum`, `crm`, `helpdesk`, `everything`) |
| `--app "Name:f:t Other:f:t"` | Inline resource list (whitespace-separated `Name:field:type,…`) |
| `--theme NAME` | CSS theme: `simple`, `dark`, `pico`, `bootstrap`, `tailwind`, `cyberpunk`, `synthwave`, `terminal`, `matrix` |
| `--auth` | User model + sessions + signup/login/logout pages |
| `--admin` | Admin panel at `/admin` with CRUD browsing |
| `--api` | API-only mode — drop views/layout, default controllers emit JSON |
| `--migrate` | Run `db migrate` so the SQLite schema is ready immediately |
| `--docker` | Add `Dockerfile` + `.dockerignore` (multi-stage build, runs migrations + boots server on container start) |
| `--ci` | Add `.github/workflows/ci.yml` running migrate + boot + `/health` smoke on push |
| `--pwa` | Add `public/manifest.json` + `public/sw.js` for installable PWA + offline cache |
| `--skip-git` | Don't `git init` |

### Smaller examples

```sh
# Just a blog with auth
s_web new myblog --app blog --auth --migrate

# An e-commerce site with admin and dark theme
s_web new shop --app ecommerce --theme dark --admin --migrate

# JSON-only API for inline-defined resources
s_web new api --api --app "User:name:string,email:string Post:title:string,body:text" --migrate

# Cyberpunk SaaS dashboard — neon cyan/magenta on cyber-grid
s_web new neon --app saas --theme cyberpunk --auth --admin --migrate

# Synthwave retrowave — purple/pink/orange sunset
s_web new wave --app cms --theme synthwave --auth --migrate

# Terminal phosphor — green on black VT220 look
s_web new tty --app forum --theme terminal --auth --migrate

# Matrix — green digital rain background
s_web new neo --app social --theme matrix --auth --migrate

# Amazon clone with cyberpunk neon (25 resources)
s_web new shop --app amazon --theme cyberpunk --auth --admin --migrate

# Facebook clone with synthwave (23 resources)
s_web new fb --app facebook --theme synthwave --auth --admin --migrate

# Anki-style learning tracker with matrix theme (21 resources)
s_web new study --app learning --theme matrix --auth --admin --migrate
```

### Named clone presets

For when you want a specific app shape and don't want to think about
field lists. Each is hand-curated to match the real product:

| Preset | Resources | Key tables |
|---|---|---|
| `amazon` | 25 | Products, Variants, Brands, Departments, Carts, Orders, Payments, Shipments, Reviews, Q&A, Wishlists, Recommendations, Sellers, Listings, Returns |
| `facebook` | 23 | Friendships, Posts, Reactions, Comments, Albums, Photos, Groups, Events, RSVPs, DirectMessages, Conversations, Stories, Pages, Hashtags, Mentions |
| `learning` | 21 | Courses, Lessons, Notes, Decks, Flashcards (with SRS Reviews), StudySessions, Goals, Quizzes, QuizQuestions/Choices/Attempts, Streaks, Achievements, Highlights |
| `blog` | 8 | Posts, Comments, Tags, Categories, Subscribers, Pageviews |
| `ecommerce` | 15 | Generic shop scaffold (smaller than `amazon`) |
| `saas` | 12 | Orgs, Memberships, Plans, Subscriptions, Invoices, ApiKeys, AuditLogs, Webhooks |
| `social` | 10 | Generic social scaffold (smaller than `facebook`) |
| `cms` | 12 | Pages, BlogPosts, Media, Menus, Forms, Settings, Themes, Widgets |
| `forum` | 10 | Categories, Topics, Posts, Reactions, Subscriptions, Badges, Reports |
| `crm` | 10 | Accounts, Contacts, Leads, Opportunities, Activities, Notes, Tasks, Pipelines |
| `helpdesk` | 8 | Customers, Tickets, Replies, KnowledgeArticles, SLAs, Tags |
| `everything` | ~80 | Every preset above unioned + dedup'd |

### Cyberpunk theme family

All four neon themes are self-contained CSS only — no JS — and pull
Orbitron + Share Tech Mono + VT323 from Google Fonts at runtime. They
inherit from the same `app/views/layouts/application.html.erb` chrome
that ships with `--auth`/`--admin`, so flash bars, nav, sign-in state,
admin tables, forms, and buttons all theme correctly:

| Theme | Palette | Notes |
|---|---|---|
| `cyberpunk` | Cyan + magenta + pink on `#05050a` | CRT scanlines, cyber-grid BG, animated shimmer headings — distilled from `docs/hud-static.css` |
| `synthwave` | Purple + pink + orange | Sunset gradient, no scanlines, retrowave-style headings |
| `terminal` | Green + amber on black | VT220 phosphor CRT — pairs especially well with `--api` mode |
| `matrix` | Bright green on black | CSS-only digital-rain background, vignette |

### Generators (run inside an existing app)

| Command | Effect |
|---|---|
| `s_web g scaffold Name field:type…` | Single resource: model + migration + 7-action controller + 5 ERB views |
| `s_web g app PRESET` | Bulk preset scaffold inside an existing tree |
| `s_web g auth` | Add User model + sessions + signup/login/logout |
| `s_web g admin` | Add `/admin` panel for every existing model |
| `s_web g api Name` | Add a `Api{Plural}Controller` returning JSON for an existing model |
| `s_web g controller Name [actions…]` | Empty controller + per-action view stubs |
| `s_web g model Name field:type…` | Model + create-table migration |
| `s_web g migration Name [field:type…]` | Standalone migration |
| `s_web g mailer Name [actions…]` | Mailer stub |
| `s_web g job Name` | Background-job stub |
| `s_web g channel Name` | WebSocket / SSE channel stub |
| `s_web g docker` | Multi-stage Dockerfile + .dockerignore |
| `s_web g ci` | GitHub Actions CI workflow |
| `s_web g pwa` | PWA manifest + service worker |
| `s_web build [--out DIR] [--name NAME]` | Generate a Rust wrapper crate that embeds every `.stk` via `include_str!`. Run `cargo build --release` inside it for a single fat binary with the entire framework + app + SQLite linked statically |

## Generated layout

```
myblog/
├── app/
│   ├── controllers/
│   │   ├── application_controller.stk     # base — every controller extends
│   │   └── posts_controller.stk           # PostsController, 7 REST actions
│   ├── models/
│   │   ├── application_record.stk         # base — every model extends
│   │   └── post.stk                       # Post class with typed fields
│   ├── views/
│   │   ├── layouts/application.html.erb
│   │   └── posts/{index,show,new,edit,_form}.html.erb
│   └── helpers/
│       └── application_helper.stk
├── bin/
│   └── server                             # `stryke bin/server` boots the app
├── config/
│   ├── routes.stk                         # `route "GET /posts" ~> "posts#index"`
│   ├── application.stk                    # boot config + middleware list
│   └── database.toml                      # per-env adapter + path
├── db/
│   ├── migrate/
│   │   └── TIMESTAMP_create_posts.stk     # `class CreatePosts extends Migration`
│   └── seeds.stk
├── public/
├── test/
├── log/, tmp/, vendor/                    # standard Rails dirs
└── stryke.toml                            # framework + dependency manifest
```

## Naming conventions (Rails-compatible)

| Concept | Form | Example |
|---|---|---|
| Model class | singular Pascal | `Post` |
| Model file | singular snake | `app/models/post.stk` |
| Controller class | plural Pascal + `Controller` | `PostsController` |
| Controller file | plural snake + `_controller.stk` | `app/controllers/posts_controller.stk` |
| Table | plural snake | `posts` |
| Migration class | plural Pascal verbed | `CreatePosts`, `AddPublishedToPosts` |
| Migration file | timestamp + snake_class | `db/migrate/20260430153012_create_posts.stk` |
| Views directory | plural snake | `app/views/posts/` |
| Route prefix | plural snake | `/posts`, `/posts/:id` |

The CLI's `pluralize` / `singularize` helpers handle the standard irregulars
(`person`/`people`, `child`/`children`, `mouse`/`mice`, `goose`/`geese`) plus
the regular `-s`/`-es`/`-ies` rules. Custom inflections will go in
`inflections.rs` once the framework runtime lands.

## Framework runtime — what's shipped

PASS 1 + PASS 2 land in `strykelang/web.rs` and dispatch through the
prefixed `web_*` builtins:

| Builtin | Behavior |
|---|---|
| `web_route VERB " /path", "ctrl#act"` | Register a route in the global router |
| `web_resources "posts"` | 7-route REST scaffold (index/show/new/create/edit/update/destroy) |
| `web_root "ctrl#act"` | Shortcut for `GET /` |
| `web_application_config(+{...})` | Boot-time config hash, read by middleware later |
| `web_render(html=>…)` / `(text=>…)` / `(json=>…)` | Direct response with content-type |
| `web_render(template => "posts/index", locals => +{…})` | Resolve `app/views/posts/index.html.erb`, run through ERB engine, wrap in `app/views/layouts/application.html.erb` if present |
| `web_redirect("/path", 302)` | 3xx with `Location` header |
| `web_params()` | Hashref of merged query + form-urlencoded + JSON body + path captures |
| `web_request()` | Hashref `{method, path, query, body, headers}` |
| `web_routes_table()` | Pretty-printed routes table (used by `s_web routes`) |
| `web_boot_application(port)` | TCP accept loop → dispatcher |
| `web_db_connect("sqlite://path")` | Open + cache the global DB connection |
| `web_db_execute(sql, [bindings])` | Run raw SQL — returns affected-row count |
| `web_db_query(sql, [bindings])` | Run raw SQL — returns arrayref of hashref rows |
| `web_model_all("posts")` | `SELECT * FROM posts ORDER BY id` |
| `web_model_find("posts", $id)` | Hashref or undef |
| `web_model_where("posts", +{title => "x"})` | Arrayref of matches |
| `web_model_create("posts", +{...})` | Insert + return new row (auto `created_at`/`updated_at`) |
| `web_model_update("posts", $id, +{...})` | Update by id, returns affected count |
| `web_model_destroy("posts", $id)` | Delete by id, returns affected count |
| `web_create_table("posts", +{title => "string"})` | DDL for migrations (`id` + timestamps auto-added) |
| `web_drop_table("posts")` / `web_add_column` / `web_remove_column` | Schema DSL for `up`/`down` blocks |
| `web_migrate()` / `web_rollback()` | Apply / unwind migrations; tracked in `schema_migrations` table |
| `web_h($s)` | HTML-escape (`&` `<` `>` `"` `'`) — wrap user content with this in `<%= %>` |
| `web_link_to(label, href)` / `web_button_to(label, action, method => "delete", confirm => …)` | Anchor / form-button helpers |
| `web_form_with(url => …, method => …)` / `web_form_close()` | Open + close `<form>` with `_method` hidden-input override |
| `web_text_field(name, value)` / `web_text_area` / `web_check_box` | Form input helpers — type chosen by the scaffold from migration field type |
| `web_csrf_meta_tag` / `web_stylesheet_link_tag("application")` / `web_javascript_link_tag` / `web_image_tag("logo.png")` | Asset + meta helpers |
| `web_truncate("…", length => 30)` / `web_pluralize(3, "post")` / `web_time_ago_in_words($ts)` | Text helpers |
| `web_validate($attrs, +{title => "presence,length:1..100", email => "format:^.+@.+$"})` | Returns `{ok=>1}` / `{ok=>0, errors=>{...}}` — `presence`, `length:MIN..MAX`, `format:REGEX`, `numericality`, `inclusion:a\|b\|c`, `confirmation:other` |
| `web_model_paginate("posts", page => 2, per_page => 25)` | `{rows, total, page, per_page, total_pages}` |
| `web_model_search("posts", "stryke", cols => ["title", "body"])` | LIKE %q% across listed columns |
| `web_model_soft_destroy("posts", $id)` | Sets `deleted_at` instead of deleting (auto-adds the column if missing) |
| `web_model_count` / `web_model_first` / `web_model_last` | Single-row helpers |
| `web_db_begin` / `web_db_commit` / `web_db_rollback` | Manual transaction control |
| `web_before_action("authenticate", controller => "PostsController", only => ["edit","update"])` | Filter chain — runs before each matching action; `web_after_action` is the symmetric form |
| `web_cache_get/set/delete/clear` | Process-local in-memory cache with optional `ttl => seconds` |
| `web_t("welcome.title", "en")` / `web_load_locale("en", +{...})` | i18n lookup with fallback to the key |
| `web_session` / `web_session_get/set/clear` / `web_set_cookie` / `web_cookies` | Cookie + session storage (base64 JSON, HttpOnly, SameSite=Lax) |
| `web_flash_set("notice", "Saved!")` / `web_flash_get("notice")` | Cross-redirect flash messages |
| `web_password_hash($pw)` / `web_password_verify($pw, $stored)` | SHA-256+salt password hashing |
| `web_permit($params, "title", "body")` | Strong params — whitelist accepted keys |
| `web_signed("payload")` / `web_unsigned($signed)` | HMAC-signed round-trippable strings (uses `secret_key_base` from app config) |
| `web_log("info", "msg")` / `web_set_header("X-Frame-Options", "DENY")` / `web_status(404)` / `web_uuid()` / `web_now()` | Misc response + log helpers |
| `web_jwt_encode(+{user_id=>1, role=>"admin"})` / `web_jwt_decode($token)` | HS256 JWT mint / verify (signed with `secret_key_base`) |
| `web_rate_limit("login:$ip", 5, 60)` | Token-bucket rate limit — returns 1 if allowed, 0 if exceeded |
| `web_otp_secret()` / `web_otp_generate($secret)` / `web_otp_verify($secret, $code)` | TOTP 2FA — RFC 6238, 30s window, ±1 step skew tolerance |
| `web_markdown($text)` (alias `web_md`) | CommonMark subset: headings, **bold**, *italic*, `code`, [links](url), fenced code, lists, blockquotes, hr |
| `web_etag("payload")` | Compute + emit ETag, return 1 if request matched (auto 304 + skip render) |
| `web_csv($rows)` | Hashref array → CSV string with quoting; accepts both `\@rows` and `@rows` |
| `web_faker_name/email/sentence/paragraph/int(min, max)` | Test/seed data generators |
| `web_model_increment("posts", $id, "comments_count", 1)` | Atomic counter cache update |
| `web_model_with("posts", "user")` | Single IN-query eager load (`$post->{_user}` populated, no n+1) |
| `web_content_for("title", "...")` / `web_yield_content("title")` | Rails-style content blocks for layouts |
| `web_render_partial("posts/form", +{post=>$p})` | Resolve `app/views/posts/_form.html.erb` and render with locals |
| `web_security_headers()` | Set X-Frame-Options, X-Content-Type-Options, Referrer-Policy, HSTS, Permissions-Policy |
| `web_openapi()` | Returns the live route table as an OpenAPI 3.0 hashref (serve via `web_json`) |
| `web_token_for($user_id, "reset", 3600)` / `web_token_consume($token, "reset")` | Signed time-limited tokens for password reset / email verify links |
| `web_can("posts.edit", $user)` | Role + permissions check (`role => "admin"` short-circuits) |
| `/openapi.json` | Auto-served — OpenAPI 3.0 doc generated from the live route table |
| `/docs` | Auto-served — Swagger UI loading `/openapi.json` |
| `/docs/redoc` | Auto-served — Redoc rendering of `/openapi.json` |
| `web_jsonapi_resource("posts", $row)` / `web_jsonapi_collection("posts", $rows)` / `web_jsonapi_error(404, "not_found", "msg")` | JSON:API envelope helpers |
| `web_bearer_token()` | Returns the `Authorization: Bearer X` token (pair with `web_jwt_decode`) |

Default-convention render: an action that calls neither `web_render`
nor `web_redirect` auto-renders `app/views/{resource}/{action}.html.erb`.

ERB syntax recognised: `<%= expr %>` interpolates, `<% stmt %>` runs
for side effects, `<%# comment %>` is dropped, `<%- ... -%>` trims
surrounding whitespace. Control structures (`for`/`if`/`while`) span
multiple tags because the engine compiles all segments into one stryke
program before execution.

## What's still TODO

1. **PASS 3 — ORM.** `Model` base class + SQLite adapter. `Post::all`,
   `Post::find($id)`, `Post::where(field => v)`, `$p->save`,
   `$p->destroy`. Until this lands, controller actions render with
   placeholder data and form posts redirect-without-persist.
2. **PASS 4 — Migrator.** `Migrator::new->migrate / rollback / reset`
   builtin. `create_table` / `add_column` DSL. Schema versioning.
3. **PASS 5 — View helpers + mailers/jobs/channels.** `link_to`,
   `form_with`, `csrf_meta_tag`, `stylesheet_link_tag`, `flash`. Plus
   `s_web g mailer`, `s_web g job`, `s_web g channel`.
4. **Generator polish** — strong params, route helper generation
   (`posts_path`, `new_post_path`), test stubs, fixtures, system test
   skeleton.
5. **Asset pipeline** — embed CSS/JS at build time (per WEB_FRAMEWORK.md).
6. **Encrypted credentials** — `config/credentials.stk.enc` + master.key.
7. **`s_web new --api`** — skip views/helpers/layout for API-only apps.
