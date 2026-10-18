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
incremental (up to 256 keys / 200 ms per pass), bounded to 8,192 keys and 512
temperature channels. Partial inventory is labeled. The connection is renewed
every minute for wake/recovery; unavailable connections retry every 30 seconds.
Known channel identifiers and unfinished discovery progress survive connection
renewal; values are always re-read, never carried forward as current readings.
Samples older than ten seconds are hidden as stale.

Measured on a MacBook Pro (Mac17,7, M5 Max, macOS 27.0.1): the controller
reports 3,864 keys, of which 359 are `T`-prefixed and 342 pass the temperature
type filter. Reading all 342 channels in one pass costs 44–51 ms against the
two-second sample interval. Keys enumerate in ASCII order, so a low channel
bound does not sample uniformly — the earlier 128-channel bound filled with
uppercase-second-character keys (`TB*`…`TV*`) and silently dropped every
lowercase family (`Ta*` `Tf*` `Tg*` `Tm*` `Tp*` `Ts*` `Tz*`). The
`inventory_limited` flag therefore means genuine truncation, not merely a large
key count.

Channel identifiers are reported exactly as the controller names them and are
never mapped to hardware locations. The dashboard temperature card renders at
most 24 channels, ordered by value descending with unavailable channels last,
and adds an exact shown/total/unavailable caption when the list is bounded or
any channel reports no value (e.g. `Showing 24 of 342 channels · 22
unavailable`, `Viser 24 av 342 kanaler · 22 utilgjengelige`). Fan rows are not
bounded.

## Battery capacity reporting

Pack capacities come from the `AppleSmartBattery` registry entry. On macOS 27 /
Apple silicon (verified on Mac17,7, M5 Max) the top level carries only
normalized `CurrentCapacity`/`MaxCapacity` percentages and `CycleCount`; the
physical mAh values live in a nested `BatteryData` dictionary
(`DesignCapacity`, `NominalChargeCapacity`, `FullChargeCapacity`). Both levels
are read, top level first. Top-level and nested `CurrentCapacity`/`MaxCapacity`
are percentages and are never accepted as mAh — a 101…200,000 guard rejects
them on every candidate path.

Maximum Capacity derives from `NominalChargeCapacity`/`DesignCapacity` (6262 /
6249 ≈ 100.2%, matching the system's report), not `FullChargeCapacity`. Battery
temperature has no `Temperature` key on this hardware and stays unavailable; no
SMC channel is substituted for it. An absent `BatteryHealthCondition` means the
system reports no adverse condition — the UI says "No condition reported" for
a present battery, which is not a failed read and not a health verdict.

## Validation

Run `cargo test sensors::tests` for ABI layout, byte-order/unit conversions,
invalid values, and stale-data checks. The read-only hardware probe is
`cargo test sensors::tests::native_sensor_probe -- --ignored --nocapture`.

For layout only, use `TIDEGAUGE_SENSOR_PREVIEW=1 cargo run` and open CPU.
The explicit sample-data banner is mandatory. This debug-only fixture does not
change collection or history and cannot be enabled in a release build.

### Named sensors

The CPU page leads with temperatures macOS names itself, read from the HID
sensor service (`src/named_sensors.rs`): the chip's die sensors (`PMU tdie`),
the SSD (`NAND`) and the battery (`gas gauge battery`). Calibration channels
(`PMU tcal`) are left out. Run the read-only probe with
`cargo test named_sensors::tests::native_named_sensors_read -- --ignored --nocapture`.

### History, alerts, fans and sleep

- **History** records the chip's hottest named sensor, the SSD and the battery every History write (`temp_chip`, `temp_ssd`, `temp_battery`), only while the sensor reading is fresh. History offers them as Chip °C, SSD °C and Battery °C.
- **Chip temperature alert** (opt-in, Settings → Alerts): the chip's hottest named sensor above the chosen level (95 °C by default, 70–105 °C) for 30 seconds. SMC channels are not used for it, since some of their codes are not temperatures.
- **Fans** show their speed as a share of the controller's top speed (`F0Mx`), as Stats and iStat Menus do, with the lowest speed (`F0Mn`) beside it. On the Mac17,7 both fans report 2,317–7,826 RPM; 2,315 and 2,500 RPM read 30 % and 32 %, matching Stats' 29 % and 32 %.
- **After sleep** the sampler notices that wall time ran on while the monotonic clock did not, then reopens AppleSMC and the HID sensor client instead of trusting connections opened before sleep.

### Checked on hardware

**Mac17,7, Apple M5 Max, macOS 27.0.1 (26A434), 2026-10-16, on AC power, busy
(a virtual Mac running).**

| What | Result |
|---|---|
| SMC inventory | 342 temperature channels, 320 with values; `source: Ready` without extra permissions |
| Fans | `F0Ac` 4,950 RPM, `F1Ac` 5,347 RPM under load; no independent source without root |
| Battery | SMC `TB0T`/`TB1T`/`TB2T` 36.6/36.6/36.0 °C, identical to the HID battery sensors |
| Chip | HID die sensors 54–62 °C (42 sensors); SMC `Tp*` 63–72 °C. Different sensors, both plausible |
| Suspect SMC keys | `Tf*` read up to 99 °C and a `TV*` key reads 1.0: not confirmed as temperatures, so the headline no longer uses the SMC list |

Not yet checked: idle readings, sleep/wake recovery, and fan RPM against an
independent reader (`sudo powermetrics --samplers smc` needs the owner).

Physical CPU/GPU locations, temperature accuracy, fan RPM, wake recovery, and
new-chip compatibility remain unvalidated until compared with an independent
native diagnostic on the actual machine. Record model, chip, macOS version,
source keys, availability, idle/load readings, and sleep/wake behavior. Do not
close hardware acceptance based on a virtual Mac or the layout fixture.
