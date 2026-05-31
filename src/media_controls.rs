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

/// Spawns a background thread that polls the Windows GSMTC for active media playback,
/// extracts cover art, and spawns asynchronous year resolution web requests.
pub fn spawn_media_controls_thread(tx: Sender<MediaEvent>) -> Option<thread::JoinHandle<()>> {
    thread::spawn(move || {
        // Initialize the GSMTC Manager (WinRT async resolved blocking)
        let manager = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .and_then(|op| op.get())
        {
            Ok(mgr) => mgr,
            Err(_) => {
                // If GSMTC is unavailable on this OS build, exit thread
                return;
            }
        };

        let mut last_track_identifier = String::new();
        let mut last_playback_state = false;
        let mut last_year: Option<String> = None;
        let mut last_art_hash = 0usize;

        loop {
            // Poll GSMTC state every 1000ms
            thread::sleep(Duration::from_millis(1000));

            // Retrieve active session
            let session = match manager.GetCurrentSession() {
                Ok(sess) => sess,
                Err(_) => {
                    let _ = tx.send(MediaEvent::NoMedia);
                    last_track_identifier.clear();
                    continue;
                }
            };

            // 1. Get Playback Info (IsPlaying status)
            let is_playing = match session.GetPlaybackInfo() {
                Ok(info) => match info.PlaybackStatus() {
                    Ok(status) => status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing,
                    Err(_) => false,
                },
                Err(_) => false,
            };

            // 2. Get Media Properties (Title, Artist, Album)
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

            // Create a unique identifier to verify track changes
            let current_track_identifier = format!("{}|{}|{}", title, artist, album);
            let track_changed = current_track_identifier != last_track_identifier;
            
            // Check if year needs resolving (resolved on track change)
            if track_changed {
                last_track_identifier = current_track_identifier.clone();
                last_playback_state = is_playing;
                last_year = None;
                
                // Immediately emit local metadata update with year as None (Pending)
                let initial_meta = Box::new(MediaMetadata {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    year: None,
                    is_playing,
                });
                let _ = tx.send(MediaEvent::Metadata(initial_meta));

                // Spawn non-blocking background thread to perform MusicBrainz API lookup
                let tx_clone = tx.clone();
                let title_query = title.clone();
                let artist_query = artist.clone();
                let album_query = album.clone();
                
                thread::spawn(move || {
                    if let Some(year) = resolve_release_year(&title_query, &artist_query) {
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

                // Extract Album Art Thumbnail via GSMTC raw byte stream
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
                // If only playback state changed, emit updated metadata retaining existing year
                last_playback_state = is_playing;
                let updated_meta = Box::new(MediaMetadata {
                    title: title.clone(),
                    artist: artist.clone(),
                    album: album.clone(),
                    year: last_year.clone(),
                    is_playing,
                });
                let _ = tx.send(MediaEvent::Metadata(updated_meta));
            }
        }
    }).into()
}

/// Resolves release year using the MusicBrainz recording search API.
fn resolve_release_year(title: &str, artist: &str) -> Option<String> {
    let client = match reqwest::blocking::Client::builder()
        .user_agent("win11_sys_mon/0.1.0 ( saqinnoor@example.com )")
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    // Format query string with explicit filters for Title and Artist
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
            // Attempt to read direct recording release date first
            if let Some(date_str) = rec.first_release_date {
                if date_str.len() >= 4 {
                    return Some(date_str[0..4].to_string());
                }
            }
            // Fallback to iterating inner releases dates
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
