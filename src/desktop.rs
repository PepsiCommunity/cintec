//! Desktop control and logind sleep notifications stay off the GPUI thread.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread,
    time::Duration,
};
use zbus::{
    blocking::{Connection, Proxy, connection::Builder},
    fdo,
    zvariant::{OwnedObjectPath, OwnedValue, Str},
};

#[derive(Debug)]
pub enum Remote {
    Command(String),
    Seek(f64),
    Position(String, f64),
    Volume(f64),
    Open(PathBuf),
    Sleep(bool, Option<Sender<()>>),
    Warning(String),
}
#[derive(Clone, Default, PartialEq)]
pub struct Snapshot {
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub url: String,
    pub art: String,
    pub track_id: String,
    pub duration: f64,
    pub position: f64,
    pub volume: f64,
    pub repeat: String,
    pub can_next: bool,
    pub can_previous: bool,
}
pub type Shared = Arc<Mutex<Snapshot>>;
fn metadata(s: &Snapshot) -> HashMap<String, OwnedValue> {
    let mut m = HashMap::new();
    if s.track_id.is_empty() {
        return m;
    }
    m.insert(
        "mpris:trackid".into(),
        zbus::zvariant::ObjectPath::try_from(s.track_id.as_str())
            .unwrap()
            .into(),
    );
    m.insert(
        "mpris:length".into(),
        ((s.duration * 1_000_000.) as i64).into(),
    );
    for (key, value) in [
        ("xesam:title", &s.title),
        ("xesam:album", &s.album),
        ("xesam:url", &s.url),
        ("mpris:artUrl", &s.art),
    ] {
        m.insert(key.into(), Str::from(value.as_str()).into());
    }
    m.insert(
        "xesam:artist".into(),
        OwnedValue::try_from(zbus::zvariant::Value::from(vec![s.artist.clone()])).unwrap(),
    );
    m
}
struct Root {
    tx: Sender<Remote>,
}
#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {} // GPUI window remains managed by the compositor.
    fn quit(&self) {
        let _ = self.tx.send(Remote::Command("quit".into()));
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Cintec"
    }
    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "cintec"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&str> {
        vec!["file"]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&str> {
        vec![
            "audio/flac",
            "audio/ogg",
            "audio/opus",
            "audio/mpeg",
            "audio/mp4",
            "audio/aac",
            "audio/x-wav",
            "audio/x-aiff",
        ]
    }
}
struct Player {
    tx: Sender<Remote>,
    state: Shared,
}
impl Player {
    fn command(&self, command: &str) {
        let _ = self.tx.send(Remote::Command(command.into()));
    }
    fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
}
#[zbus::interface(name = "org.mpris.MediaPlayer2.Player", spawn = false)]
impl Player {
    fn next(&self) {
        self.command("next");
    }
    fn previous(&self) {
        self.command("previous");
    }
    fn pause(&self) {
        self.command("pause");
    }
    fn play_pause(&self) {
        self.command("toggle");
    }
    fn stop(&self) {
        self.command("stop");
    }
    fn play(&self) {
        self.command("play");
    }
    fn seek(&self, offset: i64) {
        let _ = self.tx.send(Remote::Seek(offset as f64 / 1_000_000.));
    }
    fn set_position(&self, track_id: OwnedObjectPath, position: i64) {
        if position >= 0 {
            let _ = self.tx.send(Remote::Position(
                track_id.to_string(),
                position as f64 / 1_000_000.,
            ));
        }
    }
    fn open_uri(&self, uri: &str) -> fdo::Result<()> {
        let path = url::Url::parse(uri)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .filter(|p| p.is_file() && crate::library::is_audio(p))
            .ok_or_else(|| fdo::Error::InvalidArgs("Expected a local audio file URI".into()))?;
        let _ = self.tx.send(Remote::Open(path));
        Ok(())
    }
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.snapshot().status
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn loop_status(&self) -> String {
        self.snapshot().repeat
    }
    #[zbus(property)]
    fn set_loop_status(&mut self, value: &str) -> fdo::Result<()> {
        match value {
            "None" | "Track" | "Playlist" => self.command(&format!("repeat:{value}")),
            _ => return Err(fdo::Error::InvalidArgs("Unknown loop status".into())),
        }
        Ok(())
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.
    }
    #[zbus(property)]
    fn set_rate(&mut self, rate: f64) -> fdo::Result<()> {
        if rate == 1. {
            Ok(())
        } else {
            Err(fdo::Error::NotSupported("Only rate 1 is supported".into()))
        }
    }
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn set_shuffle(&mut self, shuffle: bool) -> fdo::Result<()> {
        if shuffle {
            Err(fdo::Error::NotSupported("Shuffle is not supported".into()))
        } else {
            Ok(())
        }
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.snapshot())
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn volume(&self) -> f64 {
        self.snapshot().volume / 100.
    }
    #[zbus(property)]
    fn set_volume(&mut self, value: f64) -> fdo::Result<()> {
        if !value.is_finite() {
            return Err(fdo::Error::InvalidArgs("Volume must be finite".into()));
        }
        let _ = self.tx.send(Remote::Volume((value * 100.).clamp(0., 100.)));
        Ok(())
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        (self.snapshot().position * 1_000_000.) as i64
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.snapshot().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.snapshot().can_previous
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        !self.snapshot().track_id.is_empty()
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.snapshot().status != "Stopped"
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        let s = self.snapshot();
        s.duration > 0. && s.status != "Stopped"
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}
pub struct Desktop {
    pub state: Shared,
    pub events: Receiver<Remote>,
    pub seeked: Sender<i64>,
}
impl Desktop {
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(Snapshot {
            status: "Stopped".into(),
            repeat: "None".into(),
            ..Snapshot::default()
        }));
        let (tx, events) = channel();
        let (seeked, seeks) = channel();
        // MPRIS and logind are Linux integrations, not native macOS services.
        if cfg!(target_os = "linux") {
            let shared = state.clone();
            let mpris_tx = tx.clone();
            thread::spawn(move || {
                let result = (|| -> anyhow::Result<()> {
                    let conn = Builder::session()?
                        .name("org.mpris.MediaPlayer2.cintec")?
                        .serve_at(
                            "/org/mpris/MediaPlayer2",
                            Root {
                                tx: mpris_tx.clone(),
                            },
                        )?
                        .serve_at(
                            "/org/mpris/MediaPlayer2",
                            Player {
                                tx: mpris_tx.clone(),
                                state: shared.clone(),
                            },
                        )?
                        .build()?;
                    let mut previous = Snapshot::default();
                    loop {
                        thread::sleep(Duration::from_millis(250));
                        let s = shared.lock().unwrap().clone();
                        // Position advances silently, as required by MPRIS. Seeked is emitted separately.
                        let mut changed = HashMap::<String, OwnedValue>::new();
                        if s.status != previous.status {
                            changed.insert(
                                "PlaybackStatus".into(),
                                Str::from(s.status.as_str()).into(),
                            );
                        }
                        if s.track_id != previous.track_id
                            || s.title != previous.title
                            || s.artist != previous.artist
                            || s.album != previous.album
                            || s.art != previous.art
                            || s.duration != previous.duration
                        {
                            changed.insert(
                                "Metadata".into(),
                                OwnedValue::try_from(zbus::zvariant::Value::from(metadata(&s)))?,
                            );
                        }
                        if s.volume != previous.volume {
                            changed.insert("Volume".into(), (s.volume / 100.).into());
                        }
                        if s.repeat != previous.repeat {
                            changed
                                .insert("LoopStatus".into(), Str::from(s.repeat.as_str()).into());
                        }
                        if s.can_next != previous.can_next {
                            changed.insert("CanGoNext".into(), s.can_next.into());
                        }
                        if s.can_previous != previous.can_previous {
                            changed.insert("CanGoPrevious".into(), s.can_previous.into());
                        }
                        if s.track_id != previous.track_id
                            || s.duration != previous.duration
                            || s.status != previous.status
                        {
                            changed.insert("CanPlay".into(), (!s.track_id.is_empty()).into());
                            changed.insert("CanPause".into(), (s.status != "Stopped").into());
                            changed.insert(
                                "CanSeek".into(),
                                (s.duration > 0. && s.status != "Stopped").into(),
                            );
                        }
                        if !changed.is_empty() {
                            conn.emit_signal(
                                None::<&str>,
                                "/org/mpris/MediaPlayer2",
                                "org.freedesktop.DBus.Properties",
                                "PropertiesChanged",
                                &(
                                    "org.mpris.MediaPlayer2.Player",
                                    changed,
                                    Vec::<String>::new(),
                                ),
                            )?;
                        }
                        for position in seeks.try_iter() {
                            conn.emit_signal(
                                None::<&str>,
                                "/org/mpris/MediaPlayer2",
                                "org.mpris.MediaPlayer2.Player",
                                "Seeked",
                                &(position,),
                            )?;
                        }
                        previous = s;
                    }
                })();
                if let Err(e) = result {
                    let _ = mpris_tx.send(Remote::Warning(format!("MPRIS unavailable: {e}")));
                }
            });
            thread::spawn(move || {
                let result = (|| -> anyhow::Result<()> {
                    let conn = Connection::system()?;
                    let proxy = Proxy::new(
                        &conn,
                        "org.freedesktop.login1",
                        "/org/freedesktop/login1",
                        "org.freedesktop.login1.Manager",
                    )?;
                    let inhibit = || -> zbus::Result<zbus::zvariant::OwnedFd> {
                        proxy.call(
                            "Inhibit",
                            &(
                                "sleep",
                                "Cintec",
                                "Save playback and close the audio device",
                                "delay",
                            ),
                        )
                    };
                    let mut signals = proxy.receive_signal("PrepareForSleep")?;
                    let mut inhibitor = inhibit().ok();
                    for message in &mut signals {
                        let (sleeping,): (bool,) = message.body().deserialize()?;
                        if sleeping {
                            let (ack, done) = channel();
                            if tx.send(Remote::Sleep(true, Some(ack))).is_err() {
                                break;
                            }
                            let _ = done.recv_timeout(Duration::from_secs(2));
                            inhibitor.take();
                        } else {
                            // Give PipeWire/PulseAudio and Bluetooth endpoints time to reappear.
                            thread::sleep(Duration::from_secs(1));
                            if tx.send(Remote::Sleep(false, None)).is_err() {
                                break;
                            }
                            inhibitor = inhibit().ok();
                        }
                    }
                    drop(inhibitor);
                    Ok(())
                })();
                if let Err(e) = result {
                    let _ = tx.send(Remote::Warning(format!(
                        "Suspend monitoring unavailable: {e}; use Ctrl-Alt-R to recover audio"
                    )));
                }
            });
        }
        Self {
            state,
            events,
            seeked,
        }
    }
}
