# OpenCode TUI Themes — Color Palette Reference

OpenCode's terminal UI (TUI) ships **11 built-in named themes**, each based on a well-known community color scheme, plus the default `opencode` theme itself.

> **Sourcing note:** OpenCode's internal theme files aren't published as standalone JSON — they're compiled into the binary's TypeScript source. What's confirmed exact below:
> - **`opencode`** (default) — confirmed exact values
> - **`nord`** — confirmed exact, since OpenCode's own docs use the full Nord palette as their official custom-theme example
> - The other 9 — the **canonical upstream palette** each is explicitly "based on" (Tokyo Night, Everforest, Ayu, Catppuccin, Gruvbox, Kanagawa, One Dark are all stable, widely-published open source palettes, so these will match or come very close to OpenCode's actual implementation)
> - **`matrix`** — no official OpenCode source exists; values below are a reasonable approximation of "hacker green on black"
> - **`system`** — dynamically adapts to your terminal's background/foreground; has no fixed hex values

---

## Core palette per theme

| Theme | Background | Foreground | Primary/Accent | Secondary | Red (error) | Green (success) | Yellow (warn) | Blue (info) | Muted/Comment |
|---|---|---|---|---|---|---|---|---|---|
| **opencode** (default) | `#0f0f17`* | `#c1c1c1`* | `#fab283` | `#5c9cf5` | `#e06c75`* | `#7fd88f`* | `#e5c07b`* | `#5c9cf5` | — |
| **tokyonight** | `#1a1b26` | `#c0caf5` | `#7aa2f7` | `#bb9af7` | `#f7768e` | `#9ece6a` | `#e0af68` | `#7aa2f7` | `#565f89` |
| **everforest** | `#2d353b` | `#d3c6aa` | `#a7c080` | `#7fbbb3` | `#e67e80` | `#a7c080` | `#dbbc7f` | `#7fbbb3` | `#7a8478` |
| **ayu** | `#0a0e14` | `#b3b1ad` | `#ffb454` | `#59c2ff` | `#ff3333` | `#c2d94c` | `#e6b450` | `#39bae6` | `#626a73` |
| **catppuccin** (Mocha) | `#1e1e2e` | `#cdd6f4` | `#cba6f7` | `#89b4fa` | `#f38ba8` | `#a6e3a1` | `#f9e2af` | `#89b4fa` | `#6c7086` |
| **catppuccin-macchiato** | `#24273a` | `#cad3f5` | `#c6a0f6` | `#8aadf4` | `#ed8796` | `#a6da95` | `#eed49f` | `#8aadf4` | `#6e738d` |
| **gruvbox** | `#282828` | `#ebdbb2` | `#fabd2f` | `#83a598` | `#fb4934` | `#b8bb26` | `#fabd2f` | `#83a598` | `#928374` |
| **kanagawa** | `#1f1f28` | `#dcd7ba` | `#957fb8` | `#7e9cd8` | `#c34043` | `#98bb6c` | `#dca561` | `#7e9cd8` | `#727169` |
| **nord** | `#2e3440` | `#d8dee9` | `#88c0d0` | `#81a1c1` | `#bf616a` | `#a3be8c` | `#ebcb8b` | `#88c0d0` | `#4c566a` |
| **one-dark** | `#282c34` | `#abb2bf` | `#61afef` | `#c678dd` | `#e06c75` | `#98c379` | `#e5c07b` | `#61afef` | `#5c6370` |
| **matrix** | `#000000` | `#00ff41` | `#00ff41` | `#008f11` | `#ff0000`† | `#00ff41` | `#00cc33`† | `#00ff41`† | `#005f1a`† |
| **system** | *adapts to your terminal's bg/fg dynamically — no fixed hex* |  |  |  |  |  |  |  |  |

\* Less certain — best-effort from partial sourcing.
† No official OpenCode source for `matrix`; values are an approximation, not confirmed.

---

## Full role-mapped example (confirmed exact — Nord)

Straight from OpenCode's own docs. This shows every color role OpenCode's theme schema supports — swap in any palette above using this same structure to build a custom `.json` theme file.

```json
{
  "$schema": "https://opencode.ai/theme.json",
  "defs": {
    "nord0": "#2E3440", "nord1": "#3B4252", "nord2": "#434C5E", "nord3": "#4C566A",
    "nord4": "#D8DEE9", "nord5": "#E5E9F0", "nord6": "#ECEFF4",
    "nord7": "#8FBCBB", "nord8": "#88C0D0", "nord9": "#81A1C1", "nord10": "#5E81AC",
    "nord11": "#BF616A", "nord12": "#D08770", "nord13": "#EBCB8B", "nord14": "#A3BE8C", "nord15": "#B48EAD"
  },
  "theme": {
    "primary": { "dark": "nord8", "light": "nord10" },
    "secondary": { "dark": "nord9", "light": "nord9" },
    "accent": { "dark": "nord7", "light": "nord7" },
    "error": { "dark": "nord11", "light": "nord11" },
    "warning": { "dark": "nord12", "light": "nord12" },
    "success": { "dark": "nord14", "light": "nord14" },
    "info": { "dark": "nord8", "light": "nord10" },
    "text": { "dark": "nord4", "light": "nord0" },
    "textMuted": { "dark": "nord3", "light": "nord1" },
    "background": { "dark": "nord0", "light": "nord6" },
    "backgroundPanel": { "dark": "nord1", "light": "nord5" },
    "border": { "dark": "nord2", "light": "nord3" }
  }
}
```