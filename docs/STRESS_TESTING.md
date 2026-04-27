# stryke Distributed Load & Infrastructure Testing

> *"The hottest language ever created. Literally."*
>
> *"100% TDP — beware."*

stryke is a **server farms first** language — the first programming language designed from the ground up for distributed infrastructure load testing, capacity validation, and BCP/DR exercises.

**Not HTTP load testing. Not API benchmarks. BARE METAL HEAT.**

- Pin every core to 100% TDP
- Instant fire, instant terminate
- Single binary, zero dependencies
- Interactive REPL control
- Works on RHEL, Ubuntu, Alpine, any Linux

## Stress Testing Builtins

stryke ships with native stress testing primitives that **pin ALL cores to 100% TDP**:

```stk
# CPU stress — SHA256 hashing across ALL cores
stress_cpu(10)           # 10 seconds, returns total hash count

# Memory stress — allocate and touch across ALL cores
stress_mem(1e9)          # 1GB distributed across cores

# IO stress — parallel write/read temp files
stress_io("/tmp", 100)   # 100 iterations per core

# Combined stress test — CPU + memory + IO
my $r = stress_test(10)  # 10 seconds
p "Hashes: $r->{cpu_hashes}, Duration: $r->{duration}s"
```

**The `heat` function — maximum thermal stress:**

```stk
heat(60)     # 60 seconds of pure thermal assault
```

Output:
```
🔥 HEAT: Pinning 18 cores to 100% TDP for 60s (Ctrl-C to stop early)
🔥 HEAT: 3,116,320,000 hashes in 60.00s (51.9M/s across 18 cores)
```

**Measured performance (Apple M3 Max, 18 cores):**

| Function | Result | CPU Usage |
|----------|--------|-----------|
| `stress_cpu(3)` | 154M hashes | 1117% (all cores) |
| `stress_mem(1e9)` | 1GB touched | 452% (parallel) |
| `heat(60)` | 3.1B hashes | 1800% (max TDP) |

This is bare metal heat. Every core working. Maximum TDP.

**stryke is the hottest language ever created. Literally.**

## Distributed Stress Testing

Combine with `cluster` + `pmap_on` for distributed load:

```stk
# Build worker pool across 3 servers (8 workers each = 24 total)
my $c = cluster(["server1:8", "server2:8", "server3:8"])

# Saturate all 24 workers for 60 seconds
1:24 |> pmap_on $c { stress_cpu(60) }

# Or use stress_test with cluster
my $r = stress_test($c, 60)
p "Total hashes: $r->{cpu_hashes}, Workers: $r->{workers}"
```

This is real, working code. `cluster` opens persistent SSH connections to each host, spawns `stryke --remote-worker` processes, and `pmap_on` distributes work across all slots with work-stealing.

## Agent/Controller Architecture

stryke ships with a complete distributed load testing system:

### Controller (Master REPL)

```sh
stryke controller                    # listen on 0.0.0.0:9999
stryke controller --port 8888        # custom port
stryke controller --bind 10.0.0.1    # specific interface
```

**Commands:**

| Command | Description |
|---------|-------------|
| `status` | List connected agents with cores, memory, state |
| `fire [SECS]` | Start stress test (default: 10 seconds) |
| `terminate` | Stop stress test on all agents |
| `shutdown` | Disconnect all agents and exit |

### Agent (Worker Daemon)

```sh
stryke agent                              # use config file
stryke agent --controller 10.0.0.1        # connect to specific host
stryke agent --port 9999                  # specific port
stryke agent -c /path/to/agent.toml       # custom config
```

**Config file:** `~/.config/stryke/agent.toml`

```toml
[controller]
host = "controller.example.com"
port = 9999

[limits]
max_temp = 85       # auto-terminate if CPU temp exceeds (Celsius)
max_duration = 3600 # max seconds per stress session

[agent]
name = "node-01"    # optional, defaults to hostname
```

### Example Session

```
# Terminal 1: Start controller
$ stryke controller
stryke controller listening on 0.0.0.0:9999
Waiting for agents...

[agent connected] node-01 (cores=64, session=1)
[agent connected] node-02 (cores=64, session=2)
[agent connected] node-03 (cores=64, session=3)

controller> status
AGENT                 CORES     MEMORY        STATE       UPTIME
------------------------------------------------------------
node-01                  64       256GB         idle         42s
node-02                  64       256GB         idle         38s
node-03                  64       256GB         idle         35s

Total: 3 agents, 192 cores, 0 firing

controller> fire 60
[fire] 3 agents, duration=60s

controller> status
AGENT                 CORES     MEMORY        STATE       UPTIME
------------------------------------------------------------
node-01                  64       256GB       FIRING        102s
node-02                  64       256GB       FIRING         98s
node-03                  64       256GB       FIRING         95s

Total: 3 agents, 192 cores, 3 firing

controller> terminate
[terminate] 3 agents

controller> shutdown
[shutdown] all agents disconnected
```

## Roadmap

The following features are planned:

- Prometheus metrics export (`/metrics` endpoint)
- Kubernetes Operator for agent deployment
- Helm chart for k8s deployment
- Grafana dashboard
- RHEL certification

---

## What stryke Validates

### Hardware Layer

| Component | Validation Goal | How stryke Tests It |
|-----------|-----------------|---------------------|
| **Cooling capacity** | CRAC units handle sustained full load | Sustained CPU load, monitor thermals |
| **Power distribution** | PDU rated for peak simultaneous draw | Full cluster load, monitor power metrics |
| **UPS/Generator** | Backup power handles failover correctly | Sustained load through switchover |
| **Server hardware** | No thermal throttling under load | Per-node temperature monitoring |

### Infrastructure Layer

| Component | Validation Goal | How stryke Tests It |
|-----------|-----------------|---------------------|
| **Network fabric** | Sufficient bandwidth for peak traffic | Inter-pod traffic under load |
| **Storage IOPS** | Shared storage meets SLA under pressure | Parallel IO from every container |
| **k8s scheduler** | Graceful handling of resource pressure | Real pressure from inside cluster |
| **Autoscaling** | HPA/VPA respond within SLA | Load spike, measure scale-up time |

### Operational Layer

| Component | Validation Goal | How stryke Tests It |
|-----------|-----------------|---------------------|
| **Alerting** | Monitoring detects load conditions | Verify alerts fire as expected |
| **Runbooks** | Procedures are current and effective | Execute procedures under load |
| **Response times** | Team responds within SLA | Measure time from load to response |
| **DR procedures** | Failover works as documented | Full BCP exercise with load |

---

## Architecture

### Controller/Agent Model

```
┌─────────────────────────────────────────────────────────────────┐
│                        MASTER REPL                               │
│  fire / terminate / status / throttle                           │
└─────────────────────────┬───────────────────────────────────────┘
                          │ TCP/Unix socket
        ┌─────────────────┼─────────────────┐
        ▼                 ▼                 ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│    AGENT      │ │    AGENT      │ │    AGENT      │
│  (container)  │ │  (container)  │ │  (container)  │
│               │ │               │ │               │
│ polls master  │ │ polls master  │ │ polls master  │
│ executes work │ │ executes work │ │ executes work │
│ reports stats │ │ reports stats │ │ reports stats │
└───────────────┘ └───────────────┘ └───────────────┘
     Pod A             Pod B             Pod C
     Node 1            Node 1            Node 2
```

### Agent Lifecycle

1. **Dormant** — agent starts with container, connects to master, waits
2. **Armed** — master sends workload definition, agent compiles (or loads from SQLite cache)
3. **Firing** — agent executes workload, pins cores to 100% TDP
4. **Reporting** — agent streams metrics back to master (CPU%, temp, memory)
5. **Released** — master sends terminate, agent returns to dormant

### Communication Protocol

```
MASTER                           AGENT
   │                               │
   │──── HELLO ───────────────────►│  (version, session_id)
   │◄─────────────── HELLO_ACK ────│  (hostname, cores, memory)
   │                               │
   │──── ARM(workload) ───────────►│  (stryke source or cached hash)
   │◄─────────────── ARM_ACK ──────│  (compiled, ready)
   │                               │
   │──── FIRE ────────────────────►│
   │◄─────────────── METRICS ──────│  (streaming: cpu, temp, mem)
   │◄─────────────── METRICS ──────│
   │◄─────────────── METRICS ──────│
   │                               │
   │──── TERMINATE ───────────────►│
   │◄─────────────── TERM_ACK ─────│  (final stats)
   │                               │
```

---

## Deployment

### Zero-Install Deployment

`stryke build` bakes your entire codebase — workloads, libraries, configurations — into a single static binary. Ship one file to any cluster. No stryke installation required on target machines.

```sh
# Build self-contained binary with embedded workloads
stryke build agent.stk -o stryke-agent

# scp to any Linux host and run — no dependencies
scp stryke-agent node1:/usr/local/bin/
ssh node1 '/usr/local/bin/stryke-agent --controller controller:9999'
```

The binary includes:
- stryke runtime (~13MB static)
- Your agent code and workload definitions
- All imported modules and libraries (via `register_virtual_module`)

`require`/`use` calls resolve against embedded virtual modules — no filesystem access needed on target machines.

### Container Image

```dockerfile
FROM alpine:latest
COPY stryke-agent /usr/local/bin/stryke-agent
ENTRYPOINT ["/usr/local/bin/stryke-agent", "--controller", "stryke-controller:9999"]
```

Single static binary. No runtime dependencies. Works on RHEL, Ubuntu, Alpine, any Linux.

### Kubernetes DaemonSet

```yaml
apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: stryke-agent
  namespace: stryke-system
spec:
  selector:
    matchLabels:
      app: stryke-agent
  template:
    metadata:
      labels:
        app: stryke-agent
    spec:
      containers:
      - name: agent
        image: ghcr.io/menketechnologies/stryke:latest
        args: ["agent", "--master", "stryke-master.stryke-system:9999"]
        resources:
          requests:
            cpu: "10m"
            memory: "16Mi"
          limits:
            cpu: "64"        # no limit when firing
            memory: "128Gi"  # no limit when firing
        securityContext:
          privileged: false
          readOnlyRootFilesystem: true
```

### Sidecar Injection

For per-pod granularity, inject stryke agent as a sidecar:

```yaml
spec:
  containers:
  - name: app
    image: your-app:latest
  - name: stryke-agent
    image: ghcr.io/menketechnologies/stryke:latest
    args: ["agent", "--master", "stryke-master:9999"]
```

---

## Master REPL Commands

| Command | Description |
|---------|-------------|
| `status` | List connected agents with hostname, cores, state |
| `fire` | Start workload on all agents |
| `fire node1,node2` | Start workload on specific nodes |
| `fire --cores=50%` | Limit to 50% of cores per agent |
| `terminate` | Stop workload on all agents |
| `terminate node1` | Stop workload on specific node |
| `throttle 75%` | Adjust running workload intensity |
| `metrics` | Show live metrics stream |
| `history` | Show past stress test sessions |
| `export FILE` | Export metrics to CSV/JSON |

### Example Session

```
$ stryke master --bind 0.0.0.0:9999
stryke master v0.9.0
listening on 0.0.0.0:9999
agents: 0

[agent connected: node1 (64 cores, 256GB)]
[agent connected: node2 (64 cores, 256GB)]
[agent connected: node3 (64 cores, 256GB)]
agents: 3 (192 cores total)

master> status
NODE     CORES  MEM     STATE    CPU%  TEMP
node1    64     256GB   dormant  2%    42°C
node2    64     256GB   dormant  3%    41°C
node3    64     256GB   dormant  2%    43°C

master> fire
[arming 3 agents...]
[firing...]

NODE     CORES  MEM     STATE    CPU%  TEMP
node1    64     256GB   firing   100%  78°C
node2    64     256GB   firing   100%  76°C
node3    64     256GB   firing   100%  79°C

master> ^C
[terminating...]
[released]

master> export stress_test_001.json
[exported 3 agents, 847 data points]
```

---

## Workload Types

### CPU Stress (default)

```stk
~>1:∞ pmaps { sha256(rand_bytes(1e6)) }
```

Pure compute. Saturates all cores with cryptographic hashing.

### Memory Pressure

```stk
my @blocks = 1:1000 |> pmap { rand_bytes(1e8) }  # 100GB
```

Allocate and touch memory. Tests OOM killer, swap, memory pressure eviction.

### IO Stress

```stk
~>1:∞ pmaps { 
    my $f = "/tmp/stress_" . rand_int(1000);
    write_file($f, rand_bytes(1e6));
    slurp($f);
    unlink($f);
}
```

Random read/write. Tests storage IOPS, filesystem, shared volumes.

### Network Stress

```stk
~>1:∞ pmaps {
    fetch("http://internal-service/health");
}
```

Saturate internal network. Tests service mesh, load balancers, DNS.

### Combined (Realistic)

```stk
~>1:∞ pmaps {
    my $data = fetch_json("http://api/data/" . rand_int(1e6));
    my $processed = heavy_transform($data);
    post_json("http://api/results", $processed);
}
```

Simulates real application workload at 100% intensity.

---

## BCP/DR Testing

### Datacenter Failover

```
# Terminal 1: Stress primary datacenter
$ stryke master --cluster mahwah
master> fire

# Terminal 2: Monitor secondary
$ watch kubectl --context=sandy-springs get pods

# Terminal 1: Watch primary metrics, initiate failover
master> metrics
# ... observe degradation ...

# Initiate DNS failover / traffic shift
$ kubectl apply -f failover-to-secondary.yaml

# Terminal 1: Release primary
master> ^C

# Validate secondary handled the load
```

### Generator Switchover

```
master> fire --duration=10m   # sustained load
# ... UPS battery drains ...
# ... generator kicks in ...
# ... ATS switches ...
master> metrics              # watch for blips during switchover
```

### Thermal Monitoring

```
master> fire
master> metrics --watch=temp

NODE     TEMP    STATUS
node47   78°C    nominal
node47   82°C    nominal
node47   85°C    WARNING: approaching configured limit

master> terminate   # graceful shutdown at configured threshold
```

---

## Safety

### Kill Switch

Ctrl-C from master REPL immediately sends TERMINATE to all agents. Workloads stop within milliseconds.

### Automatic Limits

```stk
# Agent config
agent {
    max_temp    => 85,      # auto-terminate if exceeded
    max_runtime => "30m",   # auto-terminate after duration
    require_ack => true,    # master must ACK every 60s or terminate
}
```

### Network Isolation

Run controller on management network. Agents only accept connections from authorized controller IPs. mTLS supported for production deployments.

### Audit Log

Every command logged with timestamp, user, affected nodes:

```
2026-04-25T13:45:00Z user=bob cmd=fire nodes=* cores=192
2026-04-25T13:47:23Z user=bob cmd=terminate nodes=*
```

---

## Metrics Export

### Prometheus

```yaml
# stryke master exposes /metrics
- job_name: 'stryke-master'
  static_configs:
  - targets: ['stryke-master:9999']
```

Metrics: `stryke_agents_total`, `stryke_cores_firing`, `stryke_cpu_percent`, `stryke_temp_celsius`

### Grafana Dashboard

Import dashboard ID `XXXXX` for pre-built stress test visualization:

- Agent count over time
- CPU% heatmap by node
- Temperature trends
- Memory pressure
- Network throughput

### CSV/JSON Export

```
master> export --format=csv stress_001.csv
master> export --format=json stress_001.json
```

---

## Comparison

| Feature | stryke | stress-ng | Locust | k6 | Chaos Monkey |
|---------|--------|-----------|--------|-----|--------------|
| Distributed native | ✓ | ✗ | ✓ | ✓ | ✓ |
| Inside-cluster agents | ✓ | ✗ | ✗ | ✗ | ✓ |
| Interactive REPL | ✓ | ✗ | ✗ | ✗ | ✗ |
| Instant fire/release | ✓ | ✗ | ✗ | ✗ | ✗ |
| Custom workloads | ✓ | limited | ✓ | ✓ | ✗ |
| Hardware stress | ✓ | ✓ | ✗ | ✗ | ✗ |
| Lines to deploy | 1 | 10+ | 50+ | 20+ | 100+ |
| Full TDP saturation | ✓ | ✓ | ✗ | ✗ | ✗ |

---

## Security & Compliance

stryke supports enterprise compliance and governance requirements:

### Capacity Validation

Prove to auditors and stakeholders that your infrastructure handles peak load:

| Test | What It Validates |
|------|-------------------|
| Sustained CPU load | Cooling capacity, thermal headroom |
| Memory pressure | OOM handling, eviction policies |
| Storage saturation | IOPS limits, failover behavior |
| Network throughput | Fabric capacity, QoS policies |

### Compliance Use Cases

- **SOC 2** — Demonstrate infrastructure resilience controls
- **PCI DSS** — Validate capacity planning for transaction peaks
- **ISO 27001** — Test business continuity procedures
- **FedRAMP** — Prove availability under stress conditions

### Test Authorization

Load tests should be coordinated with:
- Infrastructure/ops teams
- Change management (if required by your org)
- Cloud provider (some require notification for sustained high load)

Document:
- Scope (which clusters, which nodes)
- Duration (start time, max duration)
- Limits (max CPU%, kill switch holder)
- Rollback (who can terminate, how fast)

---

## Enterprise Support

- **RHEL certified** — (roadmap)
- **AWS Marketplace** — (roadmap)
- **Azure Marketplace** — (roadmap)
- **GCP Marketplace** — (roadmap)
- **Kubernetes Operator** — (roadmap)
- **Helm Chart** — (roadmap)

Contact: enterprise@menketechnologies.com

---

## License

MIT
