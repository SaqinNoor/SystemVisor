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
#[allow(dead_code)]
pub struct SystemSnapshot {
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub cpu_arch: String,
    pub host_name: String,
    pub uptime: u64,

    pub cpu_count: usize,
    pub global_cpu_usage: f32,
    pub per_core_cpu_usage: Vec<f32>,

    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,

    pub disks: Vec<DiskInfo>,

    pub net_rx_bytes_sec: u64,
    pub net_tx_bytes_sec: u64,
    pub net_total_rx: u64,
    pub net_total_tx: u64,

    pub processes: Vec<ProcessInfo>,
}

pub fn spawn_telemetry_thread(tx: Sender<Box<SystemSnapshot>>, poll_interval: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut sys = System::new_all();
        let mut disks = Disks::new_with_refreshed_list();
        let mut networks = Networks::new_with_refreshed_list();

        sys.refresh_all();
        thread::sleep(Duration::from_millis(200));
        sys.refresh_all();

        let mut last_refresh = Instant::now();

        loop {
        thread::sleep(poll_interval);

        let elapsed = last_refresh.elapsed();
            last_refresh = Instant::now();
            let elapsed_secs = elapsed.as_secs_f64().max(0.001);

            sys.refresh_all();
            disks.refresh_list();
            networks.refresh();

            let os_name = System::name().unwrap_or_else(|| "Windows 11".to_string());
            let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_string());
            let kernel_version = System::kernel_version().unwrap_or_else(|| "Unknown".to_string());
            let cpu_arch = System::cpu_arch().unwrap_or_else(|| "Unknown".to_string());
            let host_name = System::host_name().unwrap_or_else(|| "localhost".to_string());
            let uptime = System::uptime();

            let cpu_count = sys.cpus().len();
            let global_cpu_usage = sys.global_cpu_info().cpu_usage();
            let per_core_cpu_usage: Vec<f32> = sys.cpus().iter().map(|cpu| cpu.cpu_usage()).collect();

            let total_memory = sys.total_memory();
            let used_memory = sys.used_memory();
            let total_swap = sys.total_swap();
            let used_swap = sys.used_swap();

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
            };

            if tx.send(Box::new(snapshot)).is_err() {
                break;
            }
        }
    })
}


