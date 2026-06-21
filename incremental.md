# Incremental Architecture Update: Media Timeline Tracking & High-Fidelity Dithered Graphics

## 1. Feature Objective & Scope
This patch details an incremental enhancement package for the `SystemVisor` media control panel. It introduces sub-second media timeline progress tracking and upgrades the visual rendering fidelity of the album art engine without departing from character-grid console constraints.

### Specific Scope
* **Linear Progress Gauge:** Add a dynamic, smoothly updating timeline progress bar within the Media layout block, showcasing elapsed track time versus total duration.
* **Perceived High-Resolution Album Art:** Overhaul the image processing worker thread to implement a color-quantization and error-diffusion algorithm (Floyd-Steinberg Dithering). This preserves granular micro-details, textures, and gradients, making the artwork look significantly sharper while maintaining a pure ANSI text-mode aesthetic.

---

## 2. Technical Blueprint & Mathematical Paradigms

### A. Media Timeline Progress Bar Engine
Building upon the linear time extrapolation engine from previous versions, the UI thread calculates the exact track progression state on every internal redraw loop tick.

#### 1. Mathematical Formulas
Let $P_{\text{current}}$ be the extrapolated playback position duration, and $D_{\text{total}}$ be the total duration of the track asset provided by the Windows GSMTC state stream.

$$\text{Progress Ratio} = \min\left(1.0, \frac{P_{\text{current}.\text{as\_secs\_f64}()}}{D_{\text{total}.\text{as\_secs\_f64}()}}\right)$$

#### 2. Formatting Logic
The elapsed and total durations must be converted into standardized time-stamp layouts (`MM:SS` or `HH:MM:SS` if the asset duration exceeds 60 minutes):

```rust
fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}
```

The progress widget will be constructed manually inside a text block using clean geometric Unicode blocks (`█` for elapsed increments, `░` for remaining cells) to adhere strictly to our zero-emoji design parameters.

### B. High-Fidelity Image Rendering via Error Diffusion
Standard downsampling often groups unique color zones into solid, muddy blocks, erasing faces, text, and logos from album covers. To artificially boost structural resolution within a tight console grid, we deploy **Floyd-Steinberg Error Diffusion Dithering**.

#### 1. The Dithering Framework
As the background image thread steps through the downsampled RGB matrix, it calculates the difference (quantization error) between the original pixel color and the closest color available in the terminal palette space. It then diffuses this error outward to neighboring un-processed pixels using specific fractional weights:

```
          [ Current Pixel ]   -->   7/16 (Right)
3/16 (Down-Left)  5/16 (Down) -->   1/16 (Down-Right)
```

#### 2. Structural Algorithm Execution Flow
For every pixel $(x, y)$ processing through the `src/art_processor.rs` thread:
1. Fetch current color value $C_{\text{old}} = \text{Matrix}[x][y]$.
2. Match $C_{\text{old}}$ to the closest console-translatable true-color token $C_{\text{new}}$.
3. Store $C_{\text{new}}$ into the final drawing buffer map.
4. Compute the structural error vector: $E = C_{\text{old}} - C_{\text{new}}$ (calculated independently across Red, Green, and Blue color channels).
5. Disperse portions of $E$ into adjacent matrix coordinates:
   * $\text{Matrix}[x+1][y]   \gets \text{Matrix}[x+1][y]   + \left(E \times \frac{7}{16}\right)$
   * $\text{Matrix}[x-1][y+1] \gets \text{Matrix}[x-1][y+1] + \left(E \times \frac{3}{16}\right)$
   * $\text{Matrix}[x][y+1]   \gets \text{Matrix}[x][y+1]   + \left(E \times \frac{5}{16}\right)$
   * $\text{Matrix}[x+1][y+1] \gets \text{Matrix}[x+1][y+1] + \left(E \times \frac{1}{16}\right)$

This process generates an ultra-fine pixel stippling effect. When rendered via Unicode half-blocks (`▄`), the human eye blends the dithered grain patterns together, perceiving micro-details and text lines that would normally be invisible at low TUI grid resolutions.

---

## 3. Updated Visual Dashboard Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  MEDIA MONITOR DASHBOARD                                                     │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌────────────────────────┐  Now Playing                                     │
│  │ ▄▀▄▄▀▄██▄▀▄▀▄██▄▀▄▀▄██ │  Track:  Isolated                                 │
│  │ █▄█▀█▄███▄█▀█▄███▄█▀█▄ │  Artist: Mind Spool                              │
│  │ ▀▄▀▀▄▀██▀▄▀▀▄▀██▀▄▀▀▄▀ │  Album:  The Grid                                │
│  │ (HIGH-FIDELITY DITHER) │  Year:   2026 [API]                              │
│  └────────────────────────┘  Status: Playing                                 │
│                                                                              │
│  Track Progress:                                                             │
│  [██████████████████████████░░░░░░░░░░░░░░░░░░░░] 02:14 / 03:45              │
│                                                                              │
│  REAL-TIME FREQUENCY SPECTRUM                                                │
│  █           █                   █                                           │
│  █     █     █                   █     █                                     │
│  █ █ █ █ █ █ █ █   █ █ █ █ █ █ █ █   █ █ █ █                                 │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Targeted Prompt for Code Generation

Act as a principal systems software engineer and digital imaging expert. Implement the incremental progress bar and high-fidelity dithering updates for our Windows 11 Rust TUI system monitor (`SystemVisor`) based on the architecture specification in incremental.md.

Execute these precise programmatic requirements:
1. Media Timeline Progress Bar: Expand the media layout view container to display a dedicated track timeline widget. Format current elapsed time positions and total track durations into 'MM:SS' or 'HH:MM:SS' layouts. Render a clean progress bar utilizing geometric Unicode blocks ('█' and '░') that maps precisely to the real-time extrapolated playback ratio.
2. High-Resolution Dithering Engine (`src/art_processor.rs`): Rewrite the image processing path to ingest the raw Windows GSMTC thumbnail bytes and apply a full Floyd-Steinberg error-diffusion dithering algorithm across the RGB channels. Ensure the quantization errors are mathematically balanced and distributed across neighboring array rows (+7/16, +3/16, +5/16, +1/16). 
3. Visual Continuity: Ensure the dithered matrix renders cleanly using the established Unicode half-block ('▄') foreground/background color mapping framework. The dithered texture must dramatically sharpen the visual definition of the album cover arts while safely maintaining a pure text-mode CLI aesthetic.

Ensure all image math operations remain strictly isolated inside our non-blocking async background worker thread to prevent rendering frame-drops on the main drawing pipeline.