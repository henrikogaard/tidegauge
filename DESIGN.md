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

- Exactly one native `NSStatusItem` stays registered. Its default content is a 16 px Tidegauge wave icon. Clicking anywhere opens Open Dashboard, Settings, and Quit.
- Readings are opt-in through Settings, with `icon`, `value`, `graph`, and `both` styles. Existing preferences are preserved; Reset to icon only clears optional readings.
- Enabled metrics share one transparent template image. Labels use 9 px system medium, values use 11 px system medium with monospaced digits, and slots have fixed widths. Spacing between readings is 10 px. Native template tint follows the menu bar and selected state.
- Value and graph readings include CPU, MEM, GPU, DSK, NET, BAT, or PWR labels. Sparklines are 24×10 px with a 1 px stroke and at most 60 real samples. Missing samples show a dash.
- The icon remains visible when all readings are off, even if its preference is Hidden. The native item is removed when the monitor exits.
- Every dashboard tab has a labeled, keyboard-accessible Settings button in the footer. Command-comma also opens Settings; global Command-Q quits from the dashboard. A per-user file lock prevents duplicate monitors. Settings provides keyboard-accessible choices and reports persistence errors.
