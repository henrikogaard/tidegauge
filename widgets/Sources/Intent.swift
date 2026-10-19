import AppIntents
import WidgetKit

/// The reading a small widget shows.
enum MetricChoice: String, AppEnum {
    case cpu, memory, gpu, battery, network, disk

    static var typeDisplayRepresentation = TypeDisplayRepresentation(name: "Metric")
    static var caseDisplayRepresentations: [MetricChoice: DisplayRepresentation] = [
        .cpu: "CPU",
        .memory: "Memory",
        .gpu: "GPU",
        .battery: "Battery",
        .network: "Network",
        .disk: "Disk",
    ]
}

struct SelectMetricIntent: WidgetConfigurationIntent {
    static var title: LocalizedStringResource = "Metric"
    static var description = IntentDescription("Choose the reading this widget shows.")

    @Parameter(title: "Metric", default: .cpu)
    var metric: MetricChoice
}
