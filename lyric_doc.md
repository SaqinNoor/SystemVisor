# Feature Implementation Specification: Synchronized Live Lyrics Subsystem

## 1. Feature Objective & Scope
The objective is to expand the `SystemVisor` media dashboard with a real-time, time-synced **Live Lyrics Panel**. This panel will ingest synchronized lyric files, track the active media player's exact playback timeline, and display dynamically highlighted text that scrolls automatically to keep the currently sung line pinned to the center of the viewport.

### Core Deliverables
* **Asynchronous Lyrics Fetcher:** Query open-source public lyric APIs (modeled after modern terminal clients like `lyricstify` and `slyrics` utilizing the public `lrclib` repository) to retrieve time-coded lyrics non-blockingly upon track change events.
* **LRC Format Parser:** Build a fast structural parser to convert raw `.lrc` timestamped strings into a sorted, match-ready timeline vector.
* **Timeline Synchronization Engine:** Extract precise playback elapsed time metrics from the Windows GSMTC framework and implement a range-matching lookup to determine the active lyric line.
* **Dynamic Panel Hot-Swapping:** Implement an interface state modifier that replaces the entire hardware resource stack (CPU, Cores, RAM, GPU, Net) with the Live Lyrics panel upon pressing a hotkey, preserving the layout flipping features engineered in Phase 2.

---

## 2. Updated Dependency Tree (`Cargo.toml`)

The lyrics module leverages our existing async HTTP infrastructure (`reqwest` and `serde`), requiring no heavy external processing crates. Verify your manifest contains these components:

```toml
[dependencies]
# Existing dependencies (ratatui, crossterm, sysinfo, cpal, rustfft, image, reqwest, serde, windows)
```

---

## 3. Concurrency & Synchronization Pipeline

To prevent networking bottlenecks or intense string manipulation from interrupting the UI rendering loop, lyric fetching and tracking operate inside a segregated worker task.

```
+-----------------------------------------------------------------+
|                     GSMTC WINDOWS MEDIA WORKER                  |
|  1. Capture Track Identification Change (Title, Artist, Duration)|
|  2. Intercept Real-Time Playback Position (TimelineProperties)  |
+-----------------------------------------------------------------+
           |                                       |
           | (Track Changed)                       | (Continuous Timeline Position)
           v                                       v
+-----------------------------------------+ +---------------------+
|         LYRICS FETCH ENGINE             | |  MAIN ENGINE        |
|  1. GET [https://lrclib.net/api/get](https://lrclib.net/api/get)      | |  STATE MACHINE      |
|  2. Download synchronized LRC string    | |                     |
|  3. Parse into Vec<LyricLine>           | |  Receives position  |
+-----------------------------------------+ |  and performs       |
           |                                |  binary search     |
           | AppEvent::LyricsLoaded(Vec)    |  to match frame.    |
           +───────────────────────────────>+---------------------+
                                                       |
                                                       v
                                            [Ratatui Render Canvas]
                                            - Highlight active line
                                            - Center-scroll viewport
```

### Event Bus Extensions (`AppEvent` & State Models)
```rust
pub enum AppEvent {
    // Existing variants...
    LyricsLoaded(Option<Vec<LyricLine>>),
}

pub struct LyricLine {
    pub timestamp: std::time::Duration, // Exact time code when line starts
    pub text: String,
}

pub enum LeftPanelMode {
    HardwareMetrics, // Default system health dashboard
    LiveLyrics,      // Toggled full-height lyrics view
}
```

---

## 4. Deep-Dive Engineering Blueprints

### A. Open-Source Lyric Fetching & Parsing Architecture
Inspired by open-source terminal audio utilities, the client queries public databases using clean, structural HTTP parameters.
* **API Targeting:** Utilize the unrestricted public API `https://lrclib.net/api/get` via our non-blocking `reqwest` client. Pass `artist`, `track_name`, and `duration` (extracted from the Windows GSMTC session) as query selectors.
* **LRC Format Architecture:** Synchronized lyrics are delivered in the standard LRC layout: `[mm:ss.xx] Lyric text phrase`. 
* **Parsing Algorithm:** Write a custom line-by-line string scanner:
  1. Isolate the timestamp string bounded by the `[` and `]` characters.
  2. Parse minutes, seconds, and hundredths of a second into a single, unified `std::time::Duration`.
  3. Extract the remaining text substring, strip trailing whitespace, and collect the structural elements into a `Vec<LyricLine>`.
  4. Ensure the vector is explicitly sorted chronologically by its duration signature.

### B. Timeline Tracking & Lookahead Interpolation
Windows exposes the current playback position via `GlobalSystemMediaTransportControlsSessionTimelineProperties`. However, Windows does not emit position change events on every millisecond tick.
* **The Solution:** When capturing a GSMTC timeline snapshot, log the reported position ($P_{\text{reported}}$) alongside a high-resolution local monotonic timestamp ($T_{\text{captured}} = \text{std::time::Instant::now()}$).
* **Linear Time Extrapolation:** On every internal UI frame tick, extrapolate the real-time playback position ($P_{\text{current}}$) to maintain absolute fluid synchronization between audio frames:
  $$P_{\text{current}} = P_{\text{reported}} + (\text{Instant::now()} - T_{\text{captured}})$$
* **Active Line Binary Search:** Since the `LyricLine` vector is sorted, use a high-speed binary search (`slice::binary_search_by_key`) on every frame loop to resolve the active index $i$ where:
  $$\text{Timestamp}_{i} \le P_{\text{current}} < \text{Timestamp}_{i+1}$$

### C. Dynamic Center-Pinned Viewport Rendering
To build an elite visual reader dashboard, the active lyrics line must remain vertically pinned to the center of the text canvas, smoothly pulling past lines up and out of view.
* **Viewport Offset Calculation:** Let the layout box height allocated by Ratatui be $H$ rows. The middle row index is $M = H / 2$. 
* **Scrolling Logic:** If the active lyric line is at index $A$ in our vector, the render loop should begin printing items from vector index $A - M$ down to $A + M$. If the calculation yields an index below zero, clamp the viewport anchor to the first line of text to prevent out-of-bounds rendering exceptions.
* **Thematic Highlighting Rules:** * **Active Line ($A$):** Render using full bold, high-contrast typography colored with the primary accent tone of the chosen theme palette.
  * **Historical Lines ($< A$):** Render with a dimmed, low-contrast grayscale tone to denote spent phrases.
  * **Upcoming Lines ($> A$):** Render with standard mid-tier visibility text, previewing what is about to be sung next.

### D. Interface Architecture Overhaul (The Workspace Switch)
Introduce a global workspace layout configuration state triggered on application ticks by a designated keyboard interface target (Hotkey: `l`).

* **Standard Viewport Layout (`LeftPanelMode::HardwareMetrics`):**
  * **Left Column:** Displays System Monitors, CPU Core Matrix, Memory Profiling, GPU Telemetry, Storage, and Net stats.
  * **Right Column:** Houses the Media Session Control Panel and the Audio Frequency Spectrum Visualizer.
* **Lyrics Viewport Layout (`LeftPanelMode::LiveLyrics`):**
  * **Left Column:** Completely unmounts the hardware performance stack. The system provisions this entire structural box to render the center-scrolling Live Lyrics panel.
  * **Right Column:** Remains dedicated to painting the active Media state, Album Art graphics canvas, and the reactive Audio Frequency Spectrum.
* **Layout Orientation Compatibility:** This feature must bind cleanly into our Phase 2 orientation inversion layout manager (`f` hotkey). If the layout orientation is inverted, the Lyrics panel effortlessly shifts to the right column, anchoring the Media controls and audio visualizer canvas to the left column.

---

## 5. Visual Dashboard Structure Blueprint

### Standard Lyrics Layout Configuration (`LeftPanelMode::LiveLyrics` / `Orientation == Default`)
```
┌───────────────────────────────────────┐┌──────────────────────────────────────┐
│  LIVE LYRICS SUB-SYSTEM               ││  MEDIA CONTROL DASHBOARD             │
├───────────────────────────────────────┤│                                      │
│  (Dimmed past lyric phrase)           ││  ┌──────────────┐  Now Playing       │
│  (Dimmed past lyric phrase)           ││  │  ALBUM ART   │  Track:  Isolated  │
│                                       ││  │  (UNCODED    │  Artist: Mind Spool│
│ » BOLD ACTIVE HIGHLIGHTED LINE «      ││  │  HALF-BLOCK) │  Album:  The Grid  │
│                                       ││  │  FULL COLOR  │  Year:   2026      │
│  (Upcoming lyric phrase)              ││  └──────────────┘  Status: Playing   │
│  (Upcoming lyric phrase)              ││──────────────────────────────────────│
│  (Upcoming lyric phrase)              ││  REAL-TIME FREQUENCY SPECTRUM        │
│  (Upcoming lyric phrase)              ││  █       █               █           │
│  (Upcoming lyric phrase)              ││  █ █ █ █ █ █ █ █   █ █ █ █ █ █ █ █   │
├───────────────────────────────────────┤│  └───┬───┴───┬───┴───┬───┴───┬───┴───┤
│ [t=Theme | v=View | f=Flip | l=Lyrics]││  └───┴───┴───┴───┴───┴───┴───┴───┴───┤
└───────────────────────────────────────┘└──────────────────────────────────────┘
```

---

## 6. Comprehensive Target Prompt for Code Generation

> Pass this explicit instruction block directly to your code generation model to deploy the Live Lyrics framework additions:

```text
"Act as a principal systems engineer, network infrastructure developer, and specialized terminal UI architect. Refactor our Windows 11 Rust system monitor (`SystemVisor`) to integrate a synchronized Live Lyrics subsystem based strictly on the architectural specification detailed above.

Your implementation must satisfy these precise functional parameters:
1. Asynchronous Open-Source Lyric Fetcher (`src/lyrics_api.rs`): Implement an async HTTP worker query system that listens for track changes from the GSMTC layer. Target the public 'lrclib.net/api/get' REST endpoint, passing artist, track name, and duration parameters via non-blocking reqwest structures.
2. High-Performance LRC Line Parser: Parse raw synchronized LRC payloads. Isolate bracketed timestamp formats '[mm:ss.xx]', convert them into exact thread-safe `std::time::Duration` metrics, pair them cleanly with their following text string slices, and return a chronologically sorted vector of custom `LyricLine` nodes.
3. Linear Timeline Extrapolation Engine: Correct low-frequency timeline updates from the OS. Mix GSMTC `TimelineProperties` positions with local monotonic `std::time::Instant` readings to linearly extrapolate playback time down to the exact millisecond frame. Use high-speed binary searching to find the active lyric text index matching the current time pointer.
4. Center-Pinned Scrolling Renderer (`src/main.rs`): Implement a dynamic vertical viewport calculation inside the text rendering engine. Center-align the active line in the middle row of the target block, applying bold primary accent color themes. Render spent text above the center line in a dimmed grayscale, and upcoming lyrics below in standard visibility text.
5. Workspace Panel Hot-Swapping: Implement a layout mode toggle mapped to the 'l' keyboard key. When activated, unmount the entire performance metrics stack (CPU, Cores, RAM, GPU, Disk, Net) and blit the Live Lyrics view into the column space. Ensure full design compatibility with the 'f' layout inversion flag, allowing lyrics and media blocks to effortlessly swap left/right window columns at runtime.

Maintain our strict zero-emoji design policy, prioritize scannable terminal layouts, and ensure all thread communication via our central AppEvent bus remains entirely non-blocking to the terminal drawing loop."
```