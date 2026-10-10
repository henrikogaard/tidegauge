# Tidegauge

A menu-bar system monitor for macOS, written in Rust with [gpui-kit](https://crates.io/crates/gpui-kit). Tidegauge groups processes by their owning app and presents system activity in a compact panel.

## Project status

Tidegauge is an early development build. This repository contains the app source and recovered development notes. Packaging, automated tests, and some controls are incomplete. See the limitations below before relying on a metric or action.

## Build and run

Requirements:

- macOS. The app uses Mach, libproc, IOKit, CoreAudio, and AppKit directly.
- A Rust toolchain with Cargo and the macOS developer tools installed.
- The macOS utilities `lsof`, `nettop`, `system_profiler`, `route`, and `osascript` available to the process.

Run a development build from the repository root:

```sh
cargo run
```

For an optimized build:

```sh
cargo build --release
./target/release/tidegauge
```

The app opens a floating panel and adds readouts to the menu bar. It runs without a Dock icon. Click a readout to bring the panel forward. When using `cargo run`, stop the app with Ctrl-C in the launching terminal.

There is currently no bundle script, `.app` packaging, signing, or notarization workflow in this repository.

## What the app contains

| Area | Current implementation |
|---|---|
| Menu bar | CPU, memory, and network readouts by default; other metrics can be selected in the settings file |
| Overview | Summary tiles and memory and power breakdowns |
| Metric tabs | CPU, Memory, Disk, Network, GPU, Neural, and Battery |
| App grouping | CPU, memory, disk counters, and estimated power grouped by executable bundle or parent app |
| Sound | Output device and volume display, plus process audio state |
| Bluetooth | Connected devices and available battery levels from `system_profiler` |
| Projects | Listening processes grouped by project directory, with Stop actions |
| History | SQLite samples of headline metrics and up to 60 apps, with a 30-day retention target |
| Alerts | Notifications for sustained CPU, memory, disk, and network thresholds, with a shared 10-minute cooldown |

## Current limitations

- System disk throughput and per-app network rates need correction in their collectors. Treat these readouts as incomplete.
- Project directory detection and CPU-based idle detection need correction. Project status may report idle even while a server is busy.
- Project Stop and Stop All actions send SIGTERM immediately. They have no confirmation dialog.
- Sound is currently a display. Volume and per-app mute controls are not connected to the UI, and the level meter is an indicative value rather than a measured signal level.
- Neural utilization is unavailable. GPU and battery detail depend on hardware and available system properties. Per-app watts are estimates derived from process counters.
- The CPU tab's uptime is time since the sampler started, rather than system uptime.
- Menu-item styles, custom alert thresholds, and launch-at-login settings are stored but not applied. Clicking a menu item brings the panel forward; it does not hide it.
- History is written to disk, but the panel shows in-memory charts. There is no history browser, and pruning is periodic rather than a strict 30-day cutoff.
- There is no user-facing Quit menu. Keep the launching terminal available during development.

## Settings and local data

| Data | macOS location |
|---|---|
| Settings | `~/Library/Application Support/no.ogard.tidegauge/settings.json` |
| History | `~/Library/Application Support/no.ogard.tidegauge/history.db` |
| Cached app icons | `~/Library/Caches/no.ogard.tidegauge/icons/` |

The app uses defaults when the settings file is absent or cannot be parsed. There is currently no settings screen. To select menu-bar metrics, create a complete settings file such as:

```json
{
  "dark": null,
  "menu_items": [
    { "metric": "cpu", "style": "both" },
    { "metric": "memory", "style": "value" },
    { "metric": "network", "style": "value" }
  ],
  "alert_cpu_pct": 90.0,
  "alert_mem_pct": 92.0,
  "launch_at_login": false
}
```

Available metric identifiers are `cpu`, `memory`, `network`, `disk`, `battery`, `gpu`, and `power`. The menu-bar pump reloads this file while the app runs. The `dark` preference is also not currently applied; the panel follows the macOS appearance.

History includes app names and resource usage. The sampler records headline metrics and the first 60 app groups sorted by memory usage approximately every five sampling passes. Sampling starts a new process and chart baseline on each launch; renaming the storage namespace does not migrate older settings or history.

## Architecture

The sampler runs on a background thread. It collects metrics into a shared `Snapshot`, which the panel and status-bar pump read. Each pass waits two seconds after its work completes; external utilities can lengthen that interval. Bluetooth collection runs every ten passes, per-app network collection every other pass, and history persistence every five passes.

| File | Responsibility |
|---|---|
| `src/main.rs` | Application entry point, panel creation, status-bar pump |
| `src/app.rs` | Sampler lifecycle, shared panel handle, notifications |
| `src/engine.rs` | Counter deltas, snapshot model, in-memory chart histories |
| `src/ffi.rs` | Raw macOS C API declarations and layouts |
| `src/metrics.rs` | System CPU, memory, disk, network, GPU, battery, and chip collectors |
| `src/processes.rs` | Process enumeration and app grouping |
| `src/projects.rs` | Listening ports, project grouping, idle tracking, per-app network collection |
| `src/audio.rs` | CoreAudio device and process properties |
| `src/bluetooth.rs` | Bluetooth JSON parsing and cache |
| `src/history.rs` | SQLite persistence and alert thresholds |
| `src/settings.rs` | Settings model, defaults, and JSON persistence |
| `src/statusbar.rs` | Native menu-bar items and sparklines |
| `src/ui.rs` | Panel navigation, tab content, and charts |
| `src/theme.rs`, `src/icons.rs` | Design tokens and app-icon caching |

## Development documentation

- [Contributing](CONTRIBUTING.md): checks, manual verification, and review expectations.
- [Design](DESIGN.md): colors, layout, and intended menu-bar behavior.
- [Recovery manifest](RECOVERY-MANIFEST.md): provenance of the recovered files.
- [Historical findings](tidegauge-findings.md) and [historical test plan](tidegauge-test-plan.md): evidence from an earlier environment, not validation of the current checkout.

## License

Tidegauge is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 Henrik Øgård.
