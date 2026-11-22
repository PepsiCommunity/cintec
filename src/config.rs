use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub library: PathBuf,
    pub library_width: f32,
    pub volume: f64,
    pub mpv: String,
    pub ffmpeg: String,
    pub ffprobe: String,
    pub font_family: String,
    /// Interface font size in logical pixels.
    pub font_size: f32,
    /// Command name -> keys. User entries replace that command's defaults.
    pub keys: BTreeMap<String, Vec<String>>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            library: directories::UserDirs::new()
                .and_then(|d| d.audio_dir().map(PathBuf::from))
                .unwrap_or_else(|| {
                    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Music")
                }),
            library_width: 300.,
            volume: 70.,
            mpv: "mpv".into(),
            ffmpeg: "ffmpeg".into(),
            ffprobe: "ffprobe".into(),
            font_family: "sans-serif".into(),
            font_size: 14.,
            keys: BTreeMap::new(),
        }
    }
}
pub fn paths() -> Result<(PathBuf, PathBuf)> {
    let d =
        directories::ProjectDirs::from("", "", "cintec").context("Cannot find XDG directories")?;
    Ok((
        d.config_dir().join("config.toml"),
        d.state_dir()
            .unwrap_or(d.data_local_dir())
            .join("session.json"),
    ))
}
impl Config {
    pub fn load() -> Result<Self> {
        let (path, _) = paths()?;
        let config: Self = if path.exists() {
            toml::from_str(&fs::read_to_string(&path)?)
                .with_context(|| format!("Invalid {}", path.display()))?
        } else {
            Self::default()
        };
        if !config.volume.is_finite() || !(0.0..=100.0).contains(&config.volume) {
            anyhow::bail!("volume must be between 0 and 100");
        }
        if !config.library_width.is_finite() {
            anyhow::bail!("library_width must be finite");
        }
        if config.font_family.trim().is_empty() {
            anyhow::bail!("font_family must not be empty");
        }
        if !config.font_size.is_finite() || !(10.0..=32.0).contains(&config.font_size) {
            anyhow::bail!("font_size must be between 10 and 32 pixels");
        }
        Ok(config)
    }
    pub fn save(&self) -> Result<()> {
        atomic_write(&paths()?.0, toml::to_string_pretty(self)?.as_bytes())
    }
    pub fn bindings(&self) -> BTreeMap<String, Vec<String>> {
        let mut keys: BTreeMap<String, Vec<String>> = [
            ("down", vec!["j", "down"]),
            ("up", vec!["k", "up"]),
            ("expand", vec!["l", "right"]),
            ("collapse", vec!["h", "left"]),
            ("first", vec!["home", "g"]),
            ("last", vec!["end", "shift-g"]),
            ("page_down", vec!["ctrl-d", "pagedown"]),
            ("page_up", vec!["ctrl-u", "pageup"]),
            ("focus", vec!["tab", "shift-tab"]),
            ("visual", vec!["v", "shift-v"]),
            ("enqueue", vec!["a"]),
            ("activate", vec!["enter"]),
            ("toggle", vec!["space"]),
            ("stop", vec!["s"]),
            ("next", vec!["n"]),
            ("previous", vec!["p"]),
            ("seek_back", vec!["alt-left"]),
            ("seek_forward", vec!["alt-right"]),
            ("font_up", vec!["ctrl-=", "ctrl-+"]),
            ("font_down", vec!["ctrl--"]),
            ("font_reset", vec!["ctrl-0"]),
            ("volume_up", vec!["=", "+"]),
            ("volume_down", vec!["-"]),
            ("narrow", vec!["["]),
            ("widen", vec!["]"]),
            ("remove", vec!["d", "delete"]),
            ("move_up", vec!["shift-k"]),
            ("move_down", vec!["shift-j"]),
            ("clear", vec!["shift-d", "ctrl-shift-backspace"]),
            ("repeat", vec!["r"]),
            ("recover", vec!["ctrl-alt-r"]),
            ("rescan", vec!["ctrl-r"]),
            ("help", vec!["f1", "?"]),
            ("dismiss", vec!["escape"]),
            ("quit", vec!["ctrl-q"]),
        ]
        .into_iter()
        .map(|(c, k)| (c.into(), k.into_iter().map(String::from).collect()))
        .collect();
        keys.extend(self.keys.clone());
        keys
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub queue: Vec<PathBuf>,
    pub current: Option<usize>,
    pub position: f64,
    pub detached: Option<PathBuf>,
}
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    use std::io::Write;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
