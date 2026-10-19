import SwiftUI
import WidgetKit

extension Color {
    init(hex: String) {
        let digits = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        let value = UInt32(digits, radix: 16) ?? 0x888888
        self.init(
            red: Double((value >> 16) & 0xff) / 255,
            green: Double((value >> 8) & 0xff) / 255,
            blue: Double(value & 0xff) / 255
        )
    }
}

private func title(_ id: String) -> LocalizedStringKey {
    switch id {
    case "cpu": return "CPU"
    case "memory": return "Memory"
    case "gpu": return "GPU"
    case "battery": return "Battery"
    case "network": return "Network"
    default: return "Disk"
    }
}

private func rate(_ bytesPerSecond: Double) -> String {
    ByteCountFormatter.string(fromByteCount: Int64(max(0, bytesPerSecond)), countStyle: .decimal) + "/s"
}

private func percent(_ value: Double) -> String {
    (value / 100).formatted(.percent.precision(.fractionLength(0)))
}

/// The big figure of a reading: "34 %" or "1.2 MB/s".
private func headline(_ r: Reading) -> String {
    r.isRate ? rate(r.value) : percent(r.value)
}

private struct Palette {
    let scheme: ColorScheme
    func accent(_ r: Reading) -> Color { Color(hex: scheme == .dark ? r.dark : r.light) }
}

/// The card fill of the app (src/theme.rs): white, or near black.
private struct CardBackground: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        Color(hex: scheme == .dark ? "#1e1e20" : "#ffffff")
    }
}

private struct Ring<Content: View>: View {
    let fraction: Double
    let color: Color
    var lineWidth: CGFloat = 9
    @ViewBuilder var content: Content

    var body: some View {
        ZStack {
            Circle().stroke(color.opacity(0.18), lineWidth: lineWidth)
            Circle()
                .trim(from: 0, to: max(0.004, min(1, fraction)))
                .stroke(color, style: StrokeStyle(lineWidth: lineWidth, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .widgetAccentable()
            content
        }
        .padding(lineWidth / 2)
    }
}

private struct Sparkline: View {
    let values: [Double]
    let maximum: Double
    let color: Color

    var body: some View {
        GeometryReader { geo in
            let points = values.count > 1 ? values : [0, 0]
            let top = max(maximum, 0.0001)
            let step = geo.size.width / CGFloat(points.count - 1)
            let y: (Double) -> CGFloat = { geo.size.height * (1 - CGFloat(min(max($0 / top, 0), 1))) }
            ZStack {
                Path { p in
                    p.move(to: CGPoint(x: 0, y: geo.size.height))
                    for (i, v) in points.enumerated() { p.addLine(to: CGPoint(x: CGFloat(i) * step, y: y(v))) }
                    p.addLine(to: CGPoint(x: geo.size.width, y: geo.size.height))
                    p.closeSubpath()
                }
                .fill(color.opacity(0.16))
                Path { p in
                    for (i, v) in points.enumerated() {
                        let point = CGPoint(x: CGFloat(i) * step, y: y(v))
                        if i == 0 { p.move(to: point) } else { p.addLine(to: point) }
                    }
                }
                .stroke(color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
                .widgetAccentable()
            }
        }
    }
}

private struct NotRunning: View {
    var body: some View {
        VStack(spacing: 6) {
            Image(systemName: "gauge.with.dots.needle.33percent")
                .font(.title2)
                .foregroundStyle(.secondary)
            Text("Open Tidegauge to start")
                .font(.footnote)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A line under a figure: network upload, disk activity, battery state.
private func caption(_ r: Reading) -> Text? {
    guard r.available else { return nil }
    switch r.id {
    case "network":
        return Text("Up \(rate(r.detail ?? 0))")
    case "disk":
        return Text("Activity \(rate(r.detail ?? 0))")
    case "battery":
        switch r.state {
        case "charging": return Text("Charging")
        case "on_battery": return Text("On battery")
        default: return Text("Plugged in")
        }
    default:
        return nil
    }
}

// MARK: Small

struct SmallView: View {
    let entry: TideEntry
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Group {
            if let snapshot = entry.snapshot, let r = snapshot.reading(entry.metric.rawValue) {
                let color = Palette(scheme: scheme).accent(r)
                VStack(spacing: 6) {
                    HStack {
                        Text(title(r.id)).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                        Spacer()
                    }
                    Ring(fraction: r.available ? r.fraction : 0, color: color) {
                        if r.available {
                            Text(headline(r))
                                .font(.system(r.isRate ? .callout : .title2, design: .rounded).weight(.semibold))
                                .minimumScaleFactor(0.6)
                                .lineLimit(1)
                                .padding(.horizontal, 14)
                        } else {
                            Text("Not available").font(.caption2).foregroundStyle(.secondary)
                        }
                    }
                    if let line = caption(r) {
                        line.font(.caption2).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.7)
                    }
                }
            } else {
                NotRunning()
            }
        }
        .containerBackground(for: .widget) { CardBackground() }
    }
}

// MARK: Medium and large

private struct Cell: View {
    let reading: Reading
    var showsRing = false
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        let color = Palette(scheme: scheme).accent(reading)
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline) {
                Text(title(reading.id)).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Spacer(minLength: 4)
                if reading.available {
                    Text(headline(reading))
                        .font(.system(.subheadline, design: .rounded).weight(.semibold))
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                }
            }
            if reading.available {
                if showsRing {
                    Ring(fraction: reading.fraction, color: color, lineWidth: 7) {
                        Text(percent(reading.fraction * 100))
                            .font(.system(.caption, design: .rounded).weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxHeight: 62)
                    .frame(maxWidth: .infinity)
                }
                Sparkline(values: reading.series, maximum: reading.seriesMax, color: color)
                    .frame(minHeight: 22)
                if let line = caption(reading) {
                    line.font(.caption2).foregroundStyle(.secondary).lineLimit(1).minimumScaleFactor(0.7)
                }
            } else {
                Spacer(minLength: 0)
                Text("Not available").font(.caption2).foregroundStyle(.secondary)
                Spacer(minLength: 0)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

private struct UpdatedLabel: View {
    let sampled: Date
    var body: some View {
        Text("Updated \(sampled, style: .time)")
            .font(.caption2)
            .foregroundStyle(.tertiary)
    }
}

struct MediumView: View {
    let entry: TideEntry

    /// CPU, memory and network, then the first of battery, GPU and disk that
    /// this Mac has.
    private func four(_ snapshot: Snapshot) -> [Reading] {
        var ids = ["cpu", "memory", "network"]
        let last = ["battery", "gpu", "disk"].first { snapshot.reading($0)?.available == true } ?? "disk"
        ids.append(last)
        return ids.compactMap { snapshot.reading($0) }
    }

    var body: some View {
        Group {
            if let snapshot = entry.snapshot {
                let readings = four(snapshot)
                VStack(spacing: 6) {
                    Grid(horizontalSpacing: 16, verticalSpacing: 10) {
                        ForEach(0..<(readings.count + 1) / 2, id: \.self) { row in
                            GridRow {
                                Cell(reading: readings[row * 2])
                                if row * 2 + 1 < readings.count { Cell(reading: readings[row * 2 + 1]) }
                            }
                        }
                    }
                    HStack { Spacer(); UpdatedLabel(sampled: snapshot.sampled) }
                }
            } else {
                NotRunning()
            }
        }
        .containerBackground(for: .widget) { CardBackground() }
    }
}

struct LargeView: View {
    let entry: TideEntry
    private let order = ["cpu", "memory", "gpu", "network", "disk", "battery"]

    var body: some View {
        Group {
            if let snapshot = entry.snapshot {
                let readings = order.compactMap { snapshot.reading($0) }
                VStack(spacing: 8) {
                    Grid(horizontalSpacing: 18, verticalSpacing: 14) {
                        ForEach(0..<(readings.count + 1) / 2, id: \.self) { row in
                            GridRow {
                                Cell(reading: readings[row * 2], showsRing: true)
                                if row * 2 + 1 < readings.count {
                                    Cell(reading: readings[row * 2 + 1], showsRing: true)
                                }
                            }
                        }
                    }
                    HStack { Spacer(); UpdatedLabel(sampled: snapshot.sampled) }
                }
            } else {
                NotRunning()
            }
        }
        .containerBackground(for: .widget) { CardBackground() }
    }
}

struct OverviewView: View {
    let entry: TideEntry
    @Environment(\.widgetFamily) private var family

    var body: some View {
        switch family {
        case .systemLarge, .systemExtraLarge: LargeView(entry: entry)
        default: MediumView(entry: entry)
        }
    }
}
