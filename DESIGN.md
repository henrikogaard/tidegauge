# Tidegauge design reference

Dense, Apple-native data UI. The panel follows the macOS appearance; menu-bar readouts query the native appearance on each refresh. See `src/theme.rs` for implemented tokens.

## Palette

| Token | Light | Dark |
|---|---|---|
| `bg` | `#ffffff` | `#1b1b1e` |
| `card` | `#f5f5f7` | `#202023` |
| `card-hi` | `#e8e8ed` | `#27272b` |
| `ink` | `#1d1d1f` | `#f5f5f7` |
| `ink-dim` | `#6b6b73` | `#86868b` |
| `line` | `#00000014` | `#ffffff1a` |

## Metric accents (light / dark)

| Metric | Light | Dark |
|---|---|---|
| CPU | `#2a78d6` | `#3987e5` |
| Memory, Neural | `#4a3aa7` | `#9085e9` |
| GPU | `#d55181` | `#e87ba4` |
| Disk | `#c98500` | `#eda100` |
| Network | `#199e70` | `#1baf7a` |
| Battery, Power | `#008300` | `#2fa83a` |
| Sound | `#c2379b` | `#d955b3` |
| Bluetooth | `#0891b2` | `#22a8cc` |
| Projects | `#d95926` | `#eb6834` |
| Accent | `#007aff` | `#0a84ff` |
| Warning | `#e0352b` | `#ff5a50` |

The soft metric accent uses 16% alpha. Individual chart fills and selection states may use their own opacity.

## Layout

- **Window**: 1192×780 with a 1050×650 minimum, resizable native window with a transparent titlebar and system window controls. Closing hides the dashboard; a menu-bar readout reopens it.
- **Navigation**: horizontal top tab bar with all 11 tabs, metric icon + label, selected tab = metric accent at 15% opacity with an accent outline. Tab/Shift-Tab moves keyboard focus; Enter/Space opens a tab or metric card.
- **Footer**: live app and process counts.
- **Cards**: 12 px radius, `card` fill, 1 px `line` border.
- **Overview**: six 222 px metric cards in three columns, 18 px radius, 16 px gaps; 34 px headline with a separate unit, three supporting readings, and a 38 px tinted bar-chart tray. Clicking a card opens its detail tab.
- **Hero card**: caption, 42 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: rank, icon (rounded tile or cached app icon), name + `N processes`, usage bar, right-aligned value.
- **Donuts**: convex ring segments (inner radius 0.70), 92 px chart with a center value and subtitle, legend beside the chart with actual amounts and app icons. App breakdowns show the top four entries and Other.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 9 px, body 12–14 px, hero number 42 px bold.
- **Spacing**: 4/8/12/16 grid; overview padding 18–20 px; lists always expanded.

Overview bar charts display at most 48 real samples and leave unfilled history slots blank. CPU/GPU averages and peaks cover the current session; network totals are interface counters since boot. Unavailable hardware readings show a dash. Per-app power remains labeled as estimated.

## Menu bar

- Settings selects enabled metrics and `icon`, `value`, `graph`, or `both` styles. Changes persist in `settings.json` and apply on the next refresh. Sparklines are 34×16 `NSImage` drawings, with a 1.2 px stroke and the last 60 samples. A metric icon fills the graph slot until samples are available.
- The Tidegauge icon opens a native menu with Open Dashboard, Settings, and Quit. Right-click or Control-click any metric to open the same menu. At least one menu item remains visible, including when all metrics and the optional icon are disabled.
- Command-comma opens Settings; Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.
- Click a readout to bring the panel forward. Hiding the panel on a second click is not implemented.
