# stryke Roadmap

**The hottest language ever created. Literally.**

This document outlines the path to mass worldwide adoption.

---

## Phase 1: Foundation (Q2 2026) ✅

### Core Language
- [x] Perl 5 compatible interpreter
- [x] fusevm bytecode VM
- [x] Block JIT compilation (Cranelift)
- [x] Fused superinstructions
- [x] NaN-boxed values
- [x] Three-tier regex

### Performance
- [x] Rayon work-stealing parallelism
- [x] Streaming `pmaps` for bounded memory
- [x] 3200+ auto-imported builtins
- [x] Faster than V8, LuaJIT on most benchmarks

### Caching
- [x] SQLite bytecode cache
- [x] zstd compression
- [x] mtime invalidation
- [x] Cache management builtins

---

## Phase 2: Server Farms First (Q2 2026) ✅

### Stress Testing Builtins
- [x] `stress_cpu` — SHA256 all-core saturation
- [x] `stress_mem` — Memory pressure testing
- [x] `stress_io` — Parallel file I/O stress
- [x] `stress_test` — Combined stress workload
- [x] `heat` — Maximum TDP, Ctrl-C termination

### Distributed Load Testing
- [x] Agent/Controller architecture
- [x] `stryke agent` daemon mode
- [x] `stryke controller` REPL
- [x] Binary wire protocol
- [x] Metrics streaming

### Documentation
- [x] RFCs for protocol, builtins, deployment
- [x] Enterprise-focused documentation
- [x] Compliance mapping (SOC 2, PCI DSS, ISO 27001)

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
