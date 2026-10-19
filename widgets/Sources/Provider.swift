import AppIntents
import WidgetKit

struct TideEntry: TimelineEntry {
    let date: Date
    /// nil: show "Open Tidegauge to start".
    let snapshot: Snapshot?
    let metric: MetricChoice
}

/// How long a reading is shown after it was sampled, even when macOS delays
/// the next timeline.
private let maxDisplayAge: TimeInterval = 30 * 60
/// WidgetKit decides when a timeline is really rebuilt; this is the request.
private let refreshRequest: TimeInterval = 5 * 60
private let retryRequest: TimeInterval = 60

enum Timelines {
    static func make(metric: MetricChoice, now: Date = Date()) -> Timeline<TideEntry> {
        SnapshotStore.touchMarker()
        // Stale when this timeline is built means Tidegauge is not running;
        // the entries after it only age the readings it did write.
        guard let snapshot = SnapshotStore.load(), !snapshot.isStale(at: now) else {
            let entry = TideEntry(date: now, snapshot: nil, metric: metric)
            return Timeline(entries: [entry], policy: .after(now.addingTimeInterval(retryRequest)))
        }
        let expiry = snapshot.sampled.addingTimeInterval(maxDisplayAge)
        var entries = (0..<20)
            .map { now.addingTimeInterval(Double($0) * 60) }
            .filter { $0 < expiry }
            .map { TideEntry(date: $0, snapshot: snapshot, metric: metric) }
        // A last entry at the boundary, so a delayed reload never leaves old
        // readings looking live.
        entries.append(TideEntry(date: max(expiry, now), snapshot: nil, metric: metric))
        return Timeline(entries: entries, policy: .after(now.addingTimeInterval(refreshRequest)))
    }

    /// Plausible readings for the widget gallery.
    static var sample: Snapshot {
        func reading(_ id: String, _ value: Double, unit: String = "percent", detail: Double? = nil,
                     state: String? = nil, max: Double = 100, light: String, dark: String) -> Reading {
            let series = (0..<30).map { i in
                max * 0.15 + max * 0.5 * (0.5 + 0.5 * sin(Double(i) / 4 + value))
            }
            return Reading(id: id, available: true, value: value, unit: unit, fraction: min(1, value / max),
                           detail: detail, state: state, series: series, seriesMax: max, light: light, dark: dark)
        }
        return Snapshot(schema: 1, writtenAt: 0, sampleAt: Date().timeIntervalSince1970, staleAfterSecs: 120, metrics: [
            reading("cpu", 34, light: "#246fc7", dark: "#5a9cf0"),
            reading("memory", 61, light: "#6550bb", dark: "#a397f4"),
            reading("gpu", 18, light: "#b94378", dark: "#f18db5"),
            reading("battery", 82, state: "on_battery", light: "#287c35", dark: "#63ca6e"),
            reading("network", 1_200_000, unit: "bytes_per_second", detail: 240_000, max: 3_000_000,
                    light: "#147d59", dark: "#41c79a"),
            reading("disk", 47, detail: 5_000_000, light: "#9b6900", dark: "#e8ad39"),
        ])
    }
}

struct MetricProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> TideEntry {
        TideEntry(date: Date(), snapshot: Timelines.sample, metric: .cpu)
    }

    func snapshot(for configuration: SelectMetricIntent, in context: Context) async -> TideEntry {
        if context.isPreview {
            return TideEntry(date: Date(), snapshot: Timelines.sample, metric: configuration.metric)
        }
        return Timelines.make(metric: configuration.metric).entries[0]
    }

    func timeline(for configuration: SelectMetricIntent, in context: Context) async -> Timeline<TideEntry> {
        Timelines.make(metric: configuration.metric)
    }
}

struct OverviewProvider: TimelineProvider {
    func placeholder(in context: Context) -> TideEntry {
        TideEntry(date: Date(), snapshot: Timelines.sample, metric: .cpu)
    }

    func getSnapshot(in context: Context, completion: @escaping (TideEntry) -> Void) {
        if context.isPreview {
            completion(TideEntry(date: Date(), snapshot: Timelines.sample, metric: .cpu))
        } else {
            completion(Timelines.make(metric: .cpu).entries[0])
        }
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<TideEntry>) -> Void) {
        completion(Timelines.make(metric: .cpu))
    }
}
