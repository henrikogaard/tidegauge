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
- Resource activity now reports per-process interrupt-wakeup rates alongside CPU
  and disk I/O, time-weighted over the same 60-second window with its own
  coverage seconds. These are reported kernel counts, not energy use;
  package-idle wakeups remain unvalidated and unavailable, and denied or new
  counters show a dash
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).

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
- Detail pages now share one template: a hero card with an uppercase caption,
  a 42 px headline value, a 130 px filled chart, and stat triplets, followed
  by sectioned attribution and supplementary content. Disk gains the hero its
  layout lacked, per-app GPU attribution is stated as unavailable instead of
  implied, and Bluetooth and Sound use the same sectioned list anatomy.
  The Neural Engine tab is folded into the GPU page as a section; saved tab
  settings simply skip it. Captions render at 10 px uppercase with correct
  light-mode contrast, and unavailable measurements use a compact empty-state
  card instead of paragraphs. The footer gives Settings, Processes, and Tools
  one bordered control style: Settings left, live counts centred, the two
  panels grouped right
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Homebrew formula actions now verify the keg's `INSTALL_RECEIPT.json` through
  the same trusted, bounded read used for cask metadata. A missing, malformed,
  symlinked, or untrusted receipt keeps the formula in the inventory but
  disables its actions with an explicit reason; touching the receipt after the
  cached snapshot fails preflight before any manager command runs
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Package post-action matching no longer treats a legitimate change to the
  manager prefix directory's size or mtime as proof of failure. Outcome
  confirmation now matches the same installation — identical source category,
  manager and runtime fingerprints, and prefix canonical path, device, inode,
  ownership, and mode — while pre-action checks still require the exact cached
  identity
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- A failed Homebrew inventory scan now explains that the scan is bounded and
  that Refresh installed retries it without changing installed packages
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Chart rendering is unified on two idioms: the Oversikt cards now draw the
  same filled sparkline as the detail tabs instead of a dense bar strip, and
  the shared area chart keeps headroom with a stroked line so a maxed-out or
  constant series (e.g. a full battery) no longer reads as a solid block. The
  Historikk chart marks empty buckets with a faint baseline tick instead of
  leaving an unexplained gap, and a largely empty selected range says so in a
  localized line
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Controls now share one anatomy across the app: bordered 28 px buttons with
  transparent rest state, segmented radio groups for mutually exclusive
  choices (appearance, launcher visibility, menu readout styles, History
  range and metric, the Tools page row, baseline windows, package source and
  sort filters, and process filters), and grouped steppers for alert
  thresholds and output volume. Arrow-only reorder buttons carry
  context-rich localized labels. The Disk hero reports system-wide disk
  activity instead of free capacity, physical capacity moves into the device
  identity card, and APFS volumes share one container header only when the
  reported metadata is consistent — shared space is never summed per volume.
  Projects gains a count summary and a sectioned list with a shared empty
  state; History uses section headers, shared empty states, and a labeled
  Recording details card that keeps every truthfulness note
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Development-server state is now a typed value (working / idle / barely
  used) instead of a parsed localized string, and project and summary counts
  count distinct server processes rather than listening ports. Menu-bar
  label slots are font-measured once per label instead of assuming a fixed
  width, so localized labels such as MINNE and EFF size correctly
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Keyboard activation now prevents the framework-generated second click, so
  Enter/Space invokes actions once and modified activation keys are ignored
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- The Tools Startup page bounds the rendered list to 20 registrations per
  page with Previous/Next navigation so every entry stays reachable; the
  collected inventory itself is never truncated
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), in review).
- Storage scans stop promptly when cancelled mid-walk, verified after real
  metadata traversal rather than only from a pre-set flag. When a scan's
  coverage is unknown — the folder could not be opened or stat'ed, or the scan
  ended with nothing readable — the status row now states that no size totals
  are available instead of presenting them as 0 B
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
