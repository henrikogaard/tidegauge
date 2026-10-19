import Foundation

/// The app group Tidegauge and this extension share. Keep in step with
/// `GROUP_ID` in src/widget_export.rs and the entitlements files.
let tidegaugeGroupID = "45AD7E7G5G.no.ogard.tidegauge"
private let snapshotFile = "widget-snapshot.json"
private let markerFile = "widget-active"

/// One metric as Tidegauge writes it (schema 1, see src/widget_export.rs).
struct Reading: Codable, Hashable {
    var id: String
    var available: Bool
    var value: Double
    var unit: String
    var fraction: Double
    var detail: Double?
    var state: String?
    var series: [Double]
    var seriesMax: Double
    var light: String
    var dark: String

    var isRate: Bool { unit == "bytes_per_second" }
}

struct Snapshot: Codable, Hashable {
    var schema: Int
    var writtenAt: Double
    var sampleAt: Double
    var staleAfterSecs: Double
    var metrics: [Reading]

    func reading(_ id: String) -> Reading? {
        metrics.first { $0.id == id }
    }

    var sampled: Date { Date(timeIntervalSince1970: sampleAt) }

    /// Too old to be the running app's: the same rule as `is_stale` in Rust.
    func isStale(at date: Date) -> Bool {
        date.timeIntervalSince1970 - sampleAt > staleAfterSecs
    }
}

enum SnapshotStore {
    static var container: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: tidegaugeGroupID)
    }

    /// Tells Tidegauge a widget exists, so it starts writing readings.
    static func touchMarker() {
        guard let marker = container?.appendingPathComponent(markerFile) else { return }
        if FileManager.default.fileExists(atPath: marker.path) {
            try? FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: marker.path)
        } else {
            FileManager.default.createFile(atPath: marker.path, contents: Data())
        }
    }

    static func load() -> Snapshot? {
        guard let file = container?.appendingPathComponent(snapshotFile),
              let data = try? Data(contentsOf: file) else { return nil }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        guard let snapshot = try? decoder.decode(Snapshot.self, from: data), snapshot.schema == 1 else { return nil }
        return snapshot
    }
}
