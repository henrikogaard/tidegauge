# Tidegauge artwork

## Logo

- `logo.svg`: the vector recreation of the graphite six-bar app icon. This is the editable source; the app embeds it as its native application icon. The dashboard footer has no icon tile.
- `logo.png`: the 1254 × 1254 pixel raster export, with transparency outside the rounded tile.
- `menu-bar-icon.svg`: the same six-bar silhouette simplified into a 16-point monochrome template, without a background tile. AppKit supplies the menu bar's native tint independently of the dashboard theme. The app loads this vector once rather than decoding it every sampling cycle.
- `tidegauge.icns`: the same artwork packaged at the standard macOS icon sizes, including 16, 32, 128, 256, and 512 pixels and their double-resolution variants. The app-bundle packaging workflow includes it.

Regenerate the PNG and ICNS after editing the full logo on macOS:

```sh
swift scripts/export-icons.swift
```

The menu-bar SVG is optically adjusted for small sizes. Keep its six rounded bars and relative heights consistent with the full logo.

## Screenshot archive

Native-window captures of Tidegauge 0.3.3, the signed and notarized release
build, taken in a virtual Mac on 2026-10-16 with the English interface. Each
window ran for about 90 seconds first, so charts hold real samples and app
icons have loaded. Captures exclude personal paths, credentials, device
serial numbers and other windows. On `dashboard.png` a remote-session cursor
badge in the empty title strip was painted over with the strip's own
background; nothing else is edited.

| Capture | Appearance | What it shows |
|---|---|---|
| [Overview](dashboard.png) | Dark | Sidebar, live cards, busiest apps and memory breakdowns |
| [CPU light](screenshots/cpu-light.png) | Light | CPU usage, its chart and the apps using it |
| [CPU dark](screenshots/cpu-dark.png) | Dark | The same page in dark appearance, at another moment |
| [Apps & processes](screenshots/apps-dark.png) | Dark | Every app with its processes, CPU and memory |
| [Settings](screenshots/settings-light.png) | Light | The grouped settings list with switches and sliders |

All readings belong to the virtual Mac: it has no battery and no readable GPU
usage, and its app list includes the tools that run it. None of these captures
establishes physical sensor, battery, SMART or update behaviour.
