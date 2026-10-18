# Changelog

User-visible changes are recorded here. **Unreleased is not installed by the
published updater.** Entries describe implemented scope, not completion of all
hardware acceptance checks. A merge and a published release are separate events.

## Unreleased

### Added

- Tidegauge spots an app whose memory keeps growing: at least 1 GB and half
  again over three hours, along a steady climb (not a swing that frees memory
  again), in an app that has not been quit and reopened and now holds at least
  1 GB. The Memory page lists it under Growing memory with a Quit button that
  asks first, and an opt-in "Memory keeps growing" alert (off by default, once
  per app, ten-minute cooldown) says so. The readings are kept in memory once
  a minute, so a warning needs three hours of Tidegauge running.

## [0.3.8](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.8) — 2026-10-18

### Added

- New Overview cards for Temperature, Fans and Power, off until chosen in
  Settings → Overview. The menu-bar panel shows the tiles chosen in Settings
  → Menu-bar panel, from CPU, memory, network, disk, GPU, battery,
  temperature, fans and power, and grows or shrinks to fit them
  ([#84](https://github.com/henrikogaard/tidegauge/pull/84)).
- The CPU page shows each core's use, grouped by core type with macOS's own
  names (Super, Performance, Efficiency), and each group's clock speed. The
  GPU page shows the GPU's core count and clock speed. Clock speeds come from
  macOS's performance-state counters and need no special permission
  ([#83](https://github.com/henrikogaard/tidegauge/pull/83)).

### Changed

- The fan bar shows speed as a share of the fan's top speed, as Stats and
  iStat Menus do, so a fan at its lowest speed no longer reads 0%
  ([#82](https://github.com/henrikogaard/tidegauge/pull/82)).

### Fixed

- The label inside Overview's power ring no longer spills over the ring's
  edge ([#82](https://github.com/henrikogaard/tidegauge/pull/82)).

## [0.3.7](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.7) — 2026-10-17

### Removed

- The Neural Engine reading. On an M5 Max with macOS 27 neither macOS's
  energy counters nor `powermetrics` report it, so it never showed anything
  there ([#80](https://github.com/henrikogaard/tidegauge/pull/80)).

## [0.3.6](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.6) — 2026-10-16

### Added

- Chip, SSD and battery temperatures are kept in History (Chip °C, SSD °C,
  Battery °C), and an opt-in alert fires when the chip stays above a chosen
  temperature (95 °C by default) for 30 seconds
  ([#77](https://github.com/henrikogaard/tidegauge/pull/77)).
- Each fan shows where its speed sits between its own lowest and highest
  ([#77](https://github.com/henrikogaard/tidegauge/pull/77)).
- Per-app volume and mute on the Sound page (macOS 14.2 or later), with a
  level meter for each app Tidegauge carries. macOS asks once for permission
  to capture audio; nothing is recorded or saved, and at 100% or when
  Tidegauge quits the app plays on its own again
  ([#78](https://github.com/henrikogaard/tidegauge/pull/78)).

### Fixed

- After sleep, the sensors reconnect instead of relying on connections opened
  before it ([#77](https://github.com/henrikogaard/tidegauge/pull/77)).

## [0.3.5](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.5) — 2026-10-16

### Added

- Started from outside Applications (Downloads, a disk image), Tidegauge
  offers once to move itself there and relaunch, so launch at login and
  updates work. The old copy goes to the Trash; Don't ask again is kept
  ([#75](https://github.com/henrikogaard/tidegauge/pull/75)).
- Measured system power: the whole Mac's draw as its power controller reports
  it, on battery or on the adapter. It shows on the Battery page, in the
  menu bar's power reading and in History. Macs without a battery do not
  report it ([#73](https://github.com/henrikogaard/tidegauge/pull/73)).
- Measured GPU power from macOS's GPU energy counter, on the GPU page and
  Overview's GPU card. Neural Engine power shows too on Macs where its
  energy counter moves ([#73](https://github.com/henrikogaard/tidegauge/pull/73)).
- The Sound page can switch the output and input device, as macOS Sound
  settings do, and controls the input's (microphone's) volume and mute where
  the device allows it ([#74](https://github.com/henrikogaard/tidegauge/pull/74)).

## [0.3.4](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.4) — 2026-10-16

### Added

- The CPU page leads with the temperatures macOS names itself: chip hottest
  and average, SSD and battery. The SMC channel list stays behind Show
  channels, since some of its codes are not temperatures
  ([#62](https://github.com/henrikogaard/tidegauge/pull/62)).
- The Memory page shows macOS's own memory pressure, normal, warning or
  critical in Activity Monitor's colours, and the share of memory macOS counts
  as available. Overview's memory card shows the pressure in place of wired
  memory ([#63](https://github.com/henrikogaard/tidegauge/pull/63)).
- Expanding an app in Apps & processes shows its CPU and memory over the last
  hour, with Open in History for longer ranges. History gains a 1-hour range
  ([#64](https://github.com/henrikogaard/tidegauge/pull/64)).
- An alert for one busy app: when any single app stays above a CPU level
  (80% of one core by default, up to 400%) for five minutes. Off by default;
  turn it on in Settings → Alerts. Each app alerts once until it calms down or
  quits, with its own ten-minute cooldown
  ([#65](https://github.com/henrikogaard/tidegauge/pull/65)).
- Two more opt-in alerts in Settings → Alerts: the startup disk has less than
  10 GB free for a minute, and macOS reports a serious or critical thermal
  state, when it slows the Mac down to cool it
  ([#66](https://github.com/henrikogaard/tidegauge/pull/66)).
- An opt-in memory-pressure alert, when macOS reports warning or critical
  pressure for 20 seconds. While it does, the menu-bar memory reading turns
  orange or red ([#67](https://github.com/henrikogaard/tidegauge/pull/67)).
- An opt-in alert when one app holds more memory than a set amount (4 GB by
  default, 1–64 GB) for five minutes, alongside the one-app CPU alert
  ([#68](https://github.com/henrikogaard/tidegauge/pull/68)).
- Export CSV on the History page saves every recorded sample of what is shown
  (range, source and metric) to a file in Downloads
  ([#69](https://github.com/henrikogaard/tidegauge/pull/69)).
- A keyboard shortcut that opens the menu-bar panel from any app: ⌃⌥⌘T or
  ⌃⌥⌘M in Settings → General → Open the panel. Off by default; it needs no
  Accessibility permission, and a shortcut another app holds is reported
  ([#70](https://github.com/henrikogaard/tidegauge/pull/70)).
- The Battery page charts capacity health per day, kept once a day beyond
  History's 30 days so wear shows over months, with the cycle count's change
  ([#71](https://github.com/henrikogaard/tidegauge/pull/71)).

### Fixed

- Per-app power was about 50 times too low. It now reads macOS's processor
  energy counter and is labelled CPU power, since GPU, display and radio are
  not included ([#61](https://github.com/henrikogaard/tidegauge/pull/61)).
- History recorded only the 60 apps using the most memory, so a busy app with
  little memory had no history. It now also records the busiest apps by CPU,
  disk and network ([#64](https://github.com/henrikogaard/tidegauge/pull/64)).

## [0.3.3](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.3) — 2026-10-16

### Fixed

- On a Mac with a battery, Overview's four bottom cards now make two rows of
  two, so their legends are no longer cut off. The battery card leaves out time
  left and battery flow when there is nothing to show, and shows time to full
  while charging ([#58](https://github.com/henrikogaard/tidegauge/pull/58)).
- Settings are read once and reused until they change, instead of being read
  many times each time the window is drawn
  ([#58](https://github.com/henrikogaard/tidegauge/pull/58)).
- Releases retry stapling when Apple's service drops the connection
  ([#57](https://github.com/henrikogaard/tidegauge/pull/57)).

## [0.3.2](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.2) — 2026-10-16

### Fixed

- The menu-bar panel's footer fits again: Quit was cut off once the update
  button was added. Open Tidegauge keeps its label; updates, Settings and
  Quit are icons ([#56](https://github.com/henrikogaard/tidegauge/pull/56)).

## [0.3.1](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.1) — 2026-10-16

### Added

- Settings → General → Updates shows the installed version and a Check for
  Updates button, and the menu-bar panel has an update button in its footer.
  Since 0.3.0 a left click on the menu-bar item opens the panel, so the update
  check was only in the right-click menu
  ([#55](https://github.com/henrikogaard/tidegauge/pull/55)).

## [0.3.0](https://github.com/henrikogaard/tidegauge/releases/tag/v0.3.0) — 2026-10-16

A redesigned window with sidebar navigation, process control that can replace
Activity Monitor for everyday use, coding agents and AI plan usage, a
menu-bar panel, and an in-app language switch. Entries further down that a
later change in this release replaced say so.

### Added

- Sidebar navigation grouped as Live, Work, Over time and Maintenance, with
  live readings beside each metric and Settings pinned at the bottom
  ([#39](https://github.com/henrikogaard/tidegauge/pull/39)).
- Process control in Apps & processes: confirmed Quit and Force Quit for an
  app, a single process, or every process an app left running, with each
  target re-identified by PID, start time, user and program before a signal is
  sent; system-critical processes and Tidegauge itself are refused
  ([#40](https://github.com/henrikogaard/tidegauge/pull/40), [#44](https://github.com/henrikogaard/tidegauge/pull/44)).
- Agents: running coding-agent sessions (Claude Code, Codex, Gemini CLI,
  Cursor Agent, Copilot CLI) with what each started, their memory and CPU, and
  Quit; AI plan usage and pace read from OpenQuota's saved data on this Mac,
  never from your accounts. Development servers show here as a section
  ([#43](https://github.com/henrikogaard/tidegauge/pull/43), [#52](https://github.com/henrikogaard/tidegauge/pull/52)).
- A menu-bar panel: a left click on the Tidegauge item opens live CPU,
  memory, network and disk with their recent curves, AI plans and the busiest
  apps; a right or Control click keeps the menu ([#52](https://github.com/henrikogaard/tidegauge/pull/52), [#53](https://github.com/henrikogaard/tidegauge/pull/53)).
- Settings → General → Language picks System, English or Norsk and switches
  everything at once, the menu-bar menu included ([#51](https://github.com/henrikogaard/tidegauge/pull/51)).
- Every page has one header: a subtitle, its actions, and an ⓘ button whose
  notes hold what the numbers mean and where they stop ([#50](https://github.com/henrikogaard/tidegauge/pull/50)).

### Changed

- Settings is a grouped list with switches for on/off choices and sliders for
  alert thresholds and output volume; Keep awake is a switch with a duration
  ([#51](https://github.com/henrikogaard/tidegauge/pull/51)).
- Apps & packages, Startup and Recent activity share one list row with actions
  on the selected row and "Show more" instead of pages. Startup groups entries
  by app or maker and hides Apple's own behind a switch; packages with an
  update come first. Apps without an icon and background programs show a quiet
  glyph instead of a letter tile ([#51](https://github.com/henrikogaard/tidegauge/pull/51)).
- Recent activity is the "Last minute" view of Apps & processes. Before & after
  is three steps with a result table. Folder sizes opens with quick picks and
  shows size bars. History reloads every minute while open and says when
  recording began ([#52](https://github.com/henrikogaard/tidegauge/pull/52)).
- CPU temperatures lead with the hottest and average reading; the native
  channel list is folded behind a button. Hardware a Mac lacks (no battery) has
  no page or card, readings that are unavailable say so instead of a dash, and
  the Neural Engine section and its guessed core count are gone ([#49](https://github.com/henrikogaard/tidegauge/pull/49),
  [#50](https://github.com/henrikogaard/tidegauge/pull/50)).

### Fixed

- Busy Claude Code sessions no longer show as idle: their session files record
  the start time in UTC ([#49](https://github.com/henrikogaard/tidegauge/pull/49)).
- Both Claude Code installs group as one app, and reverse-DNS programs get a
  readable name ([#49](https://github.com/henrikogaard/tidegauge/pull/49)).
- On newer macOS, which draws status items in another process, the menu-bar
  panel opens under the click and stays open ([#53](https://github.com/henrikogaard/tidegauge/pull/53)).

- Configurable, opt-in alerts with sustained thresholds and independent cooldowns;
  a persisted system/app History browser with visible gaps; safer development
  project Stop confirmation and outcome reporting; timed keep-awake; dashboard
  favorites, compact layout, and tab order/visibility
  ([#24](https://github.com/henrikogaard/tidegauge/pull/24), merged).
- Supported default-media-output volume/mute controls. These do **not** control
  arbitrary apps ([#24](https://github.com/henrikogaard/tidegauge/pull/24), merged).
- App/helper inspection with observed ancestry, remaining-helper explanations,
  normal app Quit, and confirmed selected-helper SIGTERM, with no automatic
  cleanup or bundle-only ownership assumptions. Force Quit came later in this
  release (see Added)
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
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).

### Fixed

- Application metadata now falls back to per-key extraction when whole-file
  `plutil` JSON conversion fails. Valid Info.plist files containing `<data>` or
  `<date>` values no longer produce a parse warning or drop the application
  from the Apps & Packages inventory
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Battery capacity now reads the nested `BatteryData` dictionary used by newer
  Apple silicon firmware, restoring design/full capacities and health where
  the top level exposes only normalized percentages. A present battery with no
  reported health condition reads "No condition reported" instead of
  "Unavailable"
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Temperature channel discovery is bounded to 512 channels instead of 128.
  Because the controller enumerates keys in ASCII order, the old bound
  silently dropped whole `T…` families on hardware with larger inventories.
  The temperature card now renders at most 24 channels, hottest first with
  unavailable channels last, plus an exact shown/total/unavailable caption
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Remaining user-visible English strings are now localized to Norwegian
  Bokmål: overview card stat labels, memory/power breakdown cards, GPU and
  Neural Engine availability text, Bluetooth device kinds and counts, dev-server
  status sentences, session-uptime durations with singular/plural forms, and
  history range chips
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Detail pages now share one template: a hero card with an uppercase caption,
  a 42 px headline value, a 130 px filled chart, and stat triplets, followed
  by sectioned attribution and supplementary content. Disk gains the hero its
  layout lacked, per-app GPU attribution is stated as unavailable instead of
  implied, and Bluetooth and Sound use the same sectioned list anatomy.
  The Neural Engine tab is folded into the GPU page as a section; saved tab
  settings simply skip it. Captions render at 10 px uppercase with correct
  light-mode contrast, and unavailable measurements use a compact empty-state
  card instead of paragraphs. (The footer and the Neural Engine section were
  replaced later in this release by the sidebar and the page header.)
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Homebrew formula actions now verify the keg's `INSTALL_RECEIPT.json` through
  the same trusted, bounded read used for cask metadata. A missing, malformed,
  symlinked, or untrusted receipt keeps the formula in the inventory but
  disables its actions with an explicit reason; touching the receipt after the
  cached snapshot fails preflight before any manager command runs
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Package post-action matching no longer treats a legitimate change to the
  manager prefix directory's size or mtime as proof of failure. Outcome
  confirmation now matches the same installation — identical source category,
  manager and runtime fingerprints, and prefix canonical path, device, inode,
  ownership, and mode — while pre-action checks still require the exact cached
  identity
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Package update confirmation now compares the manager's typed installed
  version instead of the display label, so an ambiguous or unknown version
  (for example multiple retained kegs) can never be reported as a successful
  update even when the manager command exits cleanly
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Homebrew update checks now read the current `current_version` schema, so
  available updates are no longer reported as uninterpretable JSON. Packages
  with an ambiguous installed version also show their managed root path instead
  of a path containing the "Multiple versions" label, and remain read-only
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Startup override detection now validates the complete `launchctl
  print-disabled` output shape instead of accepting any output containing
  quoted lines, so diagnostic noise can no longer be misread as an empty
  override set ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- The temperature card caption now counts every channel as unavailable when the
  sensor sample is stale instead of reporting only the channels that were
  already empty, and per-app metadata parsing shares one time budget between
  the whole-file conversion and the per-key fallback
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- A failed Homebrew inventory scan now explains that the scan is bounded and
  that Refresh installed retries it without changing installed packages
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Chart rendering is unified on two idioms: the Oversikt cards now draw the
  same filled sparkline as the detail tabs instead of a dense bar strip, and
  the shared area chart keeps headroom with a stroked line so a maxed-out or
  constant series (e.g. a full battery) no longer reads as a solid block. The
  Historikk chart marks empty buckets with a faint baseline tick instead of
  leaving an unexplained gap, and a largely empty selected range says so in a
  localized line
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Controls now share one anatomy across the app: bordered 28 px buttons with
  transparent rest state, segmented radio groups for mutually exclusive
  choices (appearance, menu readout styles, History range and metric,
  baseline windows, package source, and process filters). Steppers for alert
  thresholds and output volume were later replaced by sliders. Arrow-only reorder buttons carry
  context-rich localized labels. The Disk hero reports system-wide disk
  activity instead of free capacity, physical capacity moves into the device
  identity card, and APFS volumes share one container header only when the
  reported metadata is consistent — shared space is never summed per volume.
  Projects gains a count summary and a sectioned list with a shared empty
  state; History uses section headers and shared empty states, and every
  truthfulness note now sits behind the page's ⓘ
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Development-server state is now a typed value (working / idle / barely
  used) instead of a parsed localized string, and project and summary counts
  count distinct server processes rather than listening ports. Menu-bar
  label slots are font-measured once per label instead of assuming a fixed
  width, so localized labels such as MINNE and EFF size correctly
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Keyboard activation now prevents the framework-generated second click, so
  Enter/Space invokes actions once and modified activation keys are ignored
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- The Startup list renders in steps so every entry stays reachable and the
  collected inventory is never truncated (grouped with "Show more" since
  {PR(51)})
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Storage scans stop promptly when cancelled mid-walk, verified after real
  metadata traversal rather than only from a pre-set flag. When a scan's
  coverage is unknown — the folder could not be opened or stat'ed, or the scan
  ended with nothing readable — the status row now states that no size totals
  are available instead of presenting them as 0 B
  ([#35](https://github.com/henrikogaard/tidegauge/pull/35), merged).
- Audio mute actions read fresh device state before applying a change.
- App-instance attribution preserves multiple main processes and requires
  observed ancestry before offering remaining-helper Stop.
- Startup override parsing accepts explicit boolean and textual states; absent
  overrides remain unknown.
- Package discovery and cask target validation handle verified installed
  metadata while rejecting mismatched, stale, unsafe, or ambiguous identities.
- English and Norwegian Bokmål UI text no longer mix. Counts read "1 app" or
  "1 prosess" instead of "1 apps"; Norwegian uses a decimal comma and "50 %"
  with a space; battery time uses "t" for hours; menu-bar labels DISK, NETT and
  BATT follow MINNE and EFF; error codes read "feilkode" instead of "OSStatus";
  and terminology is consistent (Bundet, Utnyttelse, Påloggingsobjekter, App,
  anslått, utgangspunkt). A source-level test fails when UI text bypasses the
  locale helpers.

### Boundaries and remaining acceptance

- Direct third-party startup disable/remove/restore remains unavailable.
- Per-app audio routing and manual fan control are not implemented.
- Baselines are system-level; per-app baseline identity and history migration
  are not implemented. Reported interrupt wakeup rates are implemented, but
  package-idle wakeups and energy attribution are not.
- Physical sensor, battery and disk observations were checked on one Mac;
  independent absolute fan RPM readings and idle/load/sleep-wake coverage are
  still pending. Audio control and actual sign-in launch checks remain open.
- The menu-bar panel was checked on one display; panel placement on a second
  display is covered by tests only. A completed Before & after comparison and
  Agents' development-server section with a live server were not exercised.
  Neural Engine usage is not measured.
- Native invalid-signature rejection was verified on hardware using ad-hoc
  signed QA bundles: a valid update installed first, then a size-preservingly
  tampered archive was rejected at signature verification with no update
  applied. The unreachable-feed error/recovery test passed after 0.2.0. No
  production feed, key, or signed artifact was changed for either test.
- Package actions were exercised only against disposable guest Homebrew
  formula fixtures: a genuine 2.12.2 to 2.12.3 upgrade plus stale-identity,
  cancel and removal checks. Retained old kegs correctly left the upgrade
  result unverified instead of claimed. Native cask and npm mutations remain
  unexercised, and host packages were never changed.

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
