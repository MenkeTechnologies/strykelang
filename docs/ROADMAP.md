# stryke Roadmap

**The hottest language ever created. Literally.**

This document outlines the path to mass worldwide adoption. ✅ marks shipped, ⏳ marks deferred or in-progress, ⏭️ marks future phases not started.

## Phase 1 — Foundations — ✅ MOSTLY SHIPPED
1. ✅ Package registry MVP: parse stryke.toml, set up ~/.stryke/store/, local path deps.
2. ✅ CLI surface: s new, s init, s add, s remove, s install (plus update, outdated, audit, vendor, clean, run, install -g).
3. ✅ AOT binary output via Cranelift: native ELF/Mach-O/PE.
4. ✅ s bench harness.
5. ✅ s fmt + s check basics.
6. ✅ Stress testing: throttle command for live intensity adjustment.
7. ⏳ Stress testing: mTLS controller/agent communication — current model is plaintext over TCP.
8. ✅ Stress testing: audit log for compliance — `audit_log("event", ...)` JSONL append, `~/.stryke/audit.log` or `$STRYKE_AUDIT_LOG`.
9. ✅ Stress testing: auto-terminate guards (max temp, max runtime, ack timeout).

## Phase 2 — AI primitives (single provider) — ✅ SHIPPED
10. ✅ prompt and stream_prompt builtins, Anthropic + others.
11. ✅ embed builtin + sqlite-vec backend.
12. ✅ Result cache + ai_cost tracking.
13. ✅ tool fn declaration with build-time JSON schema.
14. ✅ ai builtin: agent loop with local tools.
15. ✅ ai_mock_install / STRYKE_AI_MODE=mock-only for tests.

## Phase 3 — Web framework MVP — ✅ SHIPPED (core), ⏳ stress-metrics integration deferred
16. ✅ HTTP/1.1 server (`web_serve`).
17. ✅ Radix-trie router compiled from DSL.
18. ✅ Per-request scratch buffers.
19. ✅ Middleware (logger, security headers, ETag, CSRF).
20. ✅ ORM with chain API (SQLite default; Postgres/MySQL via builtins).
21. ✅ Migrations DSL (`web_create_table`/`add_column`/`web_migrate`).
22. ✅ ERB templates (`<%= %>` / `<% %>` / `<%- -%>`) + layouts + partials.
23. ✅ s_web new generator (`s_web new myapp --app everything --theme cyberpunk --auth --admin --docker --ci --pwa --migrate`).
24. ✅ Stress testing: Prometheus /metrics export — `stress_metrics_prometheus()` returns text-exposition format ready for a `/metrics` handler.
25. ✅ Stress testing: CSV/JSON metrics export — `stress_metrics_csv()`, `stress_metrics_json()`, `stress_metrics_export($path, format => ...)`.
26. ✅ Stress testing: live metrics streaming — `stress_metrics_watch(field => "...", interval_ms => 1000, max_ticks => 60)`.

## Phase 4 — MCP integration — ✅ SHIPPED
27. ✅ mcp_connect client (stdio + http; ws deferred).
28. ✅ mcp_server declarative DSL + programmatic `mcp_server_start`.
29. ✅ s build --mcp-server standalone server flag — wraps the script + `mcp_serve_registered_tools(name)` and bundles into a fat binary that speaks MCP stdio JSON-RPC.
30. ✅ Auto-attach connected MCP servers to ai calls.
31. ✅ AI collection builtins: ai_filter, ai_map, ai_classify, ai_sort, ai_match, ai_dedupe with batching.

## Phase 5 — Production web + K8s deploy — ⏳ PARTIAL
32. ⏳ HTTP/2 via h2, TLS via rustls.
33. ⏳ WebSockets + SSE first-class — SSE wired (`web_sse`); WS not yet.
34. ✅ Background jobs (DB-backed queue) — `web_jobs_init`, `web_job_enqueue`, `web_job_dequeue`, `web_job_complete`, `web_job_fail`, `web_jobs_list`, `web_jobs_stats`, `web_job_purge`.
35. ✅ Encrypted secrets — `secrets_encrypt`/`secrets_decrypt` (AES-256-GCM), `secrets_random_key`, `secrets_kdf` (PBKDF2-HMAC-SHA256, 600k iters default).
36. ✅ CSRF/CSP/HSTS security middleware (`web_security_headers`, `web_csrf_meta_tag`).
37. ✅ Embedded static asset pipeline (`web_static`).
38. ⏭️ Stress testing: Alpine container image + static binary.
39. ⏭️ Stress testing: DaemonSet manifest.
40. ⏭️ Stress testing: sidecar injection mode.
41. ⏭️ Stress testing: Helm chart.
42. ⏭️ Stress testing: Kubernetes Operator.
43. ⏭️ Stress testing: Grafana dashboard.

## Phase 6 — Tooling for adoption — ⏳ PARTIAL
44. ✅ LSP server (rust-analyzer-class) — full hover/completion surface in `strykelang/lsp.rs`.
45. ⏭️ DAP debugger integration.
46. ⏭️ Profiler with flamegraphs and allocation tracking — `zpwrFlame` works at the shell level; in-process language profiler deferred.
47. ⏳ Advanced linter (n+1, hot-loop AI calls, dead code).

## Phase 7 — Multi-provider + local AI — ✅ MOSTLY SHIPPED
48. ✅ OpenAI provider (chat, tool calls, streaming, Whisper, TTS, image gen, Files API, moderation).
49. ✅ Gemini provider (`gemini-2.5-flash` default).
50. ⏳ Local fallback via embedded llama.cpp — Ollama + LM Studio (`openai_compat`) cover the local path today; in-process linkage deferred.
51. ✅ [ai.routing] per-operation provider selection.

## Phase 8 — Hot reload + effects — ⏭️ NOT STARTED
52. Hot module replacement (Cranelift recompile in place).
53. Effects system (algebraic, opt-in).
54. Capability-based runtime (FilesystemCap, NetCap, AICap).

## Phase 9 — Heavyweights — ⏭️ NOT STARTED (numerical partially landed)
55. Embeddable mode (--embed build target).
56. ⏳ Numerical/data stack: native vector ops + SIMD + n-d arrays — `vec_*` cosine/search/topk shipped; n-d arrays + SIMD broadcast deferred.
57. GPU codegen via Cranelift → SPIR-V/PTX.
58. Notebook protocol (Jupyter-compatible kernel).
59. CRDTs as first-class types.

## Phase 10 — Migration shims — ⏭️ NOT STARTED
60. PyPI bridge: call Python packages from stryke.
61. npm bridge: call Node packages.
62. Rails-app importer.
63. JVM interop bridge for enterprise Java.

## Phase 11 — Top-class performance + enterprise tier — ⏭️ NOT STARTED
64. Thread-per-core runtime + io_uring (Linux).
65. SIMD HTTP parser.
66. kTLS for static asset serving.
67. HTTP/3 / QUIC default-on for TLS.
68. Stress testing: RHEL certification.
69. Stress testing: AWS/Azure/GCP marketplace listings.
70. Stress testing: compliance docs (SOC 2, PCI DSS, ISO 27001, FedRAMP).
71. Stress testing: BCP/DR procedure templates and validators.
72. Stress testing: enterprise support tier — revenue stream activation.

## Phase 12 — Reach — ⏭️ NOT STARTED
73. WASM target (--target=wasm32-wasi).
74. Lambda runtime adapter.
75. Time-travel debugging (record + replay).
76. First-class grammars (Raku-style).
77. Schema-typed embedded SQL.

---

## Phase 3: Enterprise Ready (Q3-Q4 2026)

### Kubernetes Native
- [ ] Official Helm Chart
- [ ] Kubernetes Operator
- [ ] DaemonSet templates
- [ ] Sidecar injector webhook
- [ ] CRDs for StrykeTest, StrykeAgent

### Authentication & Security
- [ ] Pre-shared key authentication
- [ ] mTLS support
- [ ] Token-based auth (OAuth2/OIDC)
- [ ] RBAC for controller commands
- [ ] Audit logging to external systems

### Cloud Marketplaces
- [ ] AWS Marketplace listing
- [ ] Azure Marketplace listing
- [ ] GCP Marketplace listing
- [ ] DigitalOcean 1-Click App

### Certifications
- [ ] RHEL certification
- [ ] Ubuntu Pro certification
- [ ] CIS benchmark compliance
- [ ] SOC 2 Type II audit

---

## Phase 4: Ecosystem (2027)

### IDE Support
- [ ] VS Code extension (LSP-based)
- [ ] IntelliJ plugin
- [ ] Neovim plugin
- [ ] Syntax highlighting for 10+ editors

### Package Management
- [ ] stryke package registry
- [ ] Dependency management
- [ ] Private registries
- [ ] Security scanning

### Observability
- [ ] Prometheus metrics exporter
- [ ] OpenTelemetry integration
- [ ] Grafana dashboards
- [ ] PagerDuty/OpsGenie integration

### Advanced Features
- [ ] GPU stress testing (CUDA/ROCm)
- [ ] Power consumption metrics
- [ ] Temperature monitoring
- [ ] Network stress testing (internal)

---

## Phase 5: Global Scale (2027+)

### Enterprise Support
- [ ] 24/7 support contracts
- [ ] Professional services
- [ ] Training and certification
- [ ] Custom development

### Community
- [ ] stryke Foundation
- [ ] Annual conference
- [ ] Regional meetups
- [ ] Contributor program

### Adoption Metrics Targets
- [ ] 10,000 GitHub stars
- [ ] 1,000 enterprise deployments
- [ ] 100 countries with active users
- [ ] 1M downloads/month

---

## Non-Goals

These are explicitly **not** on the roadmap:

- **Application performance testing** — Use k6, Locust, Gatling
- **Network DDoS** — stryke is for infrastructure validation, not attacks
- **GUI** — CLI-first, always
- **Windows support** — Unix-only, unapologetic

---

## Contributing

1. Pick an item from Phase 3 or 4
2. Open an issue to discuss approach
3. Submit PR with tests
4. Get code reviewed

---

## Contact

- **Repository:** https://github.com/MenkeTechnologies/strykelang
- **Issues:** https://github.com/MenkeTechnologies/strykelang/issues
- **Discussions:** https://github.com/MenkeTechnologies/strykelang/discussions

---

*The hottest language ever created. 100% TDP — beware.*
