use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crossbeam_channel::Sender;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};

pub fn spawn_audio_visualizer_thread(tx: Sender<Vec<f32>>) -> Option<thread::JoinHandle<()>> {
    thread::spawn(move || {
        let host = cpal::default_host();
        
        let device = match host.default_output_device() {
            Some(dev) => dev,
            None => return,
        };

        let config = match device.default_output_config() {
            Ok(cfg) => cfg,
            Err(_) => return,
        };

        let channels = config.channels() as usize;
        let sample_rate = config.sample_rate().0 as f32;

        const FFT_SIZE: usize = 2048;
        let raw_buffer = Arc::new(Mutex::new(Vec::with_capacity(FFT_SIZE * 2)));

        let buffer_clone = Arc::clone(&raw_buffer);
        let error_callback = |_err| {};

        let stream_result = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks_exact(channels) {
                                let mono_sample: f32 = chunk.iter().sum::<f32>() / channels as f32;
                                if buf.len() < FFT_SIZE * 4 {
                                    buf.push(mono_sample);
                                }
                            }
                        }
                    },
                    error_callback,
                    None
                )
            }
            cpal::SampleFormat::I16 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[i16], _: &_| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks_exact(channels) {
                                let mono_sample: f32 = chunk.iter().map(|&s| s as f32 / 32768.0).sum::<f32>() / channels as f32;
                                if buf.len() < FFT_SIZE * 4 {
                                    buf.push(mono_sample);
                                }
                            }
                        }
                    },
                    error_callback,
                    None
                )
            }
            cpal::SampleFormat::U16 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[u16], _: &_| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks_exact(channels) {
                                let mono_sample: f32 = chunk.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).sum::<f32>() / channels as f32;
                                if buf.len() < FFT_SIZE * 4 {
                                    buf.push(mono_sample);
                                }
                            }
                        }
                    },
                    error_callback,
                    None
                )
            }
            _ => return,
        };

        let stream = match stream_result {
            Ok(s) => s,
            Err(_) => return,
        };

        if stream.play().is_err() {
            return;
        }

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        
        let mut hann_window = vec![0.0f32; FFT_SIZE];
        for (i, w) in hann_window.iter_mut().enumerate() {
            *w = 0.5 * (1.0 - ((2.0 * std::f32::consts::PI * i as f32) / (FFT_SIZE as f32 - 1.0)).cos());
        }

        let mut rolling_peak = 0.5f32;

        let mut processing_samples = vec![0.0f32; FFT_SIZE];

        loop {
            thread::sleep(Duration::from_millis(16));

            let mut has_enough_data = false;
            if let Ok(mut buf) = raw_buffer.lock() {
                if buf.len() >= FFT_SIZE {
                    processing_samples.copy_from_slice(&buf[0..FFT_SIZE]);
                    buf.drain(0..(FFT_SIZE / 2));
                    has_enough_data = true;
                }
            }

            if !has_enough_data {
                continue;
            }

            let mut fft_buffer: Vec<Complex<f32>> = processing_samples
                .iter()
                .zip(hann_window.iter())
                .map(|(&sample, &win)| Complex {
                    re: sample * win,
                    im: 0.0,
                })
                .collect();

            fft.process(&mut fft_buffer);

            let half_size = FFT_SIZE / 2;
            let mut magnitudes = vec![0.0f32; half_size];
            for i in 0..half_size {
                let mag = (fft_buffer[i].re * fft_buffer[i].re + fft_buffer[i].im * fft_buffer[i].im).sqrt();
                magnitudes[i] = mag;
            }

            // 16 log-spaced bands: 40 Hz – 16 kHz (sub-bass to air)
            const BANDS: usize = 16;
            let mut bands = vec![0.0f32; BANDS];

            let freq_per_bin = sample_rate / FFT_SIZE as f32;

            let f_min = 40.0f32;
            let f_max = (sample_rate / 2.0).min(16000.0);

            let log_ratio = (f_max / f_min).ln() / BANDS as f32;

            for i in 0..BANDS {
                let f_start = f_min * (log_ratio * i as f32).exp();
                let f_end   = f_min * (log_ratio * (i + 1) as f32).exp();

                let b_start = ((f_start / freq_per_bin).floor() as usize).max(1);
                let b_end   = ((f_end   / freq_per_bin).ceil()  as usize)
                    .max(b_start + 1)
                    .min(half_size);

                let mut max_val = 0.0f32;
                for b in b_start..b_end {
                    max_val = max_val.max(magnitudes[b]);
                }
                bands[i] = max_val;
            }

            rolling_peak *= 0.985;
            let frame_max = bands.iter().cloned().fold(0.0f32, f32::max);
            if frame_max > rolling_peak {
                rolling_peak = frame_max;
            }

            let peak_scale = rolling_peak.max(0.05);

            let mut normalized_bands = vec![0.0f32; BANDS];
            for i in 0..BANDS {
                let norm = bands[i] / peak_scale;
                normalized_bands[i] = norm.min(1.0).max(0.0);
            }

            if tx.send(normalized_bands).is_err() {
                break;
            }
        }
    }).into()
}
