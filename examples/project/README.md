# STRYKE RECON

A cyberpunk system reconnaissance tool built entirely in stryke. Scans your machine, network, git repos, analyzes threats, and generates a PDF report — all from one command.

## Run

```sh
s examples/project/main.stk
```

Output:
```
╔═══════════════════════════════════════╗
║        STRYKE RECON v1.0              ║
╚═══════════════════════════════════════╝

[PHASE 1] SYSTEM SCAN
──────────────────────────────────────────
  HOST     codelabs-arm
  OS       macos aarch64
  CPUs     18 cores (12P + 6E)
  RAM      64 GB
  UPTIME   2d 14h 23m

[PHASE 2] DISK RECON
──────────────────────────────────────────
  /            ████████████░░░░░░░░  62.3% 350 GB free
  /System      ██████████████████░░  91.2% 8 GB free

[PHASE 3] NETWORK SCAN
──────────────────────────────────────────
  IPv4     10.59.0.1
  MAC      a4:cf:99:xx:xx:xx
  GATEWAY  10.59.0.1
  DNS      1.1.1.1, 8.8.8.8

[PHASE 4] PROCESS INTEL
[PHASE 5] GIT INTEL
[PHASE 6] THREAT ANALYSIS

  [WARN] /System is 91.2% full
  [OK]   All other systems nominal

[PHASE 7] REPORT
  report: /tmp/stryke_recon_12345.pdf
```

## Test

```sh
s test examples/project/t
```

## Features Used

- **System builtins**: `pool_info`, `mem_total`, `sys_uptime`, `gethostname`, `os_name`
- **Disk**: `mounts`, `human_bytes`
- **Network**: `net_ipv4`, `net_mac`, `net_gateway`, `net_dns_servers`, `net_interfaces`
- **Process**: `process_list`
- **Git** (libgit2): `git_root`, `git_branches`, `git_log`, `git_files`, `git_authors`
- **Enums**: `enum ThreatLevel { HIGH, WARN, OK }`
- **Functions**: `fn scan_system`, `fn analyze_threats`, `fn generate_report`
- **PDF generation**: `to_pdf`
- **Terminal art**: `cyber_banner`, colored output with ANSI
- **Testing**: `assert_eq`, `assert_gt`, `assert_ok`, `assert_contains`, `test_run`
