use std::thread;
use std::time::Duration;
use crossbeam_channel::Sender;
use serde::Deserialize;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionPlaybackStatus, GlobalSystemMediaTransportControlsSessionManager,
};
use windows::Storage::Streams::{Buffer, InputStreamOptions, DataReader};

#[derive(Clone, Debug)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<String>,
    pub is_playing: bool,
}

pub enum MediaEvent {
    Metadata(Box<MediaMetadata>),
    Thumbnail(Vec<u8>),
    NoMedia,
    Timeline(f64, f64, std::time::Instant), // position_secs, total_secs, captured_at
}

// Deserialization structures for MusicBrainz API payload
#[derive(Deserialize, Debug)]
struct MBResponse {
    recordings: Option<Vec<MBRecording>>,
}

#[derive(Deserialize, Debug)]
struct MBRecording {
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
    releases: Option<Vec<MBRelease>>,
}

#[derive(Deserialize, Debug)]
struct MBRelease {
    date: Option<String>,
}

pub fn spawn_media_controls_thread(tx: Sender<MediaEvent>) -> Option<thread::JoinHandle<()>> {
    thread::spawn(move || {
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .and_then(|op| op.get())
        {
            Ok(mgr) => mgr,
            Err(_) => return,
        };

        let mut last_track_identifier = String::new();
        let mut last_playback_state = false;
        let last_year = std::sync::Arc::new(std::sync::Mutex::new(None::<String>));
        let mut last_art_hash = 0usize;

        loop {
            thread::sleep(Duration::from_millis(1000));

            let session = match manager.GetCurrentSession() {
                Ok(sess) => sess,
                Err(_) => {
                    let _ = tx.send(MediaEvent::NoMedia);
                    last_track_identifier.clear();
                    continue;
                }
            };

            let is_playing = match session.GetPlaybackInfo() {
                Ok(info) => match info.PlaybackStatus() {
                    Ok(status) => status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing,
                    Err(_) => false,
                },
                Err(_) => false,
            };

            let props = match session.TryGetMediaPropertiesAsync()
                .and_then(|op| op.get())
            {
                Ok(p) => p,
                Err(_) => {
                    let _ = tx.send(MediaEvent::NoMedia);
                    last_track_identifier.clear();
                    continue;
                }
            };

            let title = props.Title().unwrap_or_default().to_string();
            let artist = props.Artist().unwrap_or_default().to_string();
            let album = props.AlbumTitle().unwrap_or_default().to_string();

            if title.is_empty() && artist.is_empty() {
                let _ = tx.send(MediaEvent::NoMedia);
                last_track_identifier.clear();
                continue;
            }

            let current_track_identifier = format!("{}|{}|{}", title, artist, album);
            let track_changed = current_track_identifier != last_track_identifier;
            
            if track_changed {
                last_track_identifier = current_track_identifier.clone();
                last_playback_state = is_playing;
                *last_year.lock().unwrap() = None;
                
                let initial_meta = Box::new(MediaMetadata {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    year: None,
                    is_playing,
                });
                let _ = tx.send(MediaEvent::Metadata(initial_meta));

                let tx_clone = tx.clone();
                let title_query = title.clone();
                let artist_query = artist.clone();
                let album_query = album.clone();
                let last_year_clone = std::sync::Arc::clone(&last_year);
                
                thread::spawn(move || {
                    if let Some(year) = resolve_release_year(&title_query, &artist_query) {
                        *last_year_clone.lock().unwrap() = Some(year.clone());
                        let updated_meta = Box::new(MediaMetadata {
                            title: title_query,
                            artist: artist_query,
                            album: album_query,
                            year: Some(year),
                            is_playing,
                        });
                        let _ = tx_clone.send(MediaEvent::Metadata(updated_meta));
                    }
                });

                if let Ok(ref_stream) = props.Thumbnail() {
                    if let Ok(stream) = ref_stream.OpenReadAsync().and_then(|op| op.get()) {
                        let size = stream.Size().unwrap_or(0);
                        if size > 0 && size < 10 * 1024 * 1024 { // max 10MB
                            let buffer = Buffer::Create(size as u32).unwrap();
                            if let Ok(result_buffer) = stream.ReadAsync(&buffer, size as u32, InputStreamOptions::None).and_then(|op| op.get()) {
                                if let Ok(reader) = DataReader::FromBuffer(&result_buffer) {
                                    let len = result_buffer.Length().unwrap_or(0) as usize;
                                    let mut bytes = vec![0u8; len];
                                    if reader.ReadBytes(&mut bytes).is_ok() {
                                        // Calculate a simple byte length hash to avoid redundant art updates
                                        let current_hash = bytes.len();
                                        if current_hash != last_art_hash {
                                            last_art_hash = current_hash;
                                            let _ = tx.send(MediaEvent::Thumbnail(bytes));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else if is_playing != last_playback_state {
                last_playback_state = is_playing;
                let year_val = last_year.lock().unwrap().clone();
                let updated_meta = Box::new(MediaMetadata {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    year: year_val,
                    is_playing,
                });
                let _ = tx.send(MediaEvent::Metadata(updated_meta));
            }

            if let Ok(timeline) = session.GetTimelineProperties() {
                let pos = timeline.Position().unwrap_or_default().Duration as f64 / 10_000_000.0;
                let end = timeline.EndTime().unwrap_or_default().Duration as f64 / 10_000_000.0;
                let _ = tx.send(MediaEvent::Timeline(pos, end, std::time::Instant::now()));
            }
        }
    }).into()
}

fn resolve_release_year(title: &str, artist: &str) -> Option<String> {
    let client = match reqwest::blocking::Client::builder()
        .user_agent("win11_sys_mon/0.1.0 ( saqinnoor@example.com )")
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let query = format!("artist:\"{}\" AND recording:\"{}\"", artist, title);
    
    let response = client
        .get("https://musicbrainz.org/ws/2/recording/")
        .query(&[("query", &query), ("fmt", &"json".to_string())])
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }

    let mb_data: MBResponse = response.json().ok()?;
    if let Some(recordings) = mb_data.recordings {
        for rec in recordings {
            if let Some(date_str) = rec.first_release_date {
                if date_str.len() >= 4 {
                    return Some(date_str[0..4].to_string());
                }
            }
            if let Some(releases) = rec.releases {
                for rel in releases {
                    if let Some(date_str) = rel.date {
                        if date_str.len() >= 4 {
                            return Some(date_str[0..4].to_string());
                        }
                    }
                }
            }
        }
    }

    None
}
