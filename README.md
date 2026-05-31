# SystemVisor

A lightweight, high-performance Windows 11 system monitor for your terminal.

Get real-time insights into your system's CPU, memory, disk, and network performance directly from your command line. SystemVisor is built for developers and power users who want instant system diagnostics without bloated GUI applications.

## Features

- **Real-time CPU monitoring** — Per-core and global CPU usage at a glance
- **Memory & swap tracking** — Comprehensive memory usage visualization
- **Disk and network stats** — Monitor I/O and network throughput in real-time
- **Process monitoring** — Track top processes by CPU and memory consumption
- **Responsive TUI** — Smooth, responsive terminal interface powered by Ratatui
- **Lightweight** — Minimal resource overhead while you work

## Quick Start

### Option 1: Download Pre-built Release (Easiest)

Head over to the [Releases](https://github.com/SaqinNoor/systemvisor/releases) page and download the latest `systemvisor.exe` or the portable ZIP. No build tools needed — just download and run.

```bash
systemvisor.exe
```

### Option 2: Build from Source

#### Prerequisites

- Windows 11
- Rust 1.70+ (from [rustup.rs](https://rustup.rs/))

#### Build

```bash
cargo build --release
```

The compiled binary will be available at `target/release/systemvisor.exe`.

### Run

```bash
./target/release/systemvisor.exe
```

Or, if you're using a release binary, simply run `systemvisor.exe`.

## Controls

- **Arrow keys** — Navigate between sections and sort processes
- **Tab** — Switch focus between panels
- **q** — Quit the application

## Building for Distribution

To create a release-ready binary:

```bash
cargo build --release --locked
```

The resulting `.exe` is fully portable and requires no additional dependencies on Windows 11.

## Development

This project is actively developed. Check the [design_doc.md](design_doc.md) for planned features including audio visualization and media session integration.

### Project Structure

```
systemvisor/
├── src/
│   ├── main.rs          — Main TUI logic and event loop
│   └── telemetry.rs     — System data collection
├── Cargo.toml           — Rust project manifest
└── README.md            — You are here
```

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.

## Contributing

Found a bug or have a feature request? Contributions are welcome. Feel free to open an issue or submit a pull request.

---

Built with ❤️ by SaqinNoor
