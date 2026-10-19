# Contributing to Tidegauge

## Development setup

Follow the [README](README.md) to install the prerequisites and run the app on macOS. Other platforms are not supported by the current native bindings.

Before editing, read the relevant module, check the working tree, and use a feature branch. Keep behavioral fixes separate from documentation-only changes where practical.

## Checks

Run the checks relevant to the change:

```sh
cargo check
cargo build
cargo test
```

`cargo check` checks Rust types without linking the binary. `cargo build` also verifies native linking. The default tests cover native structure layouts, counter conversion, CSV parsing, snapshot access, working directories, retained app rates, settings persistence, and the instance lock. Hardware and process integration checks run separately on macOS:

```sh
cargo test native_ -- --ignored
```

These checks require `nettop`, `lsof`, and `python3`. They read disk counters, transfer data over a disposable loopback connection, and start a busy local test server. The test server is stopped when its check completes. Run these checks serially when investigating counter baselines (`--test-threads=1`).

The UI is English and Norwegian Bokmål, and the two never mix. It follows
the first preferred macOS language unless Settings → General → Language picks
one; the switch applies at once, menu-bar menu included. Visible text goes
through `locale::text(english, norwegian)`, counts through `locale::count` or
`locale::plural`, and numbers through `locale::number`, `locale::percent` or the
`engine::fmt_*` helpers (Norwegian uses a decimal comma and "50 %"). The
`locale_guard` tests parse `src/` and fail on untranslated UI literals,
inconsistent translations and missing singulars. To run the whole suite as
Norwegian, set `TIDEGAUGE_TEST_LANG=nb`.

To measure what Tidegauge costs while its dashboard is closed, start it with
`TIDEGAUGE_HIDDEN=1`, and point `HOME` at a temporary directory so it does not
touch your own data:

```sh
HOME=$(mktemp -d) TIDEGAUGE_HIDDEN=1 target/release/tidegauge
```

Measure the packaged app or a copy of the binary inside an `.app` bundle: the
icon handling differs outside one.

For a documentation-only change, verify examples against the source, check local Markdown links, and inspect the diff. Source comment edits do not require new behavioral tests.

For a collector or FFI change, compare native structure sizes, field offsets, constants, and return-value conventions against the installed macOS SDK. Exercise the affected collector with a bounded, read-only probe. Prefer a focused regression test when the logic can run without native hardware.

For interrupt-wakeup reporting or storage-scan changes, run the focused checks:

```sh
cargo test --locked resource_activity::
cargo test --locked rusage_wakeup_layout
cargo test --locked storage::
```

Cloud-placeholder handling needs a real provider. Put a few files you own in a
dedicated folder inside a File Provider domain (for example under
`~/Library/CloudStorage`), make some of them online-only, and run:

```sh
TIDEGAUGE_PROVIDER_FIXTURE=/path/to/that/folder \
  cargo test --locked native_provider_placeholders_stay_offloaded -- --ignored --nocapture
```

The check fails if the folder has no offloaded entry or if any file's dataless
flag, block count or size changes during the scan. It never lists a folder that
is itself a placeholder. A synthetic flag or a virtual
Mac without a provider domain does not satisfy it.

Output volume and mute need a person listening at a physical Mac. This check
changes the default output's volume and mute, speaks six numbered lines, and
restores the starting state even if it fails:

```sh
cargo test --locked native_output_controls_are_audible_and_restored -- --ignored --nocapture
```

Line four must be silent. Record what was heard; the assertions only prove the
reported state, not the sound.

To check that a device switch makes the old target stale and that an output
without volume or mute support refuses changes, name a second output. The
check switches the default output to it and restores the original, without
changing any volume:

```sh
TIDEGAUGE_SWITCH_OUTPUT="Device name" \
  cargo test --locked native_output_switch_rejects_stale_target -- --ignored --nocapture
```

Reported interrupt wakeups are a reported kernel counter, not energy use. The rusage layout was validated with a native C probe on Mac17,7 only; package-idle wakeups remain unvalidated and unavailable.

## Manual app verification

Run the build from the same commit being reviewed and record the macOS version and hardware. Check only the behavior affected by the change:

- Menu-bar changes: capture the actual menu bar. Check the icon-only default and optional labeled readings inside a single native item. Values must remain aligned when their digits change. Change a metric style in Settings, confirm persistence after restart, reset to icon only, and disable all readings with the icon preference hidden to check the visible fallback. Click the item to open the native menu, choose Open Dashboard, then Quit and verify the process exits. Launch twice and confirm only one monitor runs.
- Panel changes: inspect the affected tabs, charts, scrolling, and appearance. Check dashboard Settings access by mouse and keyboard.
- Collector changes: compare values with the relevant macOS tool and test unavailable-data handling.
- History changes: use an isolated database and verify writes, queries, and retention.
- Project changes: use a disposable server that you own. Confirm that desktop listeners, root/home directories, and processes without a project marker are excluded. Verify that cancelling Stop or Stop All sends no signal, and that confirmation only targets the original process identity. Never test against unrelated processes.
- Tools changes: use an isolated History/profile and disposable folder fixtures. Verify baseline coverage, source preservation and restart persistence; storage cancellation and incomplete results; source filters and package action cancellation. Startup and package tests must not change the development machine's real registrations or installed packages. Mutating adapters need mocked commands or a disposable prefix. A disabled third-party startup control is intentional, not permission to bypass macOS protections.

Keep screenshots and raw diagnostics outside commits unless they are deliberately selected documentation assets. Do not record private process arguments, Bluetooth addresses, or unrelated personal data in a review report.

## Review evidence

Update [CHANGELOG.md](CHANGELOG.md)'s **Unreleased** section for user-visible
changes. Link the implementation PR and distinguish merged work from work still
in review. Do not describe an unmerged feature as shipped or check off physical
acceptance based on fixtures. At publication, move only the included entries to
the actual version/date and retain limitations that still apply. Release notes
must match the verified artifact, not merely the current branch.

For battery layout checks on a Mac without a battery, run a debug build with
`TIDEGAUGE_BATTERY_PREVIEW=charging cargo run` (or `discharging`). The Battery
tab displays an explicit sample-data banner. This UI-only fixture does not
alter collected metrics or persisted history and is disabled in release builds.
It validates layout only, not native battery readings.

Record the commit, commands, results, and any hardware-dependent checks that could not run. A build pass, a rendered check, and verification on physical hardware are separate results.

For notification branding checks, run `cargo build`, then package the debug binary with
`TIDEGAUGE_BINARY="$PWD/target/debug/tidegauge" packaging/package-app.sh preview`.
Install the bundle in Applications and quit any existing Tidegauge process. Run its
executable with `TIDEGAUGE_NOTIFICATION_PREVIEW=1` to send a clearly labeled test
alert through the normal notification path. This switch is debug-only and does
not simulate high CPU usage or add sample metrics to history. Check the app name,
six-bar icon, foreground/background delivery, and Allow/Don't Allow permission
behavior. Notifications require an app bundle; `cargo run` deliberately skips
them rather than attributing alerts to a script host. macOS notification settings
and Focus can suppress banners.

Recovered notes in `docs/recovered/` and the historical test files document a previous environment. They can explain implementation choices, but their paths, build results, and packaging claims are not current setup instructions.
