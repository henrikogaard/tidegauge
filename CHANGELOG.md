# Changelog

User-visible changes are recorded here. **Unreleased is not installed by the
published updater.** Entries describe implemented scope, not completion of all
hardware acceptance checks. A merge and a published release are separate events.

## Unreleased

### Added

- Configurable, opt-in alerts with sustained thresholds and independent cooldowns;
  a persisted system/app History browser with visible gaps; safer development
  project Stop confirmation and outcome reporting; timed keep-awake; dashboard
  favorites, compact layout, and tab order/visibility
  ([#24](https://github.com/henrikogaard/tidegauge/pull/24), merged).
- Supported default-media-output volume/mute controls. These do **not** control
  arbitrary apps ([#24](https://github.com/henrikogaard/tidegauge/pull/24), merged).
- App/helper inspection with observed ancestry, remaining-helper explanations,
  normal app Quit, and confirmed selected-helper SIGTERM. No force kill,
  automatic cleanup, or bundle-only ownership assumptions
  ([#25](https://github.com/henrikogaard/tidegauge/pull/25), merged).
- Tools: startup registration inspection and System Settings handoff; current
  parent/UID/start-time explanations and possible startup associations; saved
  5/15/30-minute system baselines; recent CPU/disk activity; cancellable read-only
  selected-folder storage inspection
  ([#34](https://github.com/henrikogaard/tidegauge/pull/34), merged).
- Apps & Packages: source filters, sorting, app and managed-package inventory,
  explicit update checks, and guarded single-item manager actions. Fresh identity
  checks, conservative cask eligibility, and explicit uncertainty prevent
  unsupported actions. Standalone apps use Reveal/own-updater guidance
  ([#34](https://github.com/henrikogaard/tidegauge/pull/34), merged).
- On-demand process signing identifier and Team ID, read off the UI thread with
  process identity checks and a bounded session cache. Metadata is **not** a
  signature-validity, developer-identity, or malware verdict
  ([#34](https://github.com/henrikogaard/tidegauge/pull/34), merged).

### Fixed

- Application metadata now falls back to per-key extraction when whole-file
  `plutil` JSON conversion fails. Valid Info.plist files containing `<data>` or
  `<date>` values no longer produce a parse warning or drop the application
  from the Apps & Packages inventory
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Battery capacity now reads the nested `BatteryData` dictionary used by newer
  Apple silicon firmware, restoring design/full capacities and health where
  the top level exposes only normalized percentages. A present battery with no
  reported health condition reads "No condition reported" instead of
  "Unavailable"
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Temperature channel discovery is bounded to 512 channels instead of 128.
  Because the controller enumerates keys in ASCII order, the old bound
  silently dropped whole `T…` families on hardware with larger inventories.
  The temperature card now renders at most 24 channels, hottest first with
  unavailable channels last, plus an exact shown/total/unavailable caption
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Remaining user-visible English strings are now localized to Norwegian
  Bokmål: overview card stat labels, memory/power breakdown cards, GPU and
  Neural Engine availability text, Bluetooth device kinds and counts, dev-server
  status sentences, session-uptime durations with singular/plural forms, and
  history range chips
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Audio mute actions read fresh device state before applying a change.
- App-instance attribution preserves multiple main processes and requires
  observed ancestry before offering remaining-helper Stop.
- Startup override parsing accepts explicit boolean and textual states; absent
  overrides remain unknown.
- Package discovery and cask target validation handle verified installed
  metadata while rejecting mismatched, stale, unsafe, or ambiguous identities.

### Boundaries and remaining acceptance

- Direct third-party startup disable/remove/restore remains unavailable.
- Per-app audio routing and manual fan control are not implemented.
- Baselines are system-level; per-app identity/history migration and validated
  wakeup/energy attribution are not implemented.
- Physical sensor, battery and disk telemetry were validated on Apple silicon
  hardware against independent native diagnostics. Audio control and actual
  sign-in launch checks remain open.
- Native invalid-signature rejection was verified on hardware using ad-hoc
  signed QA bundles: a valid update installed first, then a size-preservingly
  tampered archive was rejected at signature verification with no update
  applied. The unreachable-feed error/recovery test passed after 0.2.0. No
  production feed, key, or signed artifact was changed for either test.
- Package actions have not been exercised against real installed packages.
  Test mutations only in disposable environments.

See the [roadmap](https://github.com/henrikogaard/tidegauge/issues/12),
[process semantics](docs/PROCESSES.md), and
[release verification](docs/RELEASING.md#upgrade-acceptance).

## [0.2.0](https://github.com/henrikogaard/tidegauge/releases/tag/v0.2.0) — 2026-10-10

First published release; includes the monitoring foundation from the unpublished
0.1.0 candidate below.

### Added

- Persistent System/Light/Dark appearance selection.
- Opt-in native launch-at-login registration and status on macOS 13+.
- Signed, notarized drag-and-drop DMG with an Applications-folder shortcut;
  retained signed ZIP for Sparkle updates.
- Six-bar SVG app artwork and matching monochrome menu-bar icon.
- Native alerts using Tidegauge's name and logo, in English and Norwegian Bokmål.

### Verified delivery

- Signed/notarized artifacts, installation, public downloads, and the real
  0.1.0 candidate → 0.2.0 production-feed Sparkle upgrade, including relaunch and
  bounded settings/History preservation checks. See the
  [release verification report](https://github.com/henrikogaard/tidegauge/releases/download/v0.2.0/VERIFICATION.md).
- This does not establish physical telemetry accuracy, actual sign-in launch,
  or invalid-signature rejection.

Implementation: [#22](https://github.com/henrikogaard/tidegauge/pull/22),
[#23](https://github.com/henrikogaard/tidegauge/pull/23).

## 0.1.0 — unpublished candidate

- Native menu-bar dashboard, English/Norwegian interface, light/dark layouts,
  metric colors, responsive navigation, system/app activity and local History
  recording.
- Read-only hardware-dependent temperatures/fan RPM, per-drive inventory and
  health, and battery health/charging information.
- Production signing, notarization, signed appcast verification, archived
  screenshots, and key-recovery documentation.

This candidate remained a draft; it was used as the baseline for the verified
upgrade to 0.2.0. Its original [candidate notes](docs/RELEASE-0.1.0.md) describe
the validation state at that time, not today's release status.
