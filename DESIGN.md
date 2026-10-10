# Tidegauge design reference

Dense, Apple-native data UI. The panel follows the macOS appearance; menu-bar readouts query the native appearance on each refresh. See `src/theme.rs` for implemented tokens.

## Palette

| Token | Light | Dark |
|---|---|---|
| `bg` | `#ffffff` | `#131315` |
| `card` | `#f5f5f7` | `#1f1f22` |
| `card-hi` | `#e8e8ed` | `#27272b` |
| `ink` | `#1d1d1f` | `#f5f5f7` |
| `ink-dim` | `#86868b` | `#86868b` |
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

- **Window**: 860×640, `WindowKind::PopUp` (NSPanel — lives above full-screen spaces like a menu-bar popover).
- **Navigation**: horizontal top tab bar with all 11 tabs, metric icon + label, selected tab = metric accent at 15% opacity.
- **Footer**: live app and process counts.
- **Cards**: 12 px radius, `card` fill, 1 px `line` border.
- **Hero card**: caption, 30 px number, sub-caption, filled area chart (130 px), footer stat triplets.
- **App rows**: rank, icon (rounded tile or cached app icon), name + `N processes`, usage bar, right-aligned value.
- **Donuts**: wedge-polygon ring (inner radius 0.62), center value, legend below.
- **Fonts**: `.AppleSystemUIFont`; captions uppercase 9 px, body 12–14 px, hero number 42 px bold.
- **Spacing**: 4/8/12/16 grid; density over whitespace — lists always expanded.

## Menu bar

- Items come from `settings.json` → `menu_items` and currently display a metric value plus an available sparkline (34×16 `NSImage`, 1.2 px stroke, last 60 samples).
- The intended `value`, `graph`, and `both` style options are not yet applied.
- Click a readout to bring the panel forward. Hiding the panel on a second click is not implemented.
