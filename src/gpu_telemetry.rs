use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use crossbeam_channel::Sender;

#[derive(Clone, Debug)]
pub struct GpuSnapshot {
    pub utilization: f32,
    pub vram_used: u64,
    pub vram_total: u64,
}

pub fn spawn_gpu_telemetry_thread(tx: Sender<Box<GpuSnapshot>>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let vram_total = query_total_vram();

        let script = format!(
            r#"while ($true) {{
                $u = (Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine |
                      Measure-Object -Property UtilizationPercentage -Sum).Sum
                $m = (Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory |
                      Measure-Object -Property DedicatedUsage -Sum).Sum
                Write-Output "$u|$m"
                Start-Sleep -Seconds 1
            }}"#
        );

        let mut child = match Command::new("powershell")
            .args(&["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => {
                loop {
                    if tx
                        .send(Box::new(GpuSnapshot {
                            utilization: 0.0,
                            vram_used: 0,
                            vram_total,
                        }))
                        .is_err()
                    {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
                return;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        for line in reader.lines() {
            match line {
                Ok(l) => {
                    let parts: Vec<&str> = l.split('|').collect();
                    if parts.len() == 2 {
                        let utilization = parts[0].trim().parse::<f32>().unwrap_or(0.0);
                        let vram_used = parts[1].trim().parse::<u64>().unwrap_or(0);
                        if tx
                            .send(Box::new(GpuSnapshot {
                                utilization,
                                vram_used,
                                vram_total,
                            }))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }

        let _ = child.wait();
    })
}

fn query_total_vram() -> u64 {
    if let Ok(output) = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_VideoController | Measure-Object -Property AdapterRAM -Sum).Sum",
        ])
        .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        s.trim().parse::<u64>().unwrap_or(0)
    } else {
        0
    }
}
