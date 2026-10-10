# Local history

The History tab reads the existing `history.db` in Tidegauge's macOS application-support directory. No migration deletes or rebuilds recorded rows. New indexes improve metric/app lookups; normal retention still removes samples older than 30 days, checked hourly while Tidegauge runs. Nothing is uploaded.

## Ranges and rendering

- Choose 12 hours, 24 hours, 7 days, or 30 days. Refresh captures a new end time; changing a range/source/metric also refreshes.
- SQLite timestamps are Unix seconds. The dashboard labels the range in local time using `YYYY-MM-DD HH:MM`.
- Queries include the start and exclude the end: `[start, end)`.
- Each range is divided into 180 equally spaced buckets. A bar is the arithmetic mean of available samples within that bucket, not a peak or a time-weighted estimate.
- Empty buckets stay blank. A measured zero is a baseline mark. Missing rows are never padded with zero, and gaps are never joined by a line.
- The sampler normally records about every ten seconds. Scheduling, sleep, unavailable measurements, and collection failures can leave gaps. Short spikes can be smoothed by averaging; zoom into a shorter range to inspect them.
- Reads use a separate read-only connection and a consistent transaction on a background worker. SQLite WAL allows the sampler to keep writing. Late results from superseded selections cannot replace the current chart.

## Units and provenance

| Trend | Unit / interpretation |
|---|---|
| System CPU | Percent of overall CPU capacity |
| System memory | Used bytes / total physical memory, percent; not measured memory pressure |
| System disk and network | Bytes/second, separate read/write or receive/send |
| GPU | Reported utilization percent when available |
| Battery | Reported charge percent when available |
| System power | Measured watts only when available; currently unavailable on this collector |
| App CPU | Percent of one core, possibly over 100 across cores |
| App memory | Resident bytes in the recorded app group |
| App disk/network | Combined read+write or receive+send bytes/second |

Older GPU/battery rows can contain zero placeholders that cannot be distinguished from genuine zero measurements. Those **legacy zeroes are omitted**. Positive legacy samples remain visible. New optional measurements use separate keys (`gpu_measured`, `batt_measured`, `power_measured`), so genuine measured zeroes remain usable. When both old and new keys exist at a timestamp, the measured key wins.

Legacy system power estimates are not presented as measured watts. Per-app energy counters have not been validated as measured watts and are not selectable in History. Existing rows remain in the database until normal retention removes them.

Only up to 60 app groups are stored per sample, using the existing recorded **name** rather than a stable bundle identifier. Identical names can collide. An absent app row does not establish that the app was stopped or using zero resources. Historical zero rates cannot establish that an older collector successfully measured every source. These are limits of the stored data, not an invitation to infer missing values.

## Checks

`cargo test history::` covers optional-measurement writes. `cargo test history_browser::` uses isolated databases to cover boundaries in every range, means, gaps, real zeroes, legacy data, safe app filtering, dense downsampling, read-only reopening, and preservation of source rows. Do not seed demonstration values into a user's live history database.
