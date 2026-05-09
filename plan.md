# syswatch product roadmap

## Where we are — v0.3.1 (today)

12 tabs (Overview, CPU, Memory, Disks, FS, Procs, GPU, Power, Services, Net, Timeline, Insights), 6 themes, 2 graph styles, persistent config + settings popup, hot-reloadable refresh rate, full GPU stack (Apple Silicon temp/power via IOReport+SMC, AMDGPU sysfs depth, opt-in NVIDIA via nvml, 5 GPU insights). Tier-1 product is solid.

## Phase 1 — Workflow polish (v0.4.x, 1–2 sessions)

The "finish what's already half-built" phase. Closes visible footer hints, fills the most-asked gaps, no new architecture.

- **Per-process GPU usage** column on Procs tab — marquee, the biggest "obvious next thing" signal you can ask for. nvml `running_processes()` on Linux NVIDIA, `/proc/*/fdinfo` parsing for AMDGPU, ioreg per-IOAccelerator `Tasks` subdicts on Apple Silicon.
- **Per-process network bandwidth** column on Procs — pull from netwatch-sdk's existing per-PID accounting (eBPF on Linux, ProcessNetworkUsage on macOS).
- **Help dialog** (`?` key) — closes the footer hint, surfaces every hotkey we've added.
- **Procs filter** (`/` key) — closes another footer hint, big productivity win.
- **Snapshot to JSON** (`S` key) — dump current Snapshot to `~/.local/state/syswatch/snap-{ts}.json`. First piece of the recording story.
- **Per-tab refresh rates** — Procs/Net don't need 1Hz; Timeline benefits from 5Hz.

## Phase 2 — Recording, diff, intelligence (v0.5.x, 2–3 sessions)

Workflow features that turn syswatch from "live monitor" into "diagnostic tool."

- **Session recording** — binary format (postcard or rkyv) capturing the full Snapshot ring + insight transitions. `R` toggles; output to `~/.local/state/syswatch/sessions/`.
- **Replay mode** — `syswatch --replay session.swr` opens with timeline scrubbing across the entire recorded window, no live collection.
- **Diff mode** — `syswatch diff a.swr b.swr` produces a side-by-side or delta view of two snapshots/sessions. Useful for "before vs after" deployments.
- **Smarter insights** — sustained-anomaly streaks ("CPU above 80% for 12 of the last 15 ticks"), trend detection (memory growing 50 MB/min), correlated insights ("disk IO spike correlates with mongod CPU spike").
- **Optional LLM insights** — mirror netwatch's pattern (Ollama/local-first, OpenAI optional). Insight summaries in plain English, drill-in suggestions.

## Phase 3 — Platform breadth (v0.6.x, 3–4 sessions)

Reach the long tail of platforms and workload contexts.

- **Container awareness** — detect containerd/Docker/Podman/runc via cgroups; group procs by container, show per-container CPU/mem/IO/net.
- **Kubernetes context** — when running inside a pod, surface namespace/pod/container labels; when running on a node, group by pod via cgroup paths.
- **Windows discovery + collection** — WMI for hardware enumeration, perf counters for live data, GPU via DXGI / ADL / nvml-wrapper. Largest single platform expansion.
- **BSD support** — kvm + sysctl wrappers; mostly a translation of the Linux paths.
- **Apple ANE utilization** — we already have ANE power from IOReport; util needs the IOReport ANE channel. Useful for ML developers running on-device models.
- **GPU clocks + fan** — round out the GPU surface (sysfs/nvml expose both; quick win).
- **Multi-display + integrated/discrete classification** — small UX wins on the GPU tab.

## Phase 4 — Integration + remote (v0.7.x → v0.9.x, months)

Mirror netwatch's architecture: local-first remains the default, but a fleet story unlocks the same OSS funnel.

- **syswatch-agent** — headless collector exposing Snapshots over a wire format (mirror netwatch-agent's design). Listens on a local socket or pushes to syswatch-cloud.
- **syswatch-cloud** (private repo, mirrors netwatch-cloud business model) — fleet view across N hosts, alerts, retention. Funded by a paid plan; OSS local syswatch remains forever free.
- **IDE plugins** — VS Code status-bar widget showing CPU/mem/GPU + a "open syswatch" command. JetBrains plugin would mirror.
- **Public library crate** — `syswatch-collector` published to crates.io so other tools can reuse the per-platform collection without depending on the TUI.
- **Plugin system** — out-of-process subscribers to Snapshot stream over Unix socket. Lets users build custom views, exporters, alert backends without forking.

## v1.0 — sealed (~6–9 months out)

What 1.0 means concretely:
- Stable Snapshot wire format (bumped only with explicit deprecation cycles).
- Stable CLI flags + config keys.
- All 4 first-class platforms (Linux x86_64+aarch64, macOS aarch64+x86_64, Windows x86_64, FreeBSD x86_64) supported and CI-tested.
- Documented insight thresholds, with config knobs to tune them.
- syswatch-cloud-compatible agent shipped at >=v0.9.

## Explicit non-goals

Important to say out loud so scope doesn't drift:

- **Not htop / top** — different audience. We're for engineers diagnosing across a session, not sysadmins watching one process right now.
- **Not Datadog / Grafana** — local-first. Cloud is opt-in, not the default. No metrics scraping language, no alerting DSL.
- **Not a profiler** — observability ("what's happening, when, why"), not micro-optimization (`perf record`).
- **Not security / forensics** — process names + IO rates, not syscall traces or capability audits.
- **Not Kubernetes-first** — K8s context is supported, but the product is for any UNIX-like host. The cluster-monitor space is crowded and competitive.

## Cross-cutting (every phase)

- **Test coverage** — currently 106 tests; should grow proportionally with each feature. Target 200+ by v1.0.
- **CI matrix** — add Windows + FreeBSD as the platform support lands.
- **Demo loop** — every release ships an updated `demo.tape` showing the new feature in 30 seconds. Drives the OSS-funnel marketing strategy.
- **Performance budget** — collector tick should stay <10 ms on a 16-core / 8 GPU / 1k-proc machine. Profile before each release.

---

**Recommended near-term plan**: ship Phase 1 in 2 releases (`v0.4.0` per-process GPU + per-process net, `v0.4.1` help/filter/snapshot/per-tab-refresh), then take a beat to look at usage signal before committing to Phase 2's scope. That gives you four releases in fast succession to keep the OSS funnel warm, and lets you defer the bigger recording/cloud architecture until Phase 1 confirms which workflow gaps users actually hit.
