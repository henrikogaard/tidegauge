# Device-scoped storage overview

The Disk view selects a native device rather than combining root-filesystem
capacity with aggregate I/O. Device names, internal/external state, protocol,
media type, and capacity come from macOS `diskutil` property lists. APFS
containers are linked to their physical stores; their volumes are not counted
as independent physical drives. Virtual disk images are omitted.

## Space accounting

APFS capacity and free space belong to the shared container. A volume's
`CapacityInUse` is shown as its footprint, not used space subtracted from a
private volume allowance. Shared free space is repeated for context, not
summed. Mounted system snapshots resolve to their source volume without
duplicating it. Multi-store containers retain all physical-store identifiers.
For other mounted filesystems, the native volume size and free-space fields
are used. Missing values stay unavailable.

## Activity and health

- Device I/O comes from an `IOBlockStorageDriver` with exactly one direct,
  whole `IOMedia` BSD device. Unmapped drivers are omitted rather than assigned
  to the wrong drive. Counters cover that device's current kernel lifetime,
  not its hardware lifetime. Rates require two fresh samples; changed registry
  IDs, reduced counters, or gaps of ten seconds reset rate/history baselines.
- Per-app activity remains system-wide and is explicitly labeled as such.
- Basic SMART status comes from macOS. Unknown/not-supported is never healthy.
- An already-installed optional local SMART reader can provide a read-only
  JSON health log. No executable is downloaded or installed by the app, no
  privilege prompt is raised, and no settings, tests, or writes are requested.
  The reader is instructed to skip disks in standby; hardware/bridge behavior
  still needs external-drive validation. Without access, fields stay unavailable.
- Extended fields include current temperature, power-on hours/cycles, and
  supported NVMe endurance and lifetime I/O counters. NVMe data units are
  512,000 bytes. Large string-encoded counters are parsed exactly; overflow is
  unavailable. Vendor-specific ATA lifetime attributes are not guessed.
- Endurance used is a wear indicator that can exceed 100%, not a health score.
  A successful SMART check is not a guarantee against failure.

Inventory and I/O use independent workers (30 s / 2 s) and publish only short
snapshot swaps. Subprocesses have five-second deadlines and 2 MiB output caps;
inventory work is bounded and reports partial results. Metadata older than
90 seconds and I/O older than ten seconds are not shown as current.
No mount, unmount, eject, erase, self-test, or SMART-setting operation exists.

## Validation

`cargo test disks::tests` covers shared APFS space and snapshot relationships,
SMART units/failures/overflow, and rate reset behavior.
`cargo test disks::tests::native_disk_inventory_probe -- --ignored --nocapture`
prints the host inventory and mapped counters for comparison with `diskutil`
and `ioreg`. Do not infer physical SSD or USB-bridge support from a virtual Mac.

For layout only, `TIDEGAUGE_DISK_PREVIEW=1 cargo run` provides two clearly
labeled sample devices. This debug-only fixture is separate from collection
and history and cannot be enabled in release builds.

Physical internal/external SMART, hot-unplug/reconnect, sleeping rotating disks,
APFS multi-store devices, and permission failures require real-device testing.
