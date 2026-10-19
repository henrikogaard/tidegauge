import Foundation
import os

let log = Logger(subsystem: "no.ogard.tidegauge.widgets", category: "data")

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
        let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: tidegaugeGroupID)
        log.info("container: \(url?.path ?? "nil", privacy: .public)")
        return url
    }

    /// Tells Tidegauge a widget exists, so it starts writing readings.
    static func touchMarker() {
        guard let marker = container?.appendingPathComponent(markerFile) else { return }
        if FileManager.default.fileExists(atPath: marker.path) {
            do {
                try FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: marker.path)
                log.info("marker touched")
            } catch {
                log.error("marker touch failed: \(error.localizedDescription, privacy: .public)")
            }
        } else {
            let made = FileManager.default.createFile(atPath: marker.path, contents: Data())
            log.info("marker created: \(made, privacy: .public)")
        }
    }

    static func load() -> Snapshot? {
        guard let file = container?.appendingPathComponent(snapshotFile) else { return nil }
        let data: Data
        do {
            data = try Data(contentsOf: file)
        } catch {
            log.info("no snapshot: \(error.localizedDescription, privacy: .public)")
            return nil
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            let snapshot = try decoder.decode(Snapshot.self, from: data)
            guard snapshot.schema == 1 else {
                log.error("unknown schema \(snapshot.schema, privacy: .public)")
                return nil
            }
            let age = Date().timeIntervalSince1970 - snapshot.sampleAt
            log.info("snapshot decoded: \(snapshot.metrics.count, privacy: .public) metrics, sample age \(age, privacy: .public) s")
            return snapshot
        } catch {
            log.error("snapshot decode failed: \(String(describing: error), privacy: .public)")
            return nil
        }
    }
}
