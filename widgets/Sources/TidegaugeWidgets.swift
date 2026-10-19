import SwiftUI
import WidgetKit

struct MetricWidget: Widget {
    let kind = "no.ogard.tidegauge.metric"

    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: kind, intent: SelectMetricIntent.self, provider: MetricProvider()) { entry in
            // The gallery may ask for a placeholder with no intent; show the
            // sample readings instead of redacted blocks.
            SmallView(entry: entry).unredacted()
        }
        .configurationDisplayName("Tidegauge Reading")
        .description("One reading with a ring: CPU, memory, GPU, battery, network or disk.")
        .supportedFamilies([.systemSmall])
    }
}

struct OverviewWidget: Widget {
    let kind = "no.ogard.tidegauge.overview"

    var body: some WidgetConfiguration {
        StaticConfiguration(kind: kind, provider: OverviewProvider()) { entry in
            OverviewView(entry: entry)
        }
        .configurationDisplayName("Tidegauge Overview")
        .description("Your Mac's readings with their recent history.")
        .supportedFamilies([.systemMedium, .systemLarge])
    }
}

@main
struct TidegaugeWidgets: WidgetBundle {
    var body: some Widget {
        MetricWidget()
        OverviewWidget()
    }
}
