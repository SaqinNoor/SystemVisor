use std::thread;
use std::time::{Duration, Instant};
use crossbeam_channel::Sender;
use sysinfo::{System, Disks, Networks};

#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_space: u64,
    pub available_space: u64,
    pub file_system: String,
}

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory: u64,
}

#[derive(Clone, Debug)]
pub struct SystemSnapshot {
    // OS / System metadata
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub cpu_arch: String,
    pub host_name: String,
    pub uptime: u64,

    // CPU usage
    pub cpu_count: usize,
    pub global_cpu_usage: f32,
    pub per_core_cpu_usage: Vec<f32>,

    // Memory usage
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,

    // Disks
    pub disks: Vec<DiskInfo>,

    // Network throughput
    pub net_rx_bytes_sec: u64,
    pub net_tx_bytes_sec: u64,
    pub net_total_rx: u64,
    pub net_total_tx: u64,

    // Processes
    pub processes: Vec<ProcessInfo>,

    // GPU Metrics
    pub gpu_usage: f32,
    pub gpu_vram_used: u64,
    pub gpu_vram_total: u64,
}

/// Spawns a background thread to gather Windows metrics using the `sysinfo` crate.
/// Snapshots are passed cleanly through the crossbeam channel `tx`.
pub fn spawn_telemetry_thread(tx: Sender<Box<SystemSnapshot>>, poll_interval: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // Initialize sysinfo structs
        let mut sys = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        let mut networks = Networks::new_with_refreshed_list();

        // Perform initial refresh for CPU metrics delta calculations
        sys.refresh_all();
        thread::sleep(Duration::from_millis(200));
        sys.refresh_all();

        let mut last_refresh = Instant::now();
        let gpu_vram_total = query_total_vram();

        loop {
            // Keep thread alive at specified polling interval
            thread::sleep(poll_interval);

            // Record precise elapsed time to compute throughput accurately
            let elapsed = last_refresh.elapsed();
            last_refresh = Instant::now();
            let elapsed_secs = elapsed.as_secs_f64().max(0.001); // avoid division by zero

            // Refresh system data
            sys.refresh_all();

            // Refresh disks. In sysinfo 0.30, refresh_list updates disk info.
            disks.refresh_list();

            // Refresh network interfaces data
            networks.refresh();

            // 1. Gather System Metadata (retaining standard default if unavailable)
            let os_name = System::name().unwrap_or_else(|| "Windows 11".to_string());
            let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
            let kernel_version = System::kernel_version().unwrap_or_else(|| "Unknown".to_string());
            let cpu_arch = System::cpu_arch().unwrap_or_else(|| "Unknown".to_string());
            let host_name = System::host_name().unwrap_or_else(|| "localhost".to_string());
            let uptime = System::uptime();

            // 2. CPU Metrics
            let cpu_count = sys.cpus().len();
            let global_cpu_usage = sys.global_cpu_info().cpu_usage();
            let per_core_cpu_usage: Vec<f32> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();

            // 3. Memory metrics (bytes)
            let total_memory = sys.total_memory();
            let used_memory = sys.used_memory();
            let total_swap = sys.total_swap();
            let used_swap = sys.used_swap();

            // 4. Disks Metrics
            let disks_data: Vec<DiskInfo> = disks
                .iter()
                .map(|disk| DiskInfo {
                    name: disk.name().to_string_lossy().into_owned(),
                    mount_point: disk.mount_point().to_string_lossy().into_owned(),
                    total_space: disk.total_space(),
                    available_space: disk.available_space(),
                    file_system: disk.file_system().to_string_lossy().into_owned(),
                })
                .collect();

            // 5. Network Metrics
            let mut rx_delta = 0;
            let mut tx_delta = 0;
            let mut net_total_rx = 0;
            let mut net_total_tx = 0;

            for (_interface_name, data) in networks.iter() {
                rx_delta += data.received();
                tx_delta += data.transmitted();
                net_total_rx += data.total_received();
                net_total_tx += data.total_transmitted();
            }

            let net_rx_bytes_sec = (rx_delta as f64 / elapsed_secs) as u64;
            let net_tx_bytes_sec = (tx_delta as f64 / elapsed_secs) as u64;

            // 6. Processes Metrics
            // Retrieve all processes. In sysinfo 0.30, processes() returns HashMap<Pid, Process>
            let processes: Vec<ProcessInfo> = sys
                .processes()
                .iter()
                .map(|(&pid, proc)| ProcessInfo {
                    pid: pid.as_u32(),
                    name: proc.name().to_string(),
                    cpu_usage: proc.cpu_usage(),
                    memory: proc.memory(),
                })
                .collect();
            
            let (gpu_usage, gpu_vram_used) = query_gpu_metrics();

            let snapshot = SystemSnapshot {
                os_name,
                os_version,
                kernel_version,
                cpu_arch,
                host_name,
                uptime,
                cpu_count,
                global_cpu_usage,
                per_core_cpu_usage,
                total_memory,
                used_memory,
                total_swap,
                used_swap,
                disks: disks_data,
                net_rx_bytes_sec,
                net_tx_bytes_sec,
                net_total_rx,
                net_total_tx,
                processes,
                gpu_usage,
                gpu_vram_used,
                gpu_vram_total,
            };

            // Send system state snapshot to UI thread
            if tx.send(Box::new(snapshot)).is_err() {
                // Main UI thread disconnected, exit background thread cleanly
                break;
            }
        }
    })
}

fn query_total_vram() -> u64 {
    if let Ok(output) = std::process::Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_VideoController | Measure-Object -Property AdapterRAM -Sum).Sum"
        ])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    }
}

fn query_gpu_metrics() -> (f32, u64) {
    if let Ok(output) = std::process::Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine | Measure-Object -Property UtilizationPercentage -Sum).Sum; (Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory | Measure-Object -Property DedicatedUsage -Sum).Sum"
        ])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        let mut lines = s.lines();
        let util = lines.next().and_then(|l| l.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let vram = lines.next().and_then(|l| l.trim().parse::<u64>().ok()).unwrap_or(0);
        (util, vram)
    } else {
        (0.0, 0)
    }
}
