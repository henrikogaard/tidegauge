// Linked into the tidegauge binary by build.rs. WidgetKit takes a reload
// request only from the main executable of the app that holds the widget,
// so a helper program cannot do this.
import WidgetKit

@_cdecl("tidegauge_reload_widgets")
public func tidegaugeReloadWidgets() {
    WidgetCenter.shared.reloadAllTimelines()
}
