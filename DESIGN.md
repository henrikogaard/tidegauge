# Tidegauge design reference

Dense, Apple-native data UI. The panel follows the macOS appearance; menu-bar readouts query the native appearance on each refresh. See `src/theme.rs` for implemented tokens.

## Palette

Graphite Night uses a charcoal canvas, mineral surfaces, silver text, and neutral controls. Graphite Day uses the matching light palette. All app colors come from `src/theme.rs`; the panel follows macOS appearance.

| Token | Graphite Day | Graphite Night |
|---|---|---|
| `bg` | `#f5f5f7` | `#161618` |
| `card` | `#ffffff` | `#1e1e20` |
| `card-hi` | `#ededf0` | `#2c2c2e` |
| `ink` | `#1d1d1f` | `#e5e5ea` |
| `ink-dim` | `#6e6e73` | `#8e8e93` |
| `line` | `#d2d2d7` | `#38383a` |
| `accent` | `#1d1d1f` | `#f5f5f7` |
| Success | `#248a3d` | `#30d158` |
| Error or critical state | `#d70015` | `#ff453a` |

## Metric tones

Charts and metric headings use neutral tones. Labels, units, and values identify each metric; color is reserved for status signals. App breakdowns use progressively softer tones with a labeled legend.

| Metric | Graphite Day | Graphite Night |
|---|---|---|
| CPU | `#3a3a3c` | `#d1d1d6` |
| Memory, Neural | `#6e6e73` | `#aeaeb2` |
| GPU, Sound | `#48484a` | `#c7c7cc` |
| Disk, Projects | `#6e6e73` | `#8e8e93` |
| Network, Bluetooth | `#3a3a3c` | `#d1d1d6` |
| Battery, Power | `#48484a` | `#b8b8bd` |

The soft metric tone uses 16% alpha. Chart fills use lower opacity. Selected navigation uses `card-hi`, `line`, and `accent`; keyboard focus uses a high-contrast neutral border.

## Layout

- **Window**: 1192×780 with a 1050×650 minimum, resizable native window with a transparent titlebar and system window controls. Closing hides the dashboard; a menu-bar readout reopens it.
- **Navigation**: horizontal top tab bar with all 11 tabs, metric icon + label, selected tab = neutral raised surface with a quiet outline. Tab/Shift-Tab moves keyboard focus; Enter/Space opens a tab or metric card.
- **Footer**: generated graphite activity-bar logo, Settings access, and live app and process counts.
- **Cards**: 12 px radius, `card` fill, 1 px `line` border.
- **Overview**: six 222 px metric cards in three columns, 12 px radius, 16 px gaps; 34 px headline with a separate unit, three supporting readings, and a 38 px tinted bar-chart tray. Clicking a card opens its detail tab.
- **Hero card**: caption, 42 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: rank, icon (rounded tile or cached app icon), name + `N processes`, usage bar, right-aligned value.
- **Donuts**: convex ring segments (inner radius 0.70), 92 px chart with a center value and subtitle, legend beside the chart with actual amounts and app icons. App breakdowns show the top four entries and Other.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 9 px, body 12–14 px, hero number 42 px bold.
- **Spacing**: 4/8/12/16 grid; overview padding 18–20 px; lists always expanded.

Overview bar charts display at most 48 real samples and leave unfilled history slots blank. CPU/GPU averages and peaks cover the current session; network totals are interface counters since boot. Unavailable hardware readings show a dash. Per-app power remains labeled as estimated.

## Menu bar

- Exactly one native `NSStatusItem` stays registered. Its default content is a 16 px Tidegauge wave icon. Clicking anywhere opens Open Dashboard, Settings, and Quit.
- Readings are opt-in through Settings, with `icon`, `value`, `graph`, and `both` styles. Existing preferences are preserved; Reset to icon only clears optional readings.
- Enabled metrics share one transparent template image. Labels use 9 px system medium, values use 11 px system medium with monospaced digits, and slots have fixed widths. Spacing between readings is 10 px. Native template tint follows the menu bar and selected state.
- Value and graph readings include CPU, MEM, GPU, DSK, NET, BAT, or PWR labels. Sparklines are 24×10 px with a 1 px stroke and at most 60 real samples. Missing samples show a dash.
- The icon remains visible when all readings are off, even if its preference is Hidden. The native item is removed when the monitor exits.
- Every dashboard tab has a labeled, keyboard-accessible Settings button in the footer. Command-comma also opens Settings; global Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.

## Artwork and documentation preview

The generated activity-bar app icon uses a graphite tile with silver and off-white bars. See [artwork](assets/README.md) for the source PNG, macOS icon package, and generation prompt. UI chrome uses flat colors without gradients.

The README dashboard screenshot is a native render with generic sample data. It demonstrates layout and styling rather than measured hardware activity.
