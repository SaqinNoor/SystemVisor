use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use crossbeam_channel::Sender;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};

/// Spawns a background thread to capture WASAPI loopback audio and stream frequency spectrum.
pub fn spawn_audio_visualizer_thread(tx: Sender<Vec<f32>>) -> Option<thread::JoinHandle<()>> {
    thread::spawn(move || {
        let host = cpal::default_host();
        
        // On Windows WASAPI loopback, we open an input stream on the default OUTPUT device!
        let device = match host.default_output_device() {
            Some(dev) => dev,
            None => {
                // Exit thread if no audio output device is available
                return;
            }
        };

        // We MUST query the default output configuration because loopback format
        // is determined by the output device rendering properties.
        let config = match device.default_output_config() {
            Ok(cfg) => cfg,
            Err(_) => return,
        };

        let channels = config.channels() as usize;
        // Capture sample rate BEFORE config is moved into build_input_stream.
        // This is essential for Hz-calibrated frequency band mapping below.
        let sample_rate = config.sample_rate().0 as f32;

        // Larger FFT gives better low-frequency resolution:
        // at 44100 Hz, 2048-point FFT → ~21.5 Hz per bin (vs 43 Hz with 1024)
        const FFT_SIZE: usize = 2048;
        let raw_buffer = Arc::new(Mutex::new(Vec::with_capacity(FFT_SIZE * 2)));

        // CPAL input callback
        let buffer_clone = Arc::clone(&raw_buffer);
        let error_callback = |_err| {};

        let stream_result = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                device.build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &_| {
                        if let Ok(mut buf) = buffer_clone.lock() {
                            for chunk in data.chunks_exact(channels) {
                                // Downmix multi-channel to mono
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

        // Initialize FFT Planner
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        
        // Hann window coefficients
        let mut hann_window = vec![0.0f32; FFT_SIZE];
        for (i, w) in hann_window.iter_mut().enumerate() {
            *w = 0.5 * (1.0 - ((2.0 * std::f32::consts::PI * i as f32) / (FFT_SIZE as f32 - 1.0)).cos());
        }

        // Rolling peak tracker for Automatic Gain Control (AGC)
        let mut rolling_peak = 0.5f32;

        // Local processing buffer
        let mut processing_samples = vec![0.0f32; FFT_SIZE];

        // DSP analysis loop running independently of the CPAL stream callback
        loop {
            // Keep analysis loop running roughly at ~60 Hz (16ms)
            thread::sleep(Duration::from_millis(16));

            let mut has_enough_data = false;
            if let Ok(mut buf) = raw_buffer.lock() {
                if buf.len() >= FFT_SIZE {
                    // Extract the latest FFT_SIZE samples
                    processing_samples.copy_from_slice(&buf[0..FFT_SIZE]);
                    // Shift buffer (50% overlap for smooth updates)
                    buf.drain(0..(FFT_SIZE / 2));
                    has_enough_data = true;
                }
            }

            if !has_enough_data {
                continue;
            }

            // 1. Apply Hann Windowing
            let mut fft_buffer: Vec<Complex<f32>> = processing_samples
                .iter()
                .zip(hann_window.iter())
                .map(|(&sample, &win)| Complex {
                    re: sample * win,
                    im: 0.0,
                })
                .collect();

            // 2. Perform FFT Forward Pass
            fft.process(&mut fft_buffer);

            // 3. Compute bin magnitudes (first half represents up to Nyquist frequency)
            let half_size = FFT_SIZE / 2;
            let mut magnitudes = vec![0.0f32; half_size];
            for i in 0..half_size {
                let mag = (fft_buffer[i].re * fft_buffer[i].re + fft_buffer[i].im * fft_buffer[i].im).sqrt();
                magnitudes[i] = mag;
            }

            // 4. Musically-calibrated logarithmic frequency binning into 16 bands.
            //
            // Uses true exponential (octave-proportional) spacing anchored to real Hz values
            // derived from the device sample_rate. This ensures:
            //   Band  0:  ~40 - 80 Hz   (sub-bass / kick drum)
            //   Band  1:  ~80 - 120 Hz  (bass guitar fundamental)
            //   Band  2: ~120 - 200 Hz  (upper bass)
            //   Band  3: ~200 - 320 Hz  (low mids / warmth)
            //   Band  4: ~320 - 500 Hz  (mids)
            //   Band  5: ~500 - 800 Hz  (upper mids / body)
            //   Band  6: ~800 - 1.2 kHz (presence low)
            //   Band  7: ~1.2 - 2.0 kHz (presence high / main vocal range)
            //   Band  8: ~2.0 - 3.2 kHz (attack transients)
            //   Band  9: ~3.2 - 5.0 kHz (clarity / consonants)
            //   Band 10: ~5.0 - 8.0 kHz (brilliance)
            //   Band 11: ~8.0 - 12 kHz  (air high)
            //   Bands 12-15: 12 kHz+ shimmer
            const BANDS: usize = 16;
            let mut bands = vec![0.0f32; BANDS];

            // Frequency resolution: Hz per FFT bin
            let freq_per_bin = sample_rate / FFT_SIZE as f32;

            // Musical range: 40 Hz (lowest bass) to 16 kHz (air), capped at Nyquist
            let f_min = 40.0f32;
            let f_max = (sample_rate / 2.0).min(16000.0);

            // Exponential step per band: each band is (f_max/f_min)^(1/BANDS) wider than the last
            let log_ratio = (f_max / f_min).ln() / BANDS as f32;

            for i in 0..BANDS {
                // True Hz band edges
                let f_start = f_min * (log_ratio * i as f32).exp();
                let f_end   = f_min * (log_ratio * (i + 1) as f32).exp();

                // Convert Hz edges to FFT bin indices
                let b_start = ((f_start / freq_per_bin).floor() as usize).max(1);
                let b_end   = ((f_end   / freq_per_bin).ceil()  as usize)
                    .max(b_start + 1)
                    .min(half_size);

                // Peak-pick inside this band's bin range
                let mut max_val = 0.0f32;
                for b in b_start..b_end {
                    max_val = max_val.max(magnitudes[b]);
                }
                bands[i] = max_val;
            }

            // 5. Automatic Gain Control (AGC)
            // Track dynamic peak with a decay relaxation envelope
            rolling_peak *= 0.985; // slow decay
            let frame_max = bands.iter().cloned().fold(0.0f32, f32::max);
            if frame_max > rolling_peak {
                rolling_peak = frame_max;
            }

            // Clamp peak to avoid infinite amplification during silence
            let peak_scale = rolling_peak.max(0.05);

            // Normalize and scale current bands
            let mut normalized_bands = vec![0.0f32; BANDS];
            for i in 0..BANDS {
                let norm = bands[i] / peak_scale;
                normalized_bands[i] = norm.min(1.0).max(0.0);
            }

            // Stream analysis vector back to the main TUI event loop
            if tx.send(normalized_bands).is_err() {
                // Main UI thread disconnected, exit thread
                break;
            }
        }
    }).into()
}
