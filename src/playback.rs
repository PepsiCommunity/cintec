use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum Event {
    Property(String, Value),
    Loaded,
    AudioReconfigured,
    End(String),
    Cover(PathBuf, Option<PathBuf>),
    Error(String),
}
pub struct Backend {
    child: Child,
    socket: UnixStream,
    pub events: Receiver<Event>,
    _dir: tempfile::TempDir,
    ffmpeg: String,
    cover_tx: Sender<Event>,
}
impl Backend {
    pub fn start(mpv: &str, ffmpeg: String) -> Result<Self> {
        let dir = tempfile::Builder::new().prefix("cintec-").tempdir()?;
        let path = dir.path().join("mpv.sock");
        let mut child = Command::new(mpv)
            .args([
                "--idle=yes",
                "--no-video",
                "--no-config",
                "--load-scripts=no",
                "--input-terminal=no",
                "--audio-display=no",
                "--terminal=no",
            ])
            .arg(format!("--input-ipc-server={}", path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("Cannot start mpv; install it or enter nix develop")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let socket = loop {
            match UnixStream::connect(&path) {
                Ok(s) => break s,
                Err(e) => {
                    if let Some(status) = child.try_wait()? {
                        anyhow::bail!("mpv exited: {status}");
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(e).context("mpv IPC timed out");
                    }
                    thread::sleep(Duration::from_millis(25));
                }
            }
        };
        let (tx, events) = mpsc::channel();
        let reader = socket.try_clone()?;
        let cover_tx = tx.clone();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match v["event"].as_str() {
                    Some("property-change") => {
                        let name = v["name"].as_str().unwrap_or_default().to_owned();
                        let _ = tx.send(Event::Property(name, v["data"].clone()));
                    }
                    Some("audio-reconfig") => {
                        let _ = tx.send(Event::AudioReconfigured);
                    }
                    Some("file-loaded") => {
                        let _ = tx.send(Event::Loaded);
                    }
                    Some("end-file") => {
                        let _ =
                            tx.send(Event::End(v["reason"].as_str().unwrap_or_default().into()));
                    }
                    _ => {
                        if let Some(name) = v["request_id"]
                            .as_str()
                            .filter(|name| ["audio-params", "audio-bitrate"].contains(name))
                        {
                            if v["error"] == "success" {
                                let _ = tx.send(Event::Property(name.into(), v["data"].clone()));
                            }
                        }
                        if v["error"]
                            .as_str()
                            .is_some_and(|e| e != "success" && e != "property unavailable")
                        {
                            let _ = tx.send(Event::Error(format!("mpv: {}", v["error"])));
                        }
                    }
                }
            }
            let _ = tx.send(Event::Error("mpv disconnected".into()));
        });
        let mut backend = Self {
            child,
            socket,
            events,
            _dir: dir,
            ffmpeg,
            cover_tx,
        };
        for (i, prop) in [
            "time-pos",
            "duration",
            "pause",
            "idle-active",
            "metadata",
            "audio-params",
            "audio-bitrate",
            "volume",
            "path",
        ]
        .iter()
        .enumerate()
        {
            backend.send(json!(["observe_property", i, prop]))?;
        }
        Ok(backend)
    }
    pub fn send(&mut self, command: Value) -> Result<()> {
        writeln!(self.socket, "{}", json!({"command":command}))?;
        Ok(())
    }
    pub fn refresh_cover(&self, path: &std::path::Path) {
        extract_cover(
            path.into(),
            self._dir.path().into(),
            self.ffmpeg.clone(),
            self.cover_tx.clone(),
        );
    }
    pub fn refresh_audio_info(&mut self) -> Result<()> {
        // Observers do not emit another change if the next track has identical
        // audio parameters. Fetch explicitly after load/reconfiguration.
        for name in ["audio-params", "audio-bitrate"] {
            writeln!(
                self.socket,
                "{}",
                json!({"command":["get_property",name],"request_id":name})
            )?;
        }
        Ok(())
    }
    pub fn load(&mut self, path: &std::path::Path) -> Result<()> {
        let path = path.to_str().context("mpv requires a UTF-8 path")?;
        self.send(json!(["loadfile", path, "replace"]))?;
        self.send(json!(["set_property", "pause", false]))
    }
}
impl Drop for Backend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn extract_cover(path: PathBuf, dir: PathBuf, ffmpeg: String, tx: Sender<Event>) {
    thread::spawn(move || {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hash);
        let target = dir.join(format!("{:x}.jpg", hash.finish()));
        let result = Command::new(ffmpeg)
            .args(["-v", "error", "-nostdin", "-y", "-i"])
            .arg(&path)
            .args([
                "-an",
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                "scale=512:512:force_original_aspect_ratio=decrease",
            ])
            .arg(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let success = if let Ok(mut child) = result {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.success(),
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(50))
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break false;
                    }
                }
            }
        } else {
            false
        };
        let cover = if success && fs::metadata(&target).is_ok() {
            Some(target)
        } else {
            folder_cover(&path)
        };
        let _ = tx.send(Event::Cover(path, cover));
    });
}

pub fn positive_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite() && *n > 0.)
}
fn folder_cover(track: &std::path::Path) -> Option<PathBuf> {
    let parent = track.parent()?;
    let mut candidates: Vec<_> = fs::read_dir(parent)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                    s.eq_ignore_ascii_case("cover") || s.eq_ignore_ascii_case("folder")
                })
                && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                    [
                        "jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "avif",
                    ]
                    .contains(&e.to_ascii_lowercase().as_str())
                })
        })
        .collect();
    candidates.sort_by_key(|p| {
        (
            if p.file_stem()
                .unwrap()
                .to_string_lossy()
                .eq_ignore_ascii_case("cover")
            {
                0
            } else {
                1
            },
            p.clone(),
        )
    });
    candidates.into_iter().next()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_rate_accepts_integer_float_and_probe_strings() {
        for v in [json!(44100), json!(48000.0), json!("96000")] {
            assert!(positive_number(&v).unwrap() >= 44100.);
        }
        for v in [Value::Null, json!(0), json!("NaN"), json!(-1)] {
            assert_eq!(positive_number(&v), None);
        }
    }
    #[test]
    fn folder_art_is_case_insensitive_with_cover_precedence() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("FOLDER.WebP"), b"image").unwrap();
        let track = dir.path().join("track.flac");
        assert_eq!(folder_cover(&track), Some(dir.path().join("FOLDER.WebP")));
        fs::write(dir.path().join("Cover.JPEG"), b"image").unwrap();
        assert_eq!(folder_cover(&track), Some(dir.path().join("Cover.JPEG")));
        fs::remove_file(dir.path().join("Cover.JPEG")).unwrap();
        fs::remove_file(dir.path().join("FOLDER.WebP")).unwrap();
        fs::write(dir.path().join("cover.txt"), b"not artwork").unwrap();
        assert_eq!(folder_cover(&track), None);
    }
}
