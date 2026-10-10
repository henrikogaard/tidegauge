# Read-only thermal telemetry

The CPU view shows temperature channels and fan RPM from the native AppleSMC
controller, plus the public `NSProcessInfo.thermalState` value. Thermal state is
not a temperature reading and is not inferred from missing sensors.

The collector does not request administrator privileges and has no write
operation. It only issues read-value, key-index, and key-info commands (5, 8,
and 9) through selector 2 of the AppleSMC user client. It does not alter fan
targets, fan modes, power limits, or thermal protection.

## Availability and units

AppleSMC's key protocol is not a stable public macOS API. Exposure varies by
chip, OS, permissions, virtualisation, and power state. Missing service,
denied access, read failures, unsupported encodings, and stale samples remain
unavailable; they must not be interpreted as safe temperatures or stopped fans.

- Temperature channels retain native `T…` identifiers. No unverified CPU/GPU
  location mapping or derived package average is applied.
- `sp78` is signed big-endian fixed-point Celsius; `flt ` is little-endian
  IEEE-754. Zero/offline values, non-finite data, and values outside −40…150 °C
  are omitted. Channels may represent different locations or differential
  measurements; raw identifiers must be validated against the target hardware.
- Actual fan speed comes only from `F…Ac`, using `fpe2` or `flt ` values in RPM.
  A successfully read zero RPM is valid; a failed read is unavailable.
- `FNum = 0` means the controller reports no fans, not that collection failed.

The dedicated worker owns the connection and metadata cache. Sampling occurs
every two seconds without holding the UI or main-sampler lock. Discovery is
incremental (up to 256 keys / 200 ms per pass), bounded to 8,192 keys and 128
temperature channels. Partial inventory is labeled. The connection is renewed
every minute for wake/recovery; unavailable connections retry every 30 seconds.
Samples older than ten seconds are hidden as stale.

## Validation

Run `cargo test sensors::tests` for ABI layout, byte-order/unit conversions,
invalid values, and stale-data checks. The read-only hardware probe is
`cargo test sensors::tests::native_sensor_probe -- --ignored --nocapture`.

For layout only, use `TIDEGAUGE_SENSOR_PREVIEW=1 cargo run` and open CPU.
The explicit sample-data banner is mandatory. This debug-only fixture does not
change collection or history and cannot be enabled in a release build.

Physical CPU/GPU locations, temperature accuracy, fan RPM, wake recovery, and
new-chip compatibility remain unvalidated until compared with an independent
native diagnostic on the actual machine. Record model, chip, macOS version,
source keys, availability, idle/load readings, and sleep/wake behavior. Do not
close hardware acceptance based on a virtual Mac or the layout fixture.
