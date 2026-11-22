//! Probe queued files off the UI thread, including Opus/Vorbis stream tags.
use anyhow::{Context, Result};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackInfo {
    pub track: Option<u32>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<f64>,
    pub format: String,
    pub sample_rate: Option<f64>,
    pub bitrate: Option<f64>,
}
impl TrackInfo {
    pub fn fallback(path: &Path) -> Self {
        Self {
            title: crate::library::label(path),
            format: path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_uppercase(),
            ..Self::default()
        }
    }
    pub fn from_probe(path: &Path, probe: &Value) -> Self {
        let mut tags = HashMap::new();
        // Container tags take precedence; audio-stream tags fill the gaps.
        for object in [
            probe["format"]["tags"].as_object(),
            probe["streams"][0]["tags"].as_object(),
        ]
        .into_iter()
        .flatten()
        {
            for (key, value) in object {
                if let Some(value) = value.as_str().map(str::trim).filter(|v| !v.is_empty()) {
                    tags.entry(key.to_ascii_lowercase())
                        .or_insert_with(|| value.to_owned());
                }
            }
        }
        let mut info = Self::fallback(path);
        if let Some(title) = tags.get("title") {
            info.title = title.clone();
        }
        info.artist = tags.get("artist").cloned().unwrap_or_default();
        info.album = tags.get("album").cloned().unwrap_or_default();
        info.track = tags
            .get("track")
            .or_else(|| tags.get("tracknumber"))
            .and_then(|v| v.split('/').next()?.trim().parse::<u32>().ok())
            .filter(|n| *n > 0);
        info.duration = [
            &probe["format"]["duration"],
            &probe["streams"][0]["duration"],
        ]
        .into_iter()
        .find_map(|v| {
            v.as_f64()
                .or_else(|| v.as_str()?.parse().ok())
                .filter(|v| v.is_finite() && *v >= 0.)
        });
        info.sample_rate = crate::playback::positive_number(&probe["streams"][0]["sample_rate"]);
        info.bitrate = crate::playback::positive_number(&probe["streams"][0]["bit_rate"])
            .or_else(|| crate::playback::positive_number(&probe["format"]["bit_rate"]));
        if let Some(codec) = probe["streams"][0]["codec_name"]
            .as_str()
            .filter(|v| !v.is_empty())
        {
            if !codec.starts_with("pcm_") {
                info.format = codec.to_uppercase();
            }
        }
        info
    }
}
/// Sum queue entries, including duplicates; unknown/invalid durations stay explicit.
pub fn total_duration(entries: impl IntoIterator<Item = Option<f64>>) -> (f64, usize) {
    let mut total = 0.;
    let mut unknown = 0;
    for entry in entries {
        match entry.filter(|n| n.is_finite() && *n >= 0.) {
            Some(n) => total += n,
            None => unknown += 1,
        }
    }
    (total, unknown)
}
pub struct Probed {
    pub path: PathBuf,
    pub info: TrackInfo,
    pub error: Option<String>,
}
pub struct Reader {
    pub requests: Sender<PathBuf>,
    pub results: Receiver<Probed>,
}
impl Reader {
    pub fn start(ffprobe: String) -> Self {
        let (requests, rx) = mpsc::channel::<PathBuf>();
        let rx = Arc::new(Mutex::new(rx));
        let (tx, results) = mpsc::channel();
        for _ in 0..2 {
            let (rx, tx, ffprobe) = (rx.clone(), tx.clone(), ffprobe.clone());
            thread::spawn(move || {
                loop {
                    let Ok(path) = rx.lock().unwrap().recv() else {
                        break;
                    };
                    let result = probe(&ffprobe, &path);
                    let (info, error) = match result {
                        Ok(info) => (info, None),
                        Err(e) => (
                            TrackInfo::fallback(&path),
                            Some(format!("{}: {e:#}", path.display())),
                        ),
                    };
                    if tx.send(Probed { path, info, error }).is_err() {
                        break;
                    }
                }
            });
        }
        Self { requests, results }
    }
}
fn probe(ffprobe: &str, path: &Path) -> Result<TrackInfo> {
    let mut child = Command::new(ffprobe).args(["-v", "error", "-select_streams", "a:0", "-show_entries", "format=duration,bit_rate:format_tags=title,artist,album,track,tracknumber:stream=codec_name,duration,sample_rate,bit_rate:stream_tags=title,artist,album,track,tracknumber", "-of", "json"])
        .arg(path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().context("Cannot run ffprobe (check config.ffprobe)")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Metadata probe timed out");
            }
        }
    }
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "ffprobe could not read audio metadata"
    );
    Ok(TrackInfo::from_probe(
        path,
        &serde_json::from_slice(&output.stdout)?,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn queue_duration_counts_duplicates_and_reports_unknown_values() {
        assert_eq!(
            total_duration([Some(12.5), None, Some(12.5), Some(0.)]),
            (25., 1)
        );
        assert_eq!(total_duration([Some(f64::NAN), Some(-1.)]), (0., 2));
        assert_eq!(total_duration([]), (0., 0));
    }
    #[test]
    fn opus_stream_tags_and_album_index_are_read_without_playback() {
        let info = TrackInfo::from_probe(
            Path::new("file.ogg"),
            &json!({"format":{"duration":"123.5"},"streams":[{"codec_name":"opus","tags":{"TITLE":"Title","ARTIST":"Artist","ALBUM":"Album","TRACKNUMBER":"03/12"}}]}),
        );
        assert_eq!((info.track, info.duration), (Some(3), Some(123.5)));
        assert_eq!(
            (
                info.title.as_str(),
                info.artist.as_str(),
                info.album.as_str(),
                info.format.as_str()
            ),
            ("Title", "Artist", "Album", "OPUS")
        );
    }
    #[test]
    fn absent_and_partial_metadata_keep_filename_and_empty_fields() {
        let info = TrackInfo::from_probe(
            Path::new("no tags.flac"),
            &json!({"format":{"duration":"NaN","tags":{"title":" ","track":"bad","album":"Album"}},"streams":[{"duration":"10"}]}),
        );
        assert_eq!(info.title, "no tags.flac");
        assert!(info.artist.is_empty());
        assert_eq!(info.album, "Album");
        assert_eq!((info.track, info.duration), (None, Some(10.)));
        assert_eq!(
            TrackInfo::from_probe(Path::new("untagged.mp3"), &json!({})),
            TrackInfo::fallback(Path::new("untagged.mp3"))
        );
    }
    #[test]
    fn container_tags_override_stream_tags_but_empty_values_do_not() {
        let info = TrackInfo::from_probe(
            Path::new("x.flac"),
            &json!({"format":{"tags":{"title":"Container","artist":" "}},"streams":[{"tags":{"title":"Stream","artist":"Artist"}}]}),
        );
        assert_eq!(
            (info.title.as_str(), info.artist.as_str()),
            ("Container", "Artist")
        );
    }
    #[test]
    #[ignore = "requires ffmpeg and ffprobe"]
    fn real_flac_opus_mp3_and_untagged_wav_are_probed() {
        let dir = tempfile::tempdir().unwrap();
        for (extension, codec) in [
            ("flac", "flac"),
            ("opus", "libopus"),
            ("mp3", "libmp3lame"),
            ("m4a", "aac"),
        ] {
            let path = dir.path().join(format!("track.{extension}"));
            let status = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=1.2",
                    "-c:a",
                    codec,
                    "-metadata",
                    "title=Tagged title",
                    "-metadata",
                    "artist=Tagged artist",
                    "-metadata",
                    "album=Tagged album",
                    "-metadata",
                    "track=07/12",
                ])
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
            let info = probe("ffprobe", &path).unwrap();
            assert_eq!(info.track, Some(7), "{extension}");
            assert_eq!(info.title, "Tagged title", "{extension}");
            assert_eq!(info.artist, "Tagged artist", "{extension}");
            assert_eq!(info.album, "Tagged album", "{extension}");
            assert!((1.0..1.5).contains(&info.duration.unwrap()), "{extension}");
            assert!(info.sample_rate.unwrap() >= 44100., "{extension}");
        }
        let path = dir.path().join("no metadata.wav");
        assert!(
            Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-nostdin",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=duration=0.2"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let info = probe("ffprobe", &path).unwrap();
        assert_eq!(info.title, "no metadata.wav");
        assert!(info.artist.is_empty() && info.album.is_empty());
        assert_eq!(info.track, None);
        assert_eq!(info.format, "WAV");
    }
}
