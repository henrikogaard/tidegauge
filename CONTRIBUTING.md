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

For a documentation-only change, verify examples against the source, check local Markdown links, and inspect the diff. Source comment edits do not require new behavioral tests.

For a collector or FFI change, compare native structure sizes, field offsets, constants, and return-value conventions against the installed macOS SDK. Exercise the affected collector with a bounded, read-only probe. Prefer a focused regression test when the logic can run without native hardware.

## Manual app verification

Run the build from the same commit being reviewed and record the macOS version and hardware. Check only the behavior affected by the change:

- Menu-bar changes: verify readouts update, clicking brings the panel forward, and right-click opens the native menu. Change a metric style in Settings, confirm persistence after restart, disable all readings, and check that the Tidegauge icon remains available. Quit through the menu and verify the process exits; launch twice and confirm only one monitor runs.
- Panel changes: inspect the affected tabs, charts, scrolling, and appearance.
- Collector changes: compare values with the relevant macOS tool and test unavailable-data handling.
- History changes: use an isolated database and verify writes, queries, and retention.
- Project changes: use a disposable server that you own. Stop actions send SIGTERM immediately; do not test them against unrelated processes.

Keep screenshots and raw diagnostics outside commits unless they are deliberately selected documentation assets. Do not record private process arguments, Bluetooth addresses, or unrelated personal data in a review report.

## Review evidence

Record the commit, commands, results, and any hardware-dependent checks that could not run. A build pass, a rendered check, and verification on physical hardware are separate results.

Recovered notes in `docs/recovered/` and the historical test files document a previous environment. They can explain implementation choices, but their paths, build results, and packaging claims are not current setup instructions.
