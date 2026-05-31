# System Architecture Update & Feature Specification: Integrated Media Dashboard, Audio Visualizer, and Layout Optimization

## 1. Feature Objective & Scope
This specification details a comprehensive upgrade package for the `win11_sys_mon` terminal dashboard. The application will be expanded from a pure hardware resource monitor into a high-performance system-and-media suite.

### Core Architecture Goals
* **Windows Loopback Audio Capture & DSP:** Tap natively into the active Windows 11 output audio device, performing real-time Fast Fourier Transform (FFT) analysis to drive a fluid audio visualizer.
* **Auto-Adjusting Visualizer Gain:** Implement a dynamic automatic gain control (AGC) pipeline that normalizes audio visualization inputs on the fly, preventing the frequency bars from maxing out during loud tracks or disappearing during quiet ones.
* **Global Media Session, Album Art, & Online Metadata:** Interface with the Windows Runtime (WinRT) to track system-wide media states, dynamically parsing song metadata and extracting raw thumbnail streams. Integrate an online metadata resolution pipeline to fetch missing data fields (such as Release Year) that the native OS payload does not provide.
* **Layout Scaling & Aesthetic Refinement:** Re-engineer the layout calculations to ensure that multi-threaded, high-core-count Windows CPUs scale gracefully within their UI blocks, while stripping out unprofessional visual decorations in favor of an elite, minimalist developer aesthetic.

---

## 2. Combined Dependency Tree (`Cargo.toml`)

Add these specialized dependencies to the existing hardware-monitoring project stack. Ensure optimization features are explicitly enabled for processing intensive graphics operations:

```toml
[dependencies]
# TUI and Terminal Abstraction
ratatui = { version = "0.26", features = ["crossterm"] }
crossterm = { version = "0.27", features = ["event"] }
sysinfo = "0.30"
crossbeam-channel = "0.5"

# Audio Stream Capture (WASAPI loopback binding)
cpal = "0.15"

# Fast Fourier Transform engine for DSP pipelines
rustfft = "6.1"

# Dynamic image decoding and resizing utilities (optimized feature subset)
image = { version = "0.24", default-features = false, features = ["png", "jpeg"] }

# Asynchronous HTTP client for metadata resolution APIs
reqwest = { version = "0.11", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }

# Windows Native Runtime APIs for Global Media Session tracking
windows = { version = "0.52", features = [
    "Media_Control",
    "Foundation",
    "Storage_Streams"
] }
```

---

## 3. Concurrency & Multi-Threaded Pipeline Architecture

To maintain a locked UI framerate of 60 FPS without UI micro-stutters or audio degradation, the system delegates blocking execution trees across four dedicated background threads.

```
+-----------------------------------------------------------------+
|                       AUDIO CAPTURE WORKER                      |
|  1. Capture raw PCM via cpal WASAPI default loopback stream      |
|  2. Windowing (Hann) & Real-time FFT processing (rustfft)        |
|  3. Run Dynamic AGC Auto-Gain Filter to auto-normalize bounds   |
|  4. Pipe normalized frequency arrays down to UI channel          |
+-----------------------------------------------------------------+
                                | AppEvent::AudioSpectrum(Vec<f32>)
                                v
+-----------------------------------------------------------------+
|                     GSMTC WINDOWS MEDIA WORKER                  |
|  1. Await Global Media Transport Session updates                |
|  2. Emit textual track metadata (Title, Artist, Album)          |
|  3. Spawn Async HTTP Task to query missing Release Year API     |
|  4. Fetch raw .Thumbnail IRandomAccessStream byte array         |
+-----------------------------------------------------------------+
                                | Payload Stream bytes
                                v
+-----------------------------------------------------------------+
|                    IMAGE PROCESSING WORKER                      |
|  1. Intercept stream bytes and load via image::load_from_memory |
|  2. Apply a ~2.0 vertical aspect ratio compensation stretch    |
|  3. Resize down to match visualizer panel grid layout dimensions |
+-----------------------------------------------------------------+
                                | AppEvent::NewAlbumArt(Matrix)
                                v
+-----------------------------------------------------------------+
|                       MAIN UI THREAD (Ratatui)                  |
|  - Process events, execute decay filters, draw unified widgets.  |
+-----------------------------------------------------------------+
```

### Event Bus Specifications (`AppEvent` Enum)
```rust
pub enum AppEvent {
    Tick,
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    AudioSpectrum(Vec<f32>),
    MediaUpdate(Option<MediaMetadata>),
    NewAlbumArt(Vec<Vec<(u8, u8, u8)>>), // 2D matrix of RGB tuples for the UI blitter
}

pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<String>, // Resolved asynchronously via network API
    pub is_playing: bool,
}
```

---

## 4. Deep-Dive Engineering Blueprints

### A. Windows Media Session & Online Year Resolution
Instead of inspecting process windows or running polling scripts, the background worker ties directly into the WinRT **Global System Media Transport Controls Session Manager**.
* **Hooking:** Request access via `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()`.
* **Events:** Subscribe to `CurrentSessionChanged` and bind listeners to the active session’s `MediaPropertiesChanged` and `PlaybackInfoChanged` properties.
* **Online Year Fetch Pipeline:** The Windows native GSMTC payload returns song title, artist, and album, but it does *not* provide release year tracking. When a metadata change event fires, the media worker must catch the text strings and spawn an asynchronous non-blocking HTTP request via `reqwest` to a public database API (such as the MusicBrainz API or MusicMatch API). Once the payload returns the structural track profile, parse out the release date string and send an updated `MediaMetadata` payload to the UI channel.
* **Thumbnail Extraction:** Execute `.GetThumbnailAsync()`, open an asynchronous read stream via `IRandomAccessStreamWithContentType`, and copy the raw buffer directly over to a thread-safe memory vector (`Vec<u8>`) to send to the image pipeline.

### B. High-Fidelity Album Art Rendering via "Half-Blocks"
To output clear, colored album artwork inside a character-based console, the application implements a text-mode frame buffer shortcut utilizing the Unicode half-block symbol (`▄`). 

Because a single terminal cell is roughly twice as tall as it is wide, splitting it vertically allows a single character space to render two independent square color blocks:
1. **The Worker Step:** The background processing thread downsamples the image using `image::imageops::FilterType::Lanczos3`. It applies a scale adjustment factor (stretching the height or squeezing the width) to neutralize terminal font distortion.
2. **The Render Loop Step:** When iterating through the downsampled RGB matrix, every loop step samples two vertical pixels at a time:
   * Set the terminal cell's **Foreground Color** to match the upper pixel's RGB value.
   * Set the terminal cell's **Background Color** to match the lower pixel's RGB value.
   * Draw the character `▄`.

### C. Audio Pipeline, FFT, & Auto-Adjusting Frequency Gain
* **Host Setup:** Configure `cpal` to use the Windows WASAPI loopback device: `cpal::platform::DeviceExtWindows::default_loopback_input_device()`.
* **DSP Processing:** Collect incoming PCM data streams into discrete sample window frames ($N = 1024$ or $2048$). Smooth out edge-discontinuity artifacts by filtering buffers through a Hann window function before passing the samples into `rustfft`.
* **Auto-Adjusting Visualizer Gain Engine:** To fix the issue found in basic visualizers where loud music clips/maxes out all rendering bars and quiet music leaves the bars flat, the engine must implement dynamic scaling. Track a rolling peak threshold ($P_{\text{peak}}$) of the highest frequency bin over a moving time window (e.g., the last 2-3 seconds). Normalize the current frame's values relative to this dynamic peak range instead of hardcoded maximum constants:
  $$\text{Normalized Amplitude} = \frac{\text{Current Amplitude}}{P_{\text{peak}}}$$
  To prevent erratic bouncing, apply a slow relaxation envelope to $P_{\text{peak}}$ when the volume drops. This ensures that the visualizer auto-calibrates instantly to soft acoustic tracks, heavy electronic bass lines, or maxed system volumes without clipping.
* **Smoothing Filter:** Apply a persistent exponential smoothing decay calculation on the UI main thread during every redraw step to avoid visual jitter:
  $$V_{\text{current}} = \max(V_{\text{target}}, V_{\text{previous}} \times 0.85)$$

### D. Layout Redesign & Aesthetic Rules

#### 1. CPU Core Layout Scaling Fix
* **The Bug:** Hardcoded grid positioning for per-core performance metrics breaks when running on modern processors containing high logical thread counts (16, 24, 32, or 64 threads), resulting in broken containers and unreadable data blocks.
* **The Refactoring Requirements:** The per-core monitoring system must remain an integral part of the dashboard, but it must dynamically scale its layout contents. 
* **Dynamic Grid Solver:** Query the engine for the logical core count via `sysinfo::System::cpus().len()`. Calculate the available character area inside the layout block, and programmatically compute an optimized multi-column/multi-row matrix grid wrapper. Dynamically adjust individual core label sizes and bar chart widths based on available screen space, switching to a high-density, ultra-compact text visualization format if screen space falls below established min-width thresholds.

#### 2. Aesthetic Uniformity & Iconography Guidelines
To ensure the terminal interface presents as an elite, production-grade diagnostic application rather than an AI-generated prototype, developers must enforce strict design guidelines:
* **Zero Emojis Allowed:** Standard emojis (e.g., 🎵, 📊, 💻, 🧠) are completely banned from all blocks, labels, headers, and UI widgets.
* **Minimalist UI Indicators:** Use clean, standard geometric Unicode structures (`■`, `▲`, `▼`, `░`, `▒`, `▓`), custom block fills, and standard line drawing box components (`│`, `─`, `┌`, `┐`). This maintains visual uniformity across dark terminal themes and respects system console font settings.

---

## 5. Visual Dashboard Structure Blueprint

```
┌───────────────────────────────────────┐┌──────────────────────────────────────┐
│  SYSTEM MONITOR  ■ CPU: 24% [▓▒░░░░]  ││  MEDIA CONTROL DASHBOARD             │
├───────────────────────────────────────┤│                                      │
│ CORES PROFILE (Auto-Scaling Grid)     ││  ┌──────────────┐  Now Playing        │
│ C01: [████] C02: [██]   C03: [██████] ││  │  ALBUM ART   │  Track:  Isolated  │
│ C04: [█]    C05: [████] C06: [██████] ││  │  (UNCODED    │  Artist: Mind Spool│
│ C07: [███]  C08: [█]    C09: [███]    ││  │  HALF-BLOCKS)│  Album:  The Grid  │
│ C10: [████] C11: [██]   C12: [█]      ││  │  FULL COLOR  │  Year:   2026 [API]│
├───────────────────────────────────────┤│  └──────────────┘  Status: Playing   │
│ MEMORY PROFILE                        ││                                      │
│ RAM:  [████████████████░░░░░] 16.2 GB ││  REAL-TIME FREQUENCY SPECTRUM        │
│ SWAP: [██░░░░░░░░░░░░░░░░░░░]  2.1 GB ││  █       █               █           │
│                                       ││  █   █   █               █   █       │
│ STORAGE & NET                         ││  █   █   █   █       █   █   █   █   │
│ C:\ [██████░░]   Up:   1.2 MB/s       ││  █ █ █ █ █ █ █ █   █ █ █ █ █ █ █ █   │
│ D:\ [████████]   Down: 8.4 MB/s       ││  └───┬───┴───┬───┴───┬───┴───┬───┴───┤
└───────────────────────────────────────┘└──────┴───────┴───────┴───────┴───────┘
```

---

