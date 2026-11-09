# Open Antigravity (AI Machine Directives & Topology)
> Token-optimized specification and architecture handbook for autonomous coding agents.

This repository enforces the **Q.E.D. High-Assurance Engineering Standard**.
All autonomous agents must operate in **Mode A (Quality > Speed)** by default.

---

## 1. Project Identity & Architecture

Open Antigravity is a high-performance, zero-external-dependency portable utility written in Rust. It selectively routes traffic between the Google Antigravity editor/CLI and Google AI backend services without routing general system traffic.

### Primary Data Flow & Subsystems
- **Selective Network Gate (`src/network/`)**:
  - `dns_forwarder.rs` / `doh.rs`: Intercepts DNS queries for targeted Google AI domains.
  - `resolvers/`: Multi-provider racing DNS resolution with live substitution verification.
  - `proxy.rs`: Local HTTP/CONNECT loopback proxy (127.0.0.1) forwarding Google API endpoints.
  - `routes.rs`: Route health table, failure penalties, and latency tracking.
- **Patcher & Watchdog (`src/patcher/`)**:
  - `ide.rs` & `asar.rs`: Patches IDE Electron extension bundles and manifests safely with backup files.
  - `binary.rs`: Hex pattern matching and atomic byte substitution in `agy` language server binaries.
  - `watchdog.rs`: Directory settle monitor detecting editor updates and auto-reapplying patches.
- **Platform Integration (`src/platform/`)**:
  - Windows: Name Resolution Policy Table (NRPT) rules, scheduled task service, UAC elevation helpers.
  - Linux / SteamOS: Hosts-based forwarding, desktop files, D-Bus StatusNotifierItem (KDE/GNOME).
- **Presentation (`src/gui/`, `src/tui/`)**:
  - GUI: Pure Rust immediate-mode `eframe`/`egui` with DirectX 12 (WARP fallback) on Windows and Glow/OpenGL on Linux.
  - TUI: Terminal interface built with `ratatui` + `crossterm` for headless/SSH environments.

---

## 2. CLI Flags & Operational Contracts

- `-h, --help`: Displays CLI usage options.
- `-v, --version`: Prints current semantic version (`VERSION` file) and canary build token.
- `--tui, --cli`: Launches the terminal user interface without spawning a GUI window.
- `--check, --diagnose`: Performs comprehensive local self-test and emits structured diagnostics.
- `--about, --license`: Prints licensing and origin metadata.
- Internal worker flags:
  - `--forwarder`: Spawns headless DNS forwarder process.
  - `--watchdog`: Spawns background IDE watchdog process.
  - `--proxy`: Spawns background local loopback proxy.

---

## 3. High-Assurance Invariants (Mode A)

1. **Evidence Before Assertions**: Every code change must be verified via real terminal executions (`cargo test`, `qed check`). Never assert success without real stdout proof.
2. **The 3 AM Test**: Flat guard clauses, explicit intermediate variables, early returns. File size ceiling: <= 1,500 lines.
3. **Pristine Root**: Zero scratch files or test dumps in repository root. All temporary data belongs in `data/` or target directories.
4. **Cache & Resource Hygiene**: All LRU caches must specify `maxSize` and `ttlMs`. Guaranteed cleanup of temporary files (`.tmp` -> atomic rename).
5. **Contract Stability**: Public re-exports in `src/main.rs` and domain facades must remain 100% backward-compatible.

---

## 4. Verification Gate Commands

```bash
# Rapid compile & typecheck
cargo check

# Unit & integration test execution
cargo test

# Q.E.D. mechanical verification gate
qed check --strict

# Full repository runtime health check
qed doctor
```
