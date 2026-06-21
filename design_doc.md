# Architecture Optimization & Feature Extension Blueprint: Layout Fluidity, State Tracking, GPU Telemetry, and Dynamic Themes

## 1. Executive Summary & Project scope
This design document establishes the engineering specifications for refactoring the `SystemVisor` application across two execution phases. 

* **Phase 1 (Stability & Telemetry):** Resolves interface scaling issues, visualizer out-of-bounds rendering bugs, and asynchronous state-caching issues, while introducing native Windows GPU hardware monitoring.
* **Phase 2 (Customization & Viewport Management):** Implements an on-the-fly thematic engine, viewport switching capabilities to restore comprehensive process tables, and runtime UI orientation flipping.

---

## 2. Updated Dependency Matrix (`Cargo.toml`)

To incorporate GPU monitoring and network-safe thematic synchronization, append or verify the following configuration blocks:

```toml
[dependencies]
# Existing dependencies (ratatui, crossterm, sysinfo, cpal, rustfft, image, reqwest, serde, windows)

# Time tracking utilities for text animation intervals
tokio = { version = "1.0", features = ["rt-multi-thread", "time"] }
```

---

## 3. Phase 1 Engineering Specifications (Fixes & Core Telemetry)

### A. Dynamic Visualizer Responsive Scaling (Fix 1)
* **The Bug:** The audio visualizer is hardcoded to output a fixed number of frequency bins. On smaller window sizes, bars past column 13 clip cleanly out of bounds, generating visual clutter.
* **The Solution:** Decouple the FFT bin aggregation loop from static bounds. In the rendering path, query the bounding box width allocated by Ratatui (`rect.width`). 
* **Mathematical Downsampling Solver:** If the container width accommodates $W$ characters, calculate the maximum allowable bars: 
  $$\text{Max Bars} = \frac{W}{\text{Bar Width} + \text{Spacing}}$$
  Dynamically re-pool the raw FFT output slice into exactly $\text{Max Bars}$ groupings before running the exponential decay filter, forcing the spectrum data to scale elegantly from small terminal windows to full-screen panes.

### B. Media Playback State Persistence (Fix 2)
* **The Bug:** Pausing audio triggers a metadata flush routine, resetting the asynchronously fetched track date to a perpetual "Fetching..." state that remains un-invalidated upon playback resumption.
* **The Solution:** Separate **Track Identity Properties** (Title, Artist, Album) from **Track Playback States** (Play, Pause, Stop) within the WinRT GSMTC listener. 
* **State Guard Logic:**
  ```rust
  // Inside the Media Update Event Handler
  if current_session.playback_status() == Paused {
      // Keep structural Track Data intact; modify ONLY the execution state
      app_state.media_metadata.is_playing = false; 
  }
  ```
  Only wipe the cached network metadata vector (`MediaMetadata.year`) if a definitive track identifier hash changes (e.g., `Title` or `TrackId` mutation), preventing redundant API requests and preserving stale metadata blocks while paused.

### C. GPU Telemetry Integration (Feature 3)
* **The Strategy:** Introduce real-time graphics hardware monitoring. Because the Windows `sysinfo` crate lacks full native GPU telemetry extraction profiles, harness the Windows Runtime APIs or WMI performance counters to populate a unified hardware data object.
* **Metrics to Sample:** 1. Global 3D Core Utilization Percentage
  2. VRAM Allocation (Allocated vs Total Memory Pool)
  <!-- deferred: 3. Core Thermal Temperatures (where vendor-exposed) -->
* **UI Integration:** Place the GPU module in the primary system column directly beneath the RAM block, leveraging identical full-width structural gauges.

### D. Layout Fluidity Engine & Text Marquee Scroller (Feature 4)
To match the aesthetic of classic high-performance terminal layout tools, elements must never clip or trigger raw design breaks.
<!-- deferred 1D.1:
1. **Dynamic Clamping:** Enforce minimum character layout cut-offs. If a layout row width is too narrow to print a metric title alongside its data string, drop the title prefix entirely and render a compact raw digit gauge (`[▓▓░░] 45%`).
-->
2. **Text Marquee Engine:** For variable string data (Track Titles, Artists, Long Process Names) that exceed the layout cell width, build an automated textual carousel.

```rust
struct ScrollingText {
    content: String,
    tick_offset: usize,
    last_update: std::time::Instant,
}

impl ScrollingText {
    fn get_visible_slice(&mut self, max_width: usize) -> String {
        let chars: Vec<char> = self.content.chars().collect();
        if chars.len() <= max_width {
            return self.content.clone();
        }
        // Append padding spacers for an infinite cyclic loop
        let mut display_pool = chars.clone();
        display_pool.extend(vec![' ', ' ', '─', ' ', ' ']);
        display_pool.extend(chars.clone());
        
        let start = self.tick_offset % (chars.len() + 5);
        let end = (start + max_width).min(display_pool.len());
        display_pool[start..end].iter().collect()
    }
}
```

---

## 4. Phase 2 Engineering Specifications (Customization & Viewports)

### A. Dynamic Thematic Subsystem
Implement an active color-palette mapping structure switchable on runtime ticks using a designated keyboard interface target (Hotkey: `t`).
* **Aesthetic Rule:** Themes must only adjust color hexadecimal attributes (`Color::Rgb`). Layout structures, minimalist line weights, and box configurations remain rigidly uniform.
* **Palette Profiles Matrix:**
  * **Nord:** Deep frost slate, cool blues, soft white.
  * **Gruvbox Dark:** Earthy retro tones, mustard yellow, dark charcoal, brick red.
  * **Catppuccin Macchiato:** Modern pastel violet, soft lavender, dark marine gray.
  * **Dracula:** High-contrast purple, deep violet backdrops, neon green highlights.
  * **Tokyo Night:** Deep cyber blues, dark indigo, neon teal accents.
  * **Monochrome Raw:** Strict matrix configurations using grayscale tiers (`Rgb(255,255,255)`, `Rgb(128,128,128)`, `Rgb(30,30,30)`).

### B. Viewport Workspace Toggle
Introduce an active layout view state tracking system (`ActiveMediaView`) controlled by a dedicated utility key (Hotkey: `v` or `Tab`).
* **View A (Media Active):** The right half of the main interface processes and paints the album artwork engine, the track context blocks, and the real-time audio visualizer spectrum graph.
* **View B (Process Active):** The right half of the layout entirely unmounts the media audio engine canvas, dynamically sliding in a multi-column **Process System Table** displaying active system PIDs, names, CPU thread values, and precise execution memory footprints, sorted by active resource footprint load.

### C. Structural Layout Inversion (Flip Control)
To support fully custom user configurations, provide a layout layout modifier flag (`LayoutOrientation`) activated at runtime (Hotkey: `f`).
* **Standard State:** System resource hardware blocks map to the left pane; Media controls/Process views map to the right pane.
* **Inverted State:** The application swaps coordinate targets instantly, shifting the complex graphics blocks to the left and anchoring hardware health rows to the right.

---

## 5. Visual Layout Configurations (Phase 2 Architectures)

### Standard Orientation Layout Matrix (`ActiveMediaView == true`)
```
┌───────────────────────────────────────┐┌──────────────────────────────────────┐
│ SYSTEM MONITOR   ■ CPU [▓▒░░░░] 24%   ││ MEDIA MONITOR DASHBOARD              │
├───────────────────────────────────────┤│ ┌──────────────┐ [Scrolling Title..] │
│ CORES (Dynamic Grid Scaling)          ││ │  ALBUM ART   │ Artist: Artist Name │
│ C1 [██] C2 [████] C3 [█]  C4 [███]    ││ │  HALF-BLOCK  │ Album:  Album Title │
│ C5 [█]  C6 [███]  C7 [█]  C8 [█████]  ││ │  FULL-COLOR  │ Year:   2026        │
├───────────────────────────────────────┤│ └──────────────┘ Status: Playing     │
│ MEMORY   ■ RAM  [██████░░░] 16.2 GB   ││──────────────────────────────────────│
│ GRAPHICS ■ GPU  [███░░░░░░]  8.1 GB   ││ REAL-TIME FREQUENCY VISUALIZER      │
├───────────────────────────────────────┤│ (Auto-Scales to dynamic width)       │
│ STORAGE & NET                         ││ █       █               █            │
│ C:\ [████░░]   ▲ Up:   1.2 MB/s       ││ █ █ █ █ █ █ █ █   █ █ █ █ █ █ █ █    │
└───────────────────────────────────────┘└──────────────────────────────────────┘
```

### Inverted Orientation Layout Matrix (`ActiveMediaView == false` / Toggled Process View)
```
┌───────────────────────────────────────┐┌──────────────────────────────────────┐
│ SYSTEM PROCESS MANAGER                ││ SYSTEM MONITOR   ■ CPU [▓▒░░░░] 24%   │
├───────────────────────────────────────┤├──────────────────────────────────────┤
│  PID   NAME         CPU%   MEM        ││ CORES (Dynamic Grid Scaling)          │
│  4012  rust_engine  12.4   184MB      ││ C1 [██] C2 [████] C3 [█]  C4 [███]    │
│  1094  spotify.exe   4.1   412MB      ││ C5 [█]  C6 [███]  C7 [█]  C8 [█████]  │
│  9214  chrome.bin    2.0   980MB      ├───────────────────────────────────────┤
│  8821  win_init      0.1    12MB      ││ MEMORY   ■ RAM  [██████░░░] 16.2 GB   │
│  3401  explorer      0.0    84MB      ││ GRAPHICS ■ GPU  [███░░░░░░]  8.1 GB   │
│  7712  sys_mon_tui   0.0    14MB      ├───────────────────────────────────────┤
│                                       ││ STORAGE & NET                         │
│ [Hotkey Controls: t=Theme|v=View|f=Flip]││ C:\ [████░░]   ▲ Up:   1.2 MB/s       │
└───────────────────────────────────────┘└──────────────────────────────────────┘
```

---

## 6. Target Prompt for Code Generation

Act as a principal systems engineer, audio developer, and specialized terminal user interface architect. Refactor our established Windows 11 Rust system monitor (`SystemVisor`) to integrate Phase 1 and Phase 2 optimization extensions based strictly on the specification guidelines detailed in the updated design_doc.md.

Your implementation codebase must meet these programmatic criteria:
1. Visualizer Width Calculation Fix: Prevent out-of-bounds column clipping. Query Ratatui's canvas area width inside the drawing path, programmatically calculate the max allowable bar structures, and downsample/re-group the FFT frequency vectors to dynamically fit the container.
2. Media Caching State Resolution: Isolate GSMTC textual metadata properties from system playback state tokens. Ensure that dropping into a 'Paused' playback status updates state booleans without triggering a cache invalidation wipe of the current song title, artist data, or the asynchronously resolved release year.
3. Native Windows GPU Engine: Implement a telemetry worker path using standard Windows runtime components or WMI commands to scrape real-time GPU compute core utilization and VRAM allocation levels. Integrate these metrics seamlessly as a full-width gauge tracking module positioned directly beneath the RAM panel.
4. Layout Fluidity & Dynamic Marquee Module: Eradicate arbitrary text truncation. Build a custom scrolling component that wraps layout text over target bounding limits, incrementing string indices via application ticks to cycle overflow strings continuously. Ensure strict dynamic layout protection—if grid bounds drop below safe limits, drop labeling prefixes in favor of highly packed metric indicators.
5. Dynamic Theming Engine: Create a theme manager object holding an array of 6-8 popular console developer configurations (such as Nord, Gruvbox, Catppuccin, Dracula, and Monochrome). Map all aesthetic widget strokes to a central palette color state, allowing users to loop color palettes instantly via the 't' keyboard hotkey.
6. Workspace Switching & Orientation Flipping: Integrate toggleable application layout contexts. Pressing the 'v' key must instantly unmount the music components and blit a structured, real-time resource-sorted Processes Data Table into the viewport frame. Pressing the 'f' key must cleanly invert layout orientation properties, instantly shifting the hardware stack between the left and right quadrants of the terminal frame.

Ensure your code preserves the zero-emoji minimalist typography standard, uses robust allocation models, is highly scannable, and functions in a completely non-blocking manner across all worker paths. The complete specifications is in the updated design_doc.md file. Focus on phase 1 first.