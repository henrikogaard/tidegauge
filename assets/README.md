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

These selected native-window captures from the October 2026 layout and
localization pass are stored in Git, not dependent on external attachment
hosting. They show the implementation consolidated in [PR #19](https://github.com/henrikogaard/tidegauge/pull/19).
The traffic lights are unobstructed. Captures exclude personal paths,
credentials, device serial numbers, and unrelated desktop windows.

| Capture | Appearance / language | Data and purpose |
|---|---|---|
| [Dashboard](dashboard.png) | Dark / English | Live virtual-Mac overview; GPU utilization and battery unavailable |
| [CPU light](screenshots/cpu-light.png) | Light / English | Live virtual-Mac CPU summary, chart, and ranked processes |
| [CPU dark](screenshots/cpu-dark.png) | Dark / English | Same layout in dark appearance; captured at a different sampling time |
| [Battery](screenshots/battery-fixture-dark-nb.png) | Dark / Norwegian | Explicitly labeled development fixture, not battery measurements |
| [Disk](screenshots/disk-fixture-min-dark-nb.png) | Dark / Norwegian | Explicitly labeled development fixture at minimum window size |
| [Settings light](screenshots/settings-en-light-min.png) | Light / English | Minimum-size Settings, near the top of the scrollable form |
| [Settings dark](screenshots/settings-nb-dark-min.png) | Dark / Norwegian | Minimum-size Settings, scrolled to the lower controls |

CPU and overview values belong to the development VM and are not generic
mock readings. The battery/disk fixture banners must remain visible when
reusing those images. None of these captures establishes physical sensor,
battery, SMART, or signed-update compatibility. Settings captures include the
native window shadow; no remote-control overlay covers the window controls.

## Generation prompt

The logo was created with the built-in image generator. The activity-bar version is the selected artwork.

> Use case: logo-brand. Asset type: finished macOS app icon and repository logo for Tidegauge, a system activity monitor. Edit target: the attached activity-bar icon. Preserve the six rounded vertical bars, varied heights, shared baseline, centered composition, tile shape, padding and transparent exterior. Replace the entire blue/cyan palette with a quiet Graphite Night palette: tile charcoal #161618 with subtle #1e1e20 depth, bars in soft off-white #f5f5f7, silver #d1d1d6 and mid-gray #8e8e93. Crisp and refined, readable at 16 and 32 pixels. Very restrained material shading, no neon glow. No blue or cyan anywhere. No text, letters, numbers, axes, grid, wave, gauge, branding from other products, mockups, perspective, outer shadow, surrounding scene, or watermark. Only the finished icon with transparent canvas outside the tile.

## License

These repository assets use the project's [MIT License](../LICENSE).

Copyright (c) 2026 Henrik Øgård.
