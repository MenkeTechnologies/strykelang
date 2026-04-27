# stryke Roadmap

**The hottest language ever created. Literally.**

This document outlines the path to mass worldwide adoption.

Phase 1 — Foundations
1. Package registry MVP: parse stryke.toml, set up ~/.stryke/store/, local path deps.
2. CLI surface: s new, s init, s add, s remove, s install.
3. AOT binary output via Cranelift: native ELF/Mach-O/PE.
4. s bench harness.
5. s fmt + s check basics.
6. Stress testing: throttle command for live intensity adjustment.
7. Stress testing: mTLS controller/agent communication.
8. Stress testing: audit log for compliance.
9. Stress testing: auto-terminate guards (max temp, max runtime, ack timeout).

Phase 2 — AI primitives (single provider)
10. prompt and stream_prompt builtins, Anthropic-only.
11. embed builtin + sqlite-vec backend.
12. Result cache + ai_cost tracking.
13. tool fn declaration with build-time JSON schema.
14. ai builtin: agent loop with local tools.
15. ai_mock blocks for tests.

Phase 3 — Web framework MVP
16. HTTP/1.1 server on hyper + tokio.
17. Radix-trie router compiled from DSL at build time.
18. Per-request arena allocator.
19. Basic middleware (logger, compression).
20. Postgres ORM with chain API.
21. Migrations DSL.
22. AOT-compiled templates (#{} interpolation, <% %> blocks).
23. s new myapp --web generator.
24. Stress testing: Prometheus /metrics export from controller.
25. Stress testing: CSV/JSON metrics export.
26. Stress testing: live metrics streaming (metrics --watch=temp).

Phase 4 — MCP integration
27. mcp_connect client (stdio/ws/http).
28. mcp_server declarative DSL.
29. s build --mcp-server standalone server flag.
30. Auto-attach connected MCP servers to ai calls.
31. AI collection builtins: ai_filter, ai_map, ai_classify, ai_sort, ai_match with batching.

Phase 5 — Production web + K8s deploy
32. HTTP/2 via h2, TLS via rustls.
33. WebSockets + SSE first-class.
34. Background jobs (DB-backed queue).
35. Encrypted secrets.
36. CSRF/CSP/HSTS security middleware.
37. Embedded static asset pipeline.
38. Stress testing: Alpine container image + static binary.
39. Stress testing: DaemonSet manifest.
40. Stress testing: sidecar injection mode.
41. Stress testing: Helm chart.
42. Stress testing: Kubernetes Operator.
43. Stress testing: Grafana dashboard.

Phase 6 — Tooling for adoption
44. LSP server (rust-analyzer-class).
45. DAP debugger integration.
46. Profiler with flamegraphs and allocation tracking.
47. Advanced linter (n+1, hot-loop AI calls, dead code).

Phase 7 — Multi-provider + local AI
48. OpenAI provider.
49. Gemini provider.
50. Local fallback via embedded llama.cpp.
51. [ai.routing] per-operation provider selection.

Phase 8 — Hot reload + effects
52. Hot module replacement (Cranelift recompile in place).
53. Effects system (algebraic, opt-in).
54. Capability-based runtime (FilesystemCap, NetCap, AICap).

Phase 9 — Heavyweights
55. Embeddable mode (--embed build target).
56. Numerical/data stack: native vector ops + SIMD + n-d arrays.
57. GPU codegen via Cranelift → SPIR-V/PTX.
58. Notebook protocol (Jupyter-compatible kernel).
59. CRDTs as first-class types.

Phase 10 — Migration shims
60. PyPI bridge: call Python packages from stryke.
61. npm bridge: call Node packages.
62. Rails-app importer.
63. JVM interop bridge for enterprise Java.

Phase 11 — Top-class performance + enterprise tier
64. Thread-per-core runtime + io_uring (Linux).
65. SIMD HTTP parser.
66. kTLS for static asset serving.
67. HTTP/3 / QUIC default-on for TLS.
68. Stress testing: RHEL certification.
69. Stress testing: AWS/Azure/GCP marketplace listings.
70. Stress testing: compliance docs (SOC 2, PCI DSS, ISO 27001, FedRAMP).
71. Stress testing: BCP/DR procedure templates and validators.
72. Stress testing: enterprise support tier — revenue stream activation.

Phase 12 — Reach
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
