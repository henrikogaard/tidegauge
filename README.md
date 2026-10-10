# Tidegauge

A menu-bar system monitor for macOS, written in Rust with [gpui-kit](https://crates.io/crates/gpui-kit). Tidegauge groups processes by their owning app and presents system activity in a compact panel.

## Project status

Tidegauge is an early development build. This repository contains the app source and recovered development notes. Packaging and some controls are incomplete. Collector regression tests cover the fixes described in [CONTRIBUTING](CONTRIBUTING.md). See the limitations below before relying on a metric or action.

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

The app opens a resizable dashboard and adds one item to the menu bar. It runs without a Dock icon. Click the menu-bar item and choose Open Dashboard to bring it forward. Closing the window hides it while collection continues. The single menu-bar item opens a native menu with Open Dashboard, Settings, and Quit. Every dashboard tab has a Settings button in the footer. Command-comma also opens Settings, and Command-Q quits while the dashboard is focused. A per-user lock prevents duplicate instances; launching again leaves the existing app running.

There is currently no bundle script, `.app` packaging, signing, or notarization workflow in this repository.

## What the app contains

| Area | Current implementation |
|---|---|
| Menu bar | One Tidegauge icon by default; optional labeled readings share the same native item |
| Overview | Summary tiles and memory and power breakdowns |
| Metric tabs | CPU, Memory, Disk, Network, GPU, Neural, and Battery |
| App grouping | CPU, memory, disk counters, and estimated power grouped by executable bundle or parent app |
| Sound | Output device and volume display, plus process audio state |
| Bluetooth | Connected devices and available battery levels from `system_profiler` |
| Projects | Development listeners owned by the current user, grouped by project directory, with Stop actions |
| History | SQLite samples of headline metrics and up to 60 apps, with a 30-day retention target |
| Alerts | Notifications for sustained CPU, memory, disk, and network thresholds, with a shared 10-minute cooldown |

## Current limitations

- Project Stop and Stop All actions send SIGTERM immediately. They have no confirmation dialog.
- Sound is currently a display. Volume and per-app mute controls are not connected to the UI, and the level meter is an indicative value rather than a measured signal level.
- Neural utilization is unavailable. GPU and battery detail depend on hardware and available system properties. Per-app watts are estimates derived from process counters.
- Custom alert thresholds and launch-at-login settings are stored but not applied. Closing the dashboard hides it while collection continues.
- History is written to disk, but the panel shows in-memory charts. There is no history browser, and pruning is periodic rather than a strict 30-day cutoff.

Projects requires a working directory within four levels of a Git root or a recognized manifest (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, `Gemfile`, or `composer.json`). Known development runtimes and custom executables inside that project qualify. Desktop app bundles, other users' processes, root/home directories, and listeners without a project marker are excluded. Servers launched from unrelated folders will not appear.

## Settings and local data

| Data | macOS location |
|---|---|
| Settings | `~/Library/Application Support/no.ogard.tidegauge/settings.json` |
| History | `~/Library/Application Support/no.ogard.tidegauge/history.db` |
| Cached app icons | `~/Library/Caches/no.ogard.tidegauge/icons/` |

Open Settings using the dashboard footer, the app menu, or Command-comma. Each metric can be Off, Icon, Value, Graph, or Value + graph. Optional readings appear together in one native item, with labels for values and graphs, consistent spacing, and fixed widths for changing values. Changes are saved immediately and applied on the next refresh. Use Reset to icon only to clear extra readings. Existing preferences are preserved on upgrade.

You can hide the Tidegauge icon while a metric is enabled; it stays visible if every metric is Off so Settings and Quit remain reachable. Graphs show a dash until enough samples are available. K/s, M/s, G/s, and T/s are byte rates using powers of 1024.

The app uses defaults when the settings file is absent or cannot be parsed. You can also edit the file directly:

```json
{
  "dark": null,
  "show_app_icon": true,
  "menu_items": [],
  "alert_cpu_pct": 90.0,
  "alert_mem_pct": 92.0,
  "launch_at_login": false
}
```

Available metric identifiers are `cpu`, `memory`, `network`, `disk`, `battery`, `gpu`, and `power`. The menu-bar pump reloads this file while the app runs. The `dark` preference is also not currently applied; the panel follows the macOS appearance.

History includes app names and resource usage. The sampler records headline metrics and the first 60 app groups sorted by memory usage approximately every five sampling passes. Sampling starts a new process and chart baseline on each launch; renaming the storage namespace does not migrate older settings or history.

## Architecture

The sampler runs on a background thread. It publishes each completed `Snapshot` for the panel and status-bar pump to read without waiting for the collectors. Per-app network rates persist between network sampling passes. Each pass waits two seconds after its work completes; external utilities can lengthen that interval. Bluetooth collection runs every ten passes, per-app network collection every other pass, and history persistence every five passes.

| File | Responsibility |
|---|---|
| `src/main.rs` | Application entry point, panel creation, status-bar pump |
| `src/instance.rs` | Per-user process lock preventing duplicate monitors |
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
- [Design](DESIGN.md): colors, layout, and menu-bar behavior.
- [Recovery manifest](RECOVERY-MANIFEST.md): provenance of the recovered files.
- [Historical findings](tidegauge-findings.md) and [historical test plan](tidegauge-test-plan.md): evidence from an earlier environment, not validation of the current checkout.

## License

Tidegauge is licensed under the [MIT License](LICENSE).

Copyright (c) 2026 Henrik Øgård.
