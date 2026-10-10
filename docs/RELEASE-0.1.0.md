# Tidegauge 0.1.0

First Apple Silicon release candidate. The [GitHub release list](https://github.com/henrikogaard/tidegauge/releases)
and linked workflow run are authoritative for draft/publication and signing
status; these notes alone do not establish that a build has passed notarization.

## Included

- A native menu-bar monitor with a compact dashboard, neutral light/dark
  surfaces, distinct metric colors, and English/Norwegian controls.
- CPU, memory, network, and per-app activity, with icon loading off the
  rendering path and summaries beside charts.
- Read-only temperature/fan channels, device-scoped disk inventory and APFS
  relationships, and battery charge/health/charging detail where available.
- Project Stop/Stop All confirmation, PID deduplication, and owner/PID/start-time
  checks immediately before SIGTERM. Sound remains display-only.
- Production packaging with Developer ID signing, required notarization, and
  signed update feeds verified against the app's embedded public key.

## Installation and integrity

Download `Tidegauge-0.1.0.zip` and its matching checksum from the same release.
In their download directory, run:

```sh
shasum -a 256 -c Tidegauge-0.1.0.zip.sha256
```

Extract the ZIP and move `Tidegauge.app` into Applications. Do not bypass
Gatekeeper or strip quarantine attributes if macOS rejects the bundle; report
the failure. An unpublished draft is visible only to authorized repository
users and is not a working public update feed.

The archive, archive checksum, `appcast.xml`, and appcast checksum are the only
release assets. Signing credentials and key backups are never release assets.

## Known limitations and acceptance

- Physical temperature/fan mappings, battery accuracy, internal/external SMART,
  and sleep/reconnect behavior still require real-device validation. See
  [sensors](SENSORS.md) and [disks](DISKS.md). Missing readings are not proof of
  zero usage or healthy hardware.
- Neural utilization is unavailable; GPU detail depends on exposed system
  properties. Per-app watts are estimates, not measured system power.
- Launch-at-login, appearance overrides, and custom alert preferences are not
  fully applied. Stored history has no persisted-history browser yet.
- Project Stop safeguards are implemented, but [issue #6](https://github.com/henrikogaard/tidegauge/issues/6)
  retains follow-up acceptance for project/port presentation, eligibility
  revalidation, and more specific termination outcomes.
- No manual fan writes, power-mode controls, destructive disk actions, or
  per-app audio controls are included.
- A notarized build does not prove update delivery. The signed two-version
  upgrade and invalid-signature rejection checks in [Releasing](RELEASING.md#upgrade-acceptance)
  remain required before claiming end-to-end updater support.

The [screenshot archive](../assets/README.md#screenshot-archive) separates live
virtual-Mac readings from labeled layout fixtures. The [roadmap](https://github.com/henrikogaard/tidegauge/issues/12)
tracks remaining feature work.
