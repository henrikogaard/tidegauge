# Local history

The History tab reads the existing `history.db` in Tidegauge's macOS application-support directory. No migration deletes or rebuilds recorded rows. New indexes improve metric/app lookups; normal retention still removes samples older than 30 days, checked hourly while Tidegauge runs. Nothing is uploaded.

## Ranges and rendering

- Choose 1 hour, 12 hours, 24 hours, 7 days, or 30 days. Refresh captures a new end time; changing a range/source/metric also refreshes.
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

Each sample stores the 60 app groups using the most memory, plus the 20 busiest by CPU and the 10 busiest by disk and by network when those are not already among them, so an app that is busy but small still has a history. Rows use the recorded **name** rather than a stable bundle identifier. Identical names can collide. An absent app row does not establish that the app was stopped or using zero resources. Historical zero rates cannot establish that an older collector successfully measured every source. These are limits of the stored data, not an invitation to infer missing values.

## Export

Export CSV under the chart saves what History shows (the chosen range, source and metric) to a file in Downloads and shows it in Finder. The file holds every recorded sample, not the chart's interval averages, under the same rules: legacy placeholders and per-app power are left out, and gaps are simply missing rows. Columns: `local_time` (`YYYY-MM-DD HH:MM:SS`), `unix_time`, `source` (System or the app's recorded name), `metric` (a stable English id such as `cpu` or `disk_read`), `unit` and `value`. A source name that starts like a spreadsheet formula (`=`, `+`, `-`, `@`) is written with a leading apostrophe so it stays text, and a second export never overwrites the first: it is saved as "… 2.csv".

## In Apps & processes

Expanding an app in Apps & processes shows its CPU and memory over the last hour, read the same way as History (180 interval averages, gaps left blank) and reloaded every minute while the app stays expanded. The figure above each chart is the highest interval average, not a true peak. Open in History shows the same app there, at 1 hour.

## Battery health

On a Mac with a battery, History also keeps one row per local day in `battery_daily`: the day's latest capacity health (full-charge over design capacity), cycle count and both capacities. These rows are outside the 30-day pruning, because a battery wears over months, and are one small row a day. The Battery page charts the last year per day. Days Tidegauge did not run stay gaps, and the chart's band starts at a round floor below the lowest day, labelled as Scale. Health is only recorded when macOS reports it.

## Damaged databases

SQLite corruption is reported separately from temporary read failures. Refreshing cannot repair a damaged database; the browser never repairs or replaces it automatically. Quit Tidegauge and preserve `history.db` together with any accompanying `history.db-wal` and `history.db-shm` files before attempting recovery. Work on a copy, not the only remaining database. Do not delete history merely to dismiss an error.

## Checks

`cargo test history::` covers optional-measurement writes. `cargo test history_browser::` uses isolated databases to cover boundaries in every range, means, gaps, real zeroes, legacy data, safe app filtering, dense downsampling, read-only reopening, and preservation of source rows. Do not seed demonstration values into a user's live history database.
