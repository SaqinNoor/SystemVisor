## 0.3.0 — Themes, Marquee, Theming Engine & Process View

### New Features
- **Theme picker** — Press `t` to open a popup with 9 color schemes (tokyonight, everforest, ayu, catppuccin, gruvbox, kanagawa, nord, one-dark, matrix). Navigate with arrows, confirm with Enter.
- **Dynamic themes** — All UI colors are driven by the selected palette. Includes accent1/accent2/highlight/neutral/border roles.
- **Viewport toggle** — Press `v` to switch the right column between the media dashboard and a sortable process table (PID, Name, CPU%, Memory).
- **Text marquee** — Long track titles and artist names scroll horizontally. Header and footer also scroll when content overflows. Scrolling rate is capped at 280ms per tick.
- **Sortable process table** — Sort by PID / Name / CPU% / Memory via `Tab`/`s`, `r` for reverse, `1-4` for direct column selection.
- **Process table navigation** — `j`/`k` (scroll), `u`/`d` (page), `Home`/`End`, cycling wrap.

### Fixes
- Audio visualizer no longer clips on narrow terminals (dynamic bar count).
- Media metadata year is preserved across play/pause toggles.
- CPU gauge and sparkline are now properly boxed with borders.
- Footer text trimmed/scrolled instead of silently cut off.
- Right column height now matches left column with a spacer block.
- Header marquee no longer resets on uptime change.
- Terminal restores cleanly on crash (panic hook + drop guard).

### Tech
- Refactored right column into dispatchable views (`RightView` enum).
- Centralized color system via `ColorTheme` struct and `THEMES` array.
- Version bumped 0.1.3 → 0.2.0 → 0.3.0.
