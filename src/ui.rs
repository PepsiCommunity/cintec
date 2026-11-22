use crate::selection;
use crate::{
    config::{self, Config, Session},
    desktop::{Desktop, Remote, Snapshot},
    library::{self, Node},
    playback::{self, Backend, Event},
};
use crate::{
    icons::icon,
    metadata::{Reader, TrackInfo},
    recovery::Recovery,
};
use gpui::{prelude::*, *};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const BG: u32 = 0x101318;
const PANEL: u32 = 0x171c23;
const LINE: u32 = 0x29313b;
const MUTED: u32 = 0x92a0b2;
const TEXT: u32 = 0xe4eaf1;
const ACCENT: u32 = 0x8cdbb5;
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
pub struct RunCommand {
    pub command: String,
}
#[derive(Clone)]
struct Entry {
    id: u64,
    path: PathBuf,
}
pub struct Player {
    config: Config,
    focus: FocusHandle,
    library_focus: bool,
    tree: Option<Node>,
    expanded: HashSet<PathBuf>,
    library_cursor: usize,
    queue_cursor: usize,
    library_anchor: Option<PathBuf>,
    queue_anchor: Option<u64>,
    library_scroll: UniformListScrollHandle,
    queue_scroll: UniformListScrollHandle,
    scan: Option<Receiver<(Option<Node>, Vec<String>)>>,
    queue: Vec<Entry>,
    metadata: HashMap<PathBuf, TrackInfo>,
    metadata_reader: Reader,
    next_id: u64,
    current: Option<usize>,
    detached: Option<Entry>,
    backend: Option<Backend>,
    desktop: Desktop,
    playing: bool,
    loaded: bool,
    position: f64,
    duration: f64,
    title: String,
    artist: String,
    album: String,
    sample_rate: f64,
    bitrate: f64,
    cover: Option<PathBuf>,
    pending_seek: Option<f64>,
    repeat: String,
    notice: String,
    help: bool,
    last_save: Instant,
    sleeping: bool,
    recovery: Option<Recovery>,
    resume_guard: Option<(Recovery, Instant)>,
}
impl Player {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        let session = config::paths()
            .ok()
            .and_then(|(_, p)| fs::read(p).ok())
            .and_then(|s| serde_json::from_slice::<Session>(&s).ok())
            .unwrap_or_default();
        let queue: Vec<_> = session
            .queue
            .into_iter()
            .enumerate()
            .map(|(i, path)| Entry {
                id: i as u64 + 1,
                path,
            })
            .collect();
        let current = session.current.filter(|&i| i < queue.len());
        let detached = session.detached.map(|path| Entry {
            id: queue.len() as u64 + 1,
            path,
        });
        let metadata_reader = Reader::start(config.ffprobe.clone());
        let mut player = Self {
            config,
            focus,
            library_focus: true,
            tree: None,
            expanded: HashSet::new(),
            library_cursor: 0,
            queue_cursor: 0,
            library_anchor: None,
            queue_anchor: None,
            library_scroll: UniformListScrollHandle::new(),
            queue_scroll: UniformListScrollHandle::new(),
            scan: None,
            next_id: queue.len() as u64 + 2,
            queue,
            metadata: HashMap::new(),
            metadata_reader,
            current: if detached.is_some() { None } else { current },
            detached,
            backend: None,
            desktop: Desktop::start(),
            playing: false,
            loaded: false,
            position: session.position.max(0.),
            duration: 0.,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            sample_rate: 0.,
            bitrate: 0.,
            cover: None,
            pending_seek: None,
            repeat: "None".into(),
            notice: "Ready · a adds the selected directory recursively · F1 for keys".into(),
            help: false,
            last_save: Instant::now(),
            sleeping: false,
            recovery: None,
            resume_guard: None,
        };
        player.schedule_metadata();
        player.refresh_title();
        player.scan();
        if let Err(e) = player.ensure_backend() {
            player.notice = format!("{e:#} · Ctrl-Alt-R retries audio");
        }
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.tick(cx);
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        player
    }
    fn ensure_backend(&mut self) -> anyhow::Result<()> {
        if self.backend.is_none() {
            let mut backend = Backend::start(&self.config.mpv, self.config.ffmpeg.clone())?;
            backend.send(json!(["set_property", "volume", self.config.volume]))?;
            self.backend = Some(backend);
        }
        Ok(())
    }
    fn send(&mut self, value: serde_json::Value) {
        if let Some(backend) = &mut self.backend {
            if let Err(e) = backend.send(value) {
                self.notice = format!("Audio error: {e}. Ctrl-Alt-R to reconnect");
            }
        }
    }
    fn scan(&mut self) {
        let path = self.config.library.clone();
        let (tx, rx) = mpsc::channel();
        self.scan = Some(rx);
        self.notice = format!("Scanning {}…", path.display());
        std::thread::spawn(move || {
            let mut warnings = vec![];
            let root = Node::scan(&path, &mut warnings);
            let _ = tx.send((root, warnings));
        });
    }
    fn schedule_metadata(&mut self) {
        for entry in self.queue.iter().chain(self.detached.iter()) {
            if !self.metadata.contains_key(&entry.path) {
                self.metadata
                    .insert(entry.path.clone(), TrackInfo::fallback(&entry.path));
                let _ = self.metadata_reader.requests.send(entry.path.clone());
            }
        }
    }
    fn active_entry(&self) -> Option<&Entry> {
        self.detached
            .as_ref()
            .or_else(|| self.current.and_then(|i| self.queue.get(i)))
    }
    fn queue_summary(&self) -> String {
        let (total, unknown) = crate::metadata::total_duration(
            self.queue
                .iter()
                .map(|e| self.metadata.get(&e.path).and_then(|m| m.duration)),
        );
        if unknown == 0 {
            format!("{} tracks · {} total", self.queue.len(), clock(total))
        } else {
            format!(
                "{} tracks · {}+ total ({unknown} unknown)",
                self.queue.len(),
                clock(total)
            )
        }
    }
    fn refresh_title(&mut self) {
        let info = self.active_entry().and_then(|e| self.metadata.get(&e.path));
        self.title = info
            .map(|m| m.title.clone())
            .unwrap_or_else(|| "Nothing playing".into());
        self.artist = info.map(|m| m.artist.clone()).unwrap_or_default();
        self.album = info.map(|m| m.album.clone()).unwrap_or_default();
    }
    fn save(&mut self) {
        let session = Session {
            queue: self.queue.iter().map(|e| e.path.clone()).collect(),
            current: self.current,
            position: self.position,
            detached: self.detached.as_ref().map(|e| e.path.clone()),
        };
        let result = (|| -> anyhow::Result<()> {
            config::atomic_write(&config::paths()?.1, &serde_json::to_vec_pretty(&session)?)?;
            self.config.save()
        })();
        if let Err(e) = result {
            self.notice = format!("Cannot save session: {e}");
        }
        self.last_save = Instant::now();
    }
    fn play_at(&mut self, index: usize, position: f64, playing: bool) {
        if self.sleeping || index >= self.queue.len() {
            return;
        }
        self.current = Some(index);
        self.detached = None;
        if self.queue_anchor.is_none() {
            self.queue_cursor = index;
            self.queue_scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
        self.load_active(position, playing);
    }
    fn load_active(&mut self, position: f64, playing: bool) {
        if self.sleeping {
            return;
        }
        let Some(path) = self.active_entry().map(|e| e.path.clone()) else {
            return;
        };
        self.recovery = None;
        self.resume_guard = None;
        self.position = position;
        self.duration = 0.;
        self.cover = None;
        self.artist.clear();
        self.album.clear();
        self.bitrate = 0.;
        self.sample_rate = 0.;
        self.refresh_title();
        if let Some(info) = self.metadata.get(&path) {
            self.sample_rate = info.sample_rate.unwrap_or(0.);
            self.bitrate = info.bitrate.unwrap_or(0.);
        }
        if let Err(e) = self
            .ensure_backend()
            .and_then(|_| self.backend.as_mut().unwrap().load(&path))
        {
            self.notice = format!("Cannot play: {e:#}");
            self.playing = false;
            self.loaded = false;
            return;
        }
        self.loaded = true;
        self.playing = playing;
        self.pending_seek = Some(position);
        self.send(json!(["set_property", "pause", !playing]));
        self.notice = "Playing · Keymaps help on F1".into();
    }
    fn stop(&mut self) {
        self.recovery = None;
        self.resume_guard = None;
        self.send(json!(["stop"]));
        self.loaded = false;
        self.playing = false;
        self.position = 0.;
        self.pending_seek = None;
    }
    fn seek(&mut self, position: f64) {
        if !self.loaded || !position.is_finite() {
            return;
        }
        let position = position.max(0.).min(if self.duration > 0. {
            self.duration
        } else {
            f64::MAX
        });
        self.send(json!(["seek", position, "absolute+exact"]));
        self.position = position;
        let _ = self.desktop.seeked.send((position * 1_000_000.) as i64);
    }
    fn enqueue(&mut self, play: bool) {
        let mut paths = Vec::new();
        if let Some(tree) = &self.tree {
            let mut rows = vec![];
            tree.visible_contents(&self.expanded, &mut rows);
            let anchor = self
                .library_anchor
                .as_ref()
                .and_then(|path| rows.iter().position(|(node, _)| &node.path == path));
            paths = library::selected_tracks(
                rows[selection::span(anchor, self.library_cursor, rows.len())]
                    .iter()
                    .map(|(node, _)| *node),
            );
        }
        self.library_anchor = None;
        let count = paths.len();
        let start = self.queue.len();
        for path in paths {
            self.queue.push(Entry {
                id: self.next_id,
                path,
            });
            self.next_id += 1;
        }
        self.schedule_metadata();
        self.notice = format!("Added {count} tracks recursively");
        if play && count > 0 {
            self.play_at(start, 0., true);
        }
        if self.active_entry().is_none() && !self.queue.is_empty() {
            self.current = Some(0);
            self.refresh_title();
        }
    }
    fn cancel_visual(&mut self) {
        self.library_anchor = None;
        self.queue_anchor = None;
    }
    fn toggle_visual(&mut self) {
        if self.library_focus {
            if self.library_anchor.is_some() {
                self.library_anchor = None;
            } else if let Some(tree) = &self.tree {
                let mut rows = vec![];
                tree.visible_contents(&self.expanded, &mut rows);
                self.library_anchor = rows
                    .get(self.library_cursor)
                    .map(|(node, _)| node.path.clone());
            }
        } else if self.queue_anchor.is_some() {
            self.queue_anchor = None;
        } else {
            self.queue_anchor = self.queue.get(self.queue_cursor).map(|e| e.id);
        }
    }
    fn queue_selection(&self) -> std::ops::Range<usize> {
        let anchor = self
            .queue_anchor
            .and_then(|id| self.queue.iter().position(|e| e.id == id));
        selection::span(anchor, self.queue_cursor, self.queue.len())
    }
    fn mode_status(&self) -> String {
        if self.library_focus && self.library_anchor.is_some() {
            let mut rows = vec![];
            if let Some(tree) = &self.tree {
                tree.visible_contents(&self.expanded, &mut rows);
            }
            let anchor = self
                .library_anchor
                .as_ref()
                .and_then(|p| rows.iter().position(|(n, _)| &n.path == p));
            format!(
                "VISUAL · Library · {} selected · a Add · Esc Cancel",
                selection::span(anchor, self.library_cursor, rows.len()).len()
            )
        } else if !self.library_focus && self.queue_anchor.is_some() {
            format!(
                "VISUAL · Queue · {} selected · d Delete · Shift-J/K Move · Esc Cancel",
                self.queue_selection().len()
            )
        } else {
            self.notice.clone()
        }
    }
    fn navigate(&mut self, delta: isize) {
        let count = if self.library_focus {
            self.visible_count()
        } else {
            self.queue.len()
        };
        let cursor = if self.library_focus {
            &mut self.library_cursor
        } else {
            &mut self.queue_cursor
        };
        *cursor = cursor
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        if count > 0 {
            if self.library_focus {
                self.library_scroll
                    .scroll_to_item(*cursor, ScrollStrategy::Top);
            } else {
                self.queue_scroll
                    .scroll_to_item(*cursor, ScrollStrategy::Top);
            }
        }
    }
    fn visible_count(&self) -> usize {
        let mut rows = vec![];
        if let Some(t) = &self.tree {
            t.visible_contents(&self.expanded, &mut rows);
        }
        rows.len()
    }
    fn branch(&mut self, expand: bool) {
        if !self.library_focus {
            return;
        }
        let mut rows = vec![];
        if let Some(tree) = &self.tree {
            tree.visible_contents(&self.expanded, &mut rows);
        }
        if let Some((node, _)) = rows.get(self.library_cursor) {
            if expand {
                if node.directory && !self.expanded.insert(node.path.clone()) {
                    self.navigate(1);
                }
            } else if !self.expanded.remove(&node.path) {
                if let Some(parent) = node.path.parent() {
                    if let Some(i) = rows.iter().position(|(n, _)| n.path == parent) {
                        self.library_cursor = i;
                        self.library_scroll.scroll_to_item(i, ScrollStrategy::Top);
                    }
                }
            }
        }
        if let (Some(anchor), Some(tree)) = (&self.library_anchor, &self.tree) {
            let mut rows = vec![];
            tree.visible_contents(&self.expanded, &mut rows);
            self.library_anchor = rows
                .iter()
                .rev()
                .find(|(n, _)| anchor.starts_with(&n.path))
                .map(|(n, _)| n.path.clone());
        }
    }
    fn recover(&mut self) {
        if self.sleeping {
            return;
        }
        let recovery = Recovery::capture(self.position, self.playing, self.loaded);
        self.backend.take();
        self.loaded = false;
        self.recovery = Some(recovery);
        self.notice = "Reconnecting the audio backend…".into();
    }
    fn remove_queue_range(&mut self, range: std::ops::Range<usize>) {
        let count = range.len();
        let (edit, detached) =
            selection::remove_preserving_current(&mut self.queue, range, self.current, self.loaded);
        self.queue_anchor = None;
        self.queue_cursor = edit.cursor;
        if detached.is_some() {
            self.detached = detached;
            self.current = None;
        } else {
            self.current = edit.current;
            if self.detached.is_none() {
                if edit.removed_current {
                    self.cover = None;
                    self.duration = 0.;
                    self.sample_rate = 0.;
                    self.bitrate = 0.;
                }
                self.refresh_title();
            }
        }
        self.queue_scroll
            .scroll_to_item(self.queue_cursor, ScrollStrategy::Top);
        self.notice = format!("Removed {count} tracks from queue");
    }
    fn command(&mut self, command: &str, cx: &mut Context<Self>) {
        match command {
            "quit" => {
                self.save();
                cx.quit();
                return;
            }
            "font_up" | "font_down" | "font_reset" => {
                self.config.font_size = match command {
                    "font_up" => (self.config.font_size + 1.).min(32.),
                    "font_down" => (self.config.font_size - 1.).max(10.),
                    _ => 14.,
                };
                self.notice = format!("Font size: {:.0} px", self.config.font_size);
                self.library_scroll
                    .scroll_to_item(self.library_cursor, ScrollStrategy::Top);
                self.queue_scroll
                    .scroll_to_item(self.queue_cursor, ScrollStrategy::Top);
            }
            "help" => self.help = !self.help,
            "dismiss" => {
                if self.help {
                    self.help = false;
                } else {
                    self.cancel_visual();
                }
            }
            "visual" => self.toggle_visual(),
            "focus" => {
                self.cancel_visual();
                self.library_focus = !self.library_focus;
            }
            "down" => self.navigate(1),
            "up" => self.navigate(-1),
            "page_down" => self.navigate(12),
            "page_up" => self.navigate(-12),
            "first" => self.navigate(isize::MIN),
            "last" => self.navigate(isize::MAX),
            "expand" => self.branch(true),
            "collapse" => self.branch(false),
            "enqueue" if self.library_focus => self.enqueue(false),
            "activate" => {
                if self.library_focus {
                    self.enqueue(true);
                } else {
                    self.queue_anchor = None;
                    self.play_at(self.queue_cursor, 0., true);
                }
            }
            "toggle" | "play" | "pause" => {
                let playing = match command {
                    "play" => true,
                    "pause" => false,
                    _ => !self.playing,
                };
                if !self.loaded && playing && self.detached.is_some() {
                    self.load_active(self.position, true);
                } else if !self.loaded && playing {
                    if let Some(i) =
                        self.current
                            .or(if self.queue.is_empty() { None } else { Some(0) })
                    {
                        self.play_at(i, self.position, true);
                    }
                } else {
                    self.playing = playing && self.loaded;
                    self.send(json!(["set_property", "pause", !self.playing]));
                }
            }
            "stop" => self.stop(),
            "next" => {
                if self.detached.is_some() {
                    if self.queue.is_empty() {
                        self.stop();
                    } else {
                        self.play_at(0, 0., true);
                    }
                } else if let Some(i) = self.current {
                    if i + 1 < self.queue.len() {
                        self.play_at(i + 1, 0., true);
                    } else if self.repeat == "Playlist" && !self.queue.is_empty() {
                        self.play_at(0, 0., true);
                    } else {
                        self.stop();
                    }
                }
            }
            "previous" => {
                if self.position > 3. {
                    self.seek(0.);
                } else if self.detached.is_some() {
                    self.load_active(0., true);
                } else if let Some(i) = self.current {
                    self.play_at(i.saturating_sub(1), 0., true);
                }
            }
            "seek_back" => self.seek(self.position - 5.),
            "seek_forward" => self.seek(self.position + 5.),
            "volume_up" | "volume_down" => {
                self.config.volume = (self.config.volume
                    + if command == "volume_up" { 5. } else { -5. })
                .clamp(0., 100.);
                self.send(json!(["set_property", "volume", self.config.volume]));
            }
            "narrow" => self.config.library_width = (self.config.library_width - 24.).max(160.),
            "widen" => self.config.library_width = (self.config.library_width + 24.).min(520.),
            "clear" => {
                self.cancel_visual();
                self.remove_queue_range(0..self.queue.len());
            }
            "remove" if !self.library_focus && self.queue_cursor < self.queue.len() => {
                self.remove_queue_range(self.queue_selection());
            }
            "move_up" | "move_down"
                if !self.library_focus && self.queue_cursor < self.queue.len() =>
            {
                let current_id = self.current.and_then(|i| self.queue.get(i)).map(|e| e.id);
                let range = self.queue_selection();
                let moved =
                    selection::move_block(&mut self.queue, range.clone(), command == "move_down");
                self.queue_cursor = self
                    .queue_cursor
                    .saturating_add_signed(moved.start as isize - range.start as isize);
                self.current = current_id.and_then(|id| self.queue.iter().position(|e| e.id == id));
                self.queue_scroll
                    .scroll_to_item(self.queue_cursor, ScrollStrategy::Top);
            }
            "repeat" => {
                self.repeat = match self.repeat.as_str() {
                    "None" => "Playlist",
                    "Playlist" => "Track",
                    _ => "None",
                }
                .into()
            }
            "rescan" => {
                self.library_anchor = None;
                self.metadata.clear();
                self.schedule_metadata();
                self.scan();
            }
            "recover" => self.recover(),
            _ if command.starts_with("repeat:") => self.repeat = command[7..].into(),
            _ => {}
        }
        if !matches!(
            command,
            "down"
                | "up"
                | "expand"
                | "collapse"
                | "first"
                | "last"
                | "page_down"
                | "page_up"
                | "focus"
                | "help"
                | "dismiss"
                | "visual"
        ) {
            self.save();
        }
        cx.notify();
    }
    fn tick(&mut self, cx: &mut Context<Self>) {
        for result in self.metadata_reader.results.try_iter() {
            if let Some(error) = result.error {
                eprintln!("Metadata: {error}");
            }
            let current = self.active_entry().is_some_and(|e| e.path == result.path);
            if current {
                self.title = result.info.title.clone();
                self.artist = result.info.artist.clone();
                self.album = result.info.album.clone();
                if let Some(rate) = result.info.sample_rate {
                    self.sample_rate = rate;
                }
                if let Some(rate) = result.info.bitrate {
                    self.bitrate = rate;
                }
                if self.duration == 0. {
                    self.duration = result.info.duration.unwrap_or(0.);
                }
            }
            self.metadata.insert(result.path, result.info);
        }

        if let Some(result) = self.scan.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.scan = None;
            self.library_anchor = None;
            self.tree = result.0;
            self.expanded.insert(self.config.library.clone());
            self.library_cursor = self
                .library_cursor
                .min(self.visible_count().saturating_sub(1));
            let mut tracks = vec![];
            if let Some(t) = &self.tree {
                t.tracks(&mut tracks);
            }
            self.notice = format!(
                "{} tracks in library{}",
                tracks.len(),
                result
                    .1
                    .first()
                    .map(|s| format!(" · {} scan warnings: {s}", result.1.len()))
                    .unwrap_or_default()
            );
        }
        let events: Vec<_> = self.desktop.events.try_iter().collect();
        for event in events {
            match event {
                Remote::Command(c) => self.command(&c, cx),
                Remote::Seek(offset) => self.seek(self.position + offset),
                Remote::Position(id, pos) => {
                    if self.track_id() == id && pos <= self.duration {
                        self.seek(pos);
                    }
                }
                Remote::Volume(v) => {
                    self.config.volume = v;
                    self.send(json!(["set_property", "volume", v]));
                }
                Remote::Open(path) => {
                    let ix = self.queue.len();
                    self.queue.push(Entry {
                        id: self.next_id,
                        path,
                    });
                    self.next_id += 1;
                    self.schedule_metadata();
                    self.play_at(ix, 0., true);
                }
                Remote::Warning(w) => {
                    eprintln!("{w}");
                    self.notice = w;
                }
                Remote::Sleep(sleeping, ack) => {
                    if sleeping {
                        self.sleeping = true;
                        if self.recovery.is_none() {
                            self.recovery =
                                Some(Recovery::capture(self.position, self.playing, self.loaded));
                        }
                        self.resume_guard = None;
                        self.save();
                        self.backend.take();
                        self.loaded = false;
                        self.notice = "Suspended · playback saved and audio device released".into();
                        if let Some(ack) = ack {
                            let _ = ack.send(());
                        }
                    } else {
                        self.sleeping = false;
                        if let Some(r) = &mut self.recovery {
                            r.due = Instant::now();
                        }
                        self.notice = "Resumed · reconnecting audio…".into();
                    }
                }
            }
        }
        if !self.sleeping
            && self
                .recovery
                .as_ref()
                .is_some_and(|r| Instant::now() >= r.due)
        {
            let mut recovery = self.recovery.take().unwrap();
            match self.ensure_backend() {
                Ok(()) => {
                    if recovery.loaded {
                        self.load_active(recovery.position, recovery.playing);
                        self.resume_guard =
                            Some((recovery, Instant::now() + Duration::from_secs(10)));
                    }
                    self.notice = "Audio reconnected".into();
                }
                Err(e) => {
                    recovery.retry();
                    self.notice =
                        format!("Waiting for audio (attempt {}): {e:#}", recovery.attempts);
                    self.recovery = Some(recovery);
                }
            }
        }
        let events: Vec<_> = self
            .backend
            .as_ref()
            .map(|b| b.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                Event::Property(name, data) => match name.as_str() {
                    "time-pos" => {
                        if let Some(p) = data.as_f64() {
                            self.position = p;
                        }
                    }
                    "duration" => self.duration = data.as_f64().unwrap_or(0.),
                    "pause" => {
                        if self.loaded {
                            self.playing = !data.as_bool().unwrap_or(true);
                        }
                    }
                    "volume" => {
                        if let Some(v) = data.as_f64() {
                            self.config.volume = v.clamp(0., 100.);
                        }
                    }
                    "audio-bitrate" => {
                        if let Some(rate) = playback::positive_number(&data) {
                            self.bitrate = rate;
                        }
                    }
                    "audio-params" => {
                        if let Some(rate) = playback::positive_number(&data["samplerate"]) {
                            self.sample_rate = rate;
                        }
                    }
                    "metadata" => {
                        for (key, target) in [
                            ("title", &mut self.title),
                            ("artist", &mut self.artist),
                            ("album", &mut self.album),
                        ] {
                            if let Some(obj) = data.as_object()
                                && let Some(v) = obj
                                    .iter()
                                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                                    .and_then(|(_, v)| v.as_str())
                            {
                                *target = v.into();
                            }
                        }
                    }
                    _ => {}
                },
                Event::AudioReconfigured => {
                    if let Some(b) = &mut self.backend {
                        let _ = b.refresh_audio_info();
                    }
                }
                Event::Loaded => {
                    if let (Some(path), Some(b)) = (
                        self.active_entry().map(|e| e.path.clone()),
                        self.backend.as_ref(),
                    ) {
                        b.refresh_cover(&path);
                    }
                    if let Some(b) = &mut self.backend {
                        let _ = b.refresh_audio_info();
                    }
                    if let Some(position) = self.pending_seek.take()
                        && position > 0.
                    {
                        self.seek(position);
                    }
                }
                Event::Cover(path, cover) => {
                    if self.active_entry().is_some_and(|e| e.path == path) {
                        self.cover = cover;
                    }
                }
                Event::End(reason) if reason == "eof" => {
                    match selection::after_end(self.current, self.queue.len(), &self.repeat) {
                        selection::AfterEnd::Stop => self.stop(),
                        selection::AfterEnd::Play(i) => self.play_at(i, 0., true),
                        selection::AfterEnd::RepeatCurrent => self.load_active(0., true),
                    }
                }
                Event::End(reason) if reason == "error" => {
                    if let Some((mut r, _)) = self.resume_guard.take() {
                        r.retry();
                        self.backend.take();
                        self.loaded = false;
                        self.recovery = Some(r);
                        self.notice = "Audio device not ready after resume; retrying…".into();
                        continue;
                    }
                    self.playing = false;
                    self.loaded = false;
                    self.notice =
                        "Playback failed. Check the file or use Ctrl-Alt-R to reconnect audio."
                            .into();
                }
                Event::Error(e) => {
                    if e == "mpv disconnected" {
                        self.recover();
                    }
                    self.notice = e;
                }
                _ => {}
            }
        }
        *self.desktop.state.lock().unwrap() = Snapshot {
            status: if self.sleeping || !self.loaded {
                "Stopped"
            } else if self.playing {
                "Playing"
            } else {
                "Paused"
            }
            .into(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            url: self
                .active_entry()
                .and_then(|e| url::Url::from_file_path(&e.path).ok())
                .map(|u| u.to_string())
                .unwrap_or_default(),
            art: self
                .cover
                .as_ref()
                .and_then(|p| url::Url::from_file_path(p).ok())
                .map(|u| u.to_string())
                .unwrap_or_default(),
            track_id: self.track_id(),
            duration: self.duration,
            position: self.position,
            volume: self.config.volume,
            repeat: self.repeat.clone(),
            can_next: (self.detached.is_some() && !self.queue.is_empty())
                || self.current.is_some_and(|i| i + 1 < self.queue.len())
                || (self.repeat == "Playlist" && !self.queue.is_empty()),
            can_previous: self.active_entry().is_some(),
        };
        if self
            .resume_guard
            .as_ref()
            .is_some_and(|(_, deadline)| Instant::now() >= *deadline)
        {
            self.resume_guard = None;
        }
        if self.last_save.elapsed() >= Duration::from_secs(5) {
            self.save();
        }
    }
    fn track_id(&self) -> String {
        self.active_entry()
            .map(|e| format!("/org/mpris/MediaPlayer2/track/{}", e.id))
            .unwrap_or_default()
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.save();
    }
}
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
impl Player {
    fn button(
        &self,
        id: &'static str,
        label: &'static str,
        command: &'static str,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let font: SharedString = self.config.font_family.clone().into();
        let font_size = self.config.font_size;
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(LINE))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(0x3b4b59)))
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(icon(
                match command {
                    "previous" => "previous",
                    "next" => "next",
                    "toggle" if self.playing => "pause",
                    "toggle" => "play",
                    "stop" => "stop",
                    "volume_down" => "volume-down",
                    "volume_up" => "volume-up",
                    _ => "music",
                },
                18. * self.config.font_size / 14.,
                TEXT,
            ))
            .when(command == "toggle", |s| s.child(label))
            .tooltip(move |_, cx| {
                cx.new(|_| ControlTooltip(label, font.clone(), font_size))
                    .into()
            })
            .on_click(cx.listener(move |this, _, _, cx| this.command(command, cx)))
    }
    fn library_panel(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let count = self.visible_count();
        let mut cover = div()
            .w_full()
            .flex_shrink_0()
            .relative()
            .overflow_hidden()
            .border_t_1()
            .border_color(rgb(LINE))
            .bg(rgb(LINE))
            .flex()
            .items_center()
            .justify_center()
            .when_some(self.cover.clone(), |s, path| {
                s.child(
                    img(path)
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            })
            .when(self.cover.is_none(), |s| s.child(icon("music", 40., MUTED)));
        // Derive height from the actual panel width, including its window-width cap.
        cover.style().aspect_ratio = Some(1.);
        div()
            .flex()
            .flex_col()
            .w(px(self.config.library_width))
            .min_w(px(160.))
            .max_w(relative(0.48))
            .h_full()
            .flex_shrink_0()
            .bg(rgb(PANEL))
            .border_r_1()
            .border_color(rgb(LINE))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .text_color(rgb(if self.library_focus { ACCENT } else { MUTED }))
                            .child("LIBRARY"),
                    )
                    .child(
                        div()
                            .text_size(px(self.config.font_size * 12. / 14.))
                            .text_color(rgb(MUTED))
                            .child(self.config.library.display().to_string()),
                    ),
            )
            .child(
                uniform_list(
                    "library-list",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let mut rows = vec![];
                        if let Some(tree) = &this.tree {
                            tree.visible_contents(&this.expanded, &mut rows);
                        }
                        let anchor = this
                            .library_anchor
                            .as_ref()
                            .and_then(|p| rows.iter().position(|(n, _)| &n.path == p));
                        let selected_rows =
                            selection::span(anchor, this.library_cursor, rows.len());
                        range
                            .filter_map(|i| {
                                rows.get(i).map(|(node, depth)| {
                                    let cursor = this.library_cursor == i;
                                    let selected = selected_rows.contains(&i);
                                    let expanded = this.expanded.contains(&node.path);
                                    div()
                                        .id(i)
                                        .h(px(30. * this.config.font_size / 14.))
                                        .flex()
                                        .items_center()
                                        .pr_2()
                                        .pl(px((14. + *depth as f32 * 16.) * this.config.font_size
                                            / 14.))
                                        .relative()
                                        .children((0..*depth).map(|level| {
                                            div()
                                                .absolute()
                                                .left(px((22. + level as f32 * 16.)
                                                    * this.config.font_size
                                                    / 14.))
                                                .top_0()
                                                .bottom_0()
                                                .border_l_1()
                                                .border_color(rgb(0x39434f))
                                        }))
                                        .gap_2()
                                        .overflow_hidden()
                                        .bg(rgb(
                                            if cursor
                                                && this.library_focus
                                                && this.library_anchor.is_some()
                                            {
                                                0x365a47
                                            } else if selected && this.library_focus {
                                                0x294037
                                            } else if selected {
                                                0x242c35
                                            } else {
                                                PANEL
                                            },
                                        ))
                                        .text_color(rgb(if node.directory { TEXT } else { MUTED }))
                                        .cursor_pointer()
                                        .child(
                                            div()
                                                .w(px(16. * this.config.font_size / 14.))
                                                .flex_shrink_0()
                                                .when(node.directory, |s| {
                                                    s.child(icon(
                                                        if expanded {
                                                            "chevron-down"
                                                        } else {
                                                            "chevron-right"
                                                        },
                                                        16. * this.config.font_size / 14.,
                                                        MUTED,
                                                    ))
                                                }),
                                        )
                                        .child(icon(
                                            if node.directory {
                                                if expanded { "folder-open" } else { "folder" }
                                            } else {
                                                "music"
                                            },
                                            16. * this.config.font_size / 14.,
                                            if node.directory { TEXT } else { MUTED },
                                        ))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .child(library::label(&node.path)),
                                        )
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, window, cx| {
                                                this.focus.focus(window);
                                                if !this.library_focus {
                                                    this.cancel_visual();
                                                }
                                                this.library_focus = true;
                                                this.library_cursor = i;
                                                if event.click_count() == 2 {
                                                    this.branch(true);
                                                }
                                                cx.notify();
                                            },
                                        ))
                                })
                            })
                            .collect()
                    }),
                )
                .track_scroll(self.library_scroll.clone())
                .flex_1()
                .min_h_0(),
            )
            .child(cover)
    }
    fn queue_panel(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(LINE))
                    .child(
                        div()
                            .text_color(rgb(if !self.library_focus { ACCENT } else { MUTED }))
                            .child("PLAY QUEUE"),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .text_size(px(self.config.font_size))
                            .child(self.queue_summary()),
                    ),
            )
            .child(
                queue_columns(
                    [
                        "Track".into(),
                        "Title".into(),
                        "Artist".into(),
                        "Album".into(),
                        "Length".into(),
                        "Format".into(),
                    ],
                    self.config.font_size,
                )
                .h(px(30. * self.config.font_size / 14.))
                .border_l_2()
                .border_color(rgb(PANEL))
                .text_size(px(self.config.font_size * 12. / 14.))
                .text_color(rgb(MUTED))
                .bg(rgb(PANEL))
                .border_b_1()
                .border_color(rgb(LINE)),
            )
            .when(self.queue.is_empty(), |s| {
                s.child(
                    div()
                        .p_8()
                        .text_color(rgb(MUTED))
                        .child("Select a folder and press a to add all tracks recursively."),
                )
            })
            .child(
                uniform_list(
                    "queue-list",
                    self.queue.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let selected_rows = this.queue_selection();
                        range
                            .filter_map(|i| {
                                this.queue.get(i).map(|entry| {
                                    let info = this
                                        .metadata
                                        .get(&entry.path)
                                        .cloned()
                                        .unwrap_or_else(|| TrackInfo::fallback(&entry.path));
                                    let cursor = this.queue_cursor == i;
                                    let selected = selected_rows.contains(&i);
                                    let current = this.current == Some(i);
                                    let cells = [
                                        info.track.map(|n| n.to_string()).unwrap_or_default(),
                                        info.title,
                                        info.artist,
                                        info.album,
                                        info.duration.map(clock).unwrap_or_default(),
                                        info.format,
                                    ];
                                    div()
                                        .id(i)
                                        .h(px(34. * this.config.font_size / 14.))
                                        .border_b_1()
                                        .border_color(rgb(0x1e252d))
                                        .border_l_2()
                                        .border_color(rgb(if current { ACCENT } else { 0x1e252d }))
                                        .bg(rgb(
                                            if cursor
                                                && !this.library_focus
                                                && this.queue_anchor.is_some()
                                            {
                                                0x365a47
                                            } else if selected && !this.library_focus {
                                                0x25372f
                                            } else if selected {
                                                0x1c232c
                                            } else {
                                                BG
                                            },
                                        ))
                                        .text_color(rgb(if current { ACCENT } else { TEXT }))
                                        .cursor_pointer()
                                        .child(queue_columns(cells, this.config.font_size).h_full())
                                        .on_click(cx.listener(
                                            move |this, event: &ClickEvent, window, cx| {
                                                this.focus.focus(window);
                                                if this.library_focus {
                                                    this.cancel_visual();
                                                }
                                                this.library_focus = false;
                                                this.queue_cursor = i;
                                                if event.click_count() == 2 {
                                                    this.queue_anchor = None;
                                                    this.play_at(i, 0., true);
                                                }
                                                cx.notify();
                                            },
                                        ))
                                })
                            })
                            .collect()
                    }),
                )
                .track_scroll(self.queue_scroll.clone())
                .flex_1()
                .min_h_0(),
            )
    }
    fn transport(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let fraction = if self.duration > 0. {
            (self.position / self.duration).clamp(0., 1.) as f32
        } else {
            0.
        };
        let artist_album = [self.artist.as_str(), self.album.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let audio = format!(
            "{} · {}",
            if self.sample_rate > 0. {
                format!("{:.1} kHz", self.sample_rate / 1000.)
            } else {
                "— kHz".into()
            },
            if self.bitrate > 0. {
                format!("{:.0} kbps", self.bitrate / 1000.)
            } else {
                "— kbps".into()
            }
        );
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .bg(rgb(PANEL))
            .border_t_1()
            .border_color(rgb(LINE))
            .child(
                div().flex().items_end().px_4().pt_3().pb_2().gap_4().child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(self.config.font_size * 18. / 14.))
                                .text_center()
                                .truncate()
                                .child(self.title.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .relative()
                                .min_h(px(self.config.font_size * 1.6))
                                .text_color(rgb(MUTED))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_center()
                                        .px(px(120. * self.config.font_size / 14.))
                                        .truncate()
                                        .child(artist_album),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .right_0()
                                        .w(px(120. * self.config.font_size / 14.))
                                        .truncate()
                                        .text_right()
                                        .text_size(px(self.config.font_size * 12. / 14.))
                                        .child(audio),
                                ),
                        ),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .px_4()
                    .pb_3()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(rgb(MUTED))
                            .truncate()
                            .child(format!(
                                "{} / {}",
                                clock(self.position),
                                clock(self.duration)
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .gap_2()
                            .child(self.button("prev", "Previous", "previous", cx))
                            .child(self.button(
                                "play",
                                if self.playing { "Pause" } else { "Play" },
                                "toggle",
                                cx,
                            ))
                            .child(self.button("stop", "Stop", "stop", cx))
                            .child(self.button("next", "Next", "next", cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(self.button("quieter", "Volume down", "volume_down", cx))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .child(format!("{:.0}%", self.config.volume)),
                            )
                            .child(self.button("louder", "Volume up", "volume_up", cx))
                            .child(
                                div()
                                    .id("repeat-mode")
                                    .flex_shrink_0()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .px_2()
                                    .py_2()
                                    .rounded_md()
                                    .bg(rgb(LINE))
                                    .cursor_pointer()
                                    .child(icon(
                                        match self.repeat.as_str() {
                                            "Track" => "repeat-one",
                                            "Playlist" => "repeat",
                                            _ => "repeat-off",
                                        },
                                        18. * self.config.font_size / 14.,
                                        if self.repeat == "None" { MUTED } else { ACCENT },
                                    ))
                                    .child(match self.repeat.as_str() {
                                        "Track" => "One",
                                        "Playlist" => "All",
                                        _ => "Off",
                                    })
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.command("repeat", cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(5.))
                    .w_full()
                    .bg(rgb(LINE))
                    .child(div().h_full().w(relative(fraction)).bg(rgb(ACCENT))),
            )
    }
    fn help_panel(&self) -> impl IntoElement + use<> {
        let descriptions = [
            ("down / up", "Navigate the focused panel"),
            ("expand / collapse", "Open a folder or go to its parent"),
            ("first / last", "First or last row"),
            ("page_down / page_up", "Move twelve rows"),
            ("focus", "Switch library / queue"),
            (
                "visual",
                "Toggle visual selection; navigation extends the range",
            ),
            (
                "enqueue",
                "Append selected files and ALL folder descendants",
            ),
            ("activate", "Library: add and play. Queue: play selected"),
            ("toggle", "Play / pause"),
            ("stop", "Stop playback"),
            ("next / previous", "Next / previous track"),
            ("seek_back / seek_forward", "Seek five seconds"),
            ("volume_down / volume_up", "Volume ±5%"),
            (
                "font_down / font_up",
                "Decrease / increase interface font size",
            ),
            ("font_reset", "Reset font size to 14 px"),
            ("narrow / widen", "Resize the library panel"),
            ("remove", "Remove selected queue entry or visual range"),
            (
                "move_up / move_down",
                "Move selected queue entry or visual range",
            ),
            ("clear", "Clear the queue"),
            ("repeat", "Cycle no repeat / playlist / track"),
            ("rescan", "Refresh library from disk"),
            ("recover", "Recreate audio backend at current position"),
            ("quit", "Save and quit"),
            ("help / dismiss", "Close help / cancel visual selection"),
        ];
        let bindings = self.config.bindings();
        div().absolute().inset_0().p_6().bg(rgba(0x101318f5)).child(
            div()
                .id("keyboard-help")
                .size_full()
                .overflow_y_scroll()
                .p_5()
                .bg(rgb(PANEL))
                .rounded_lg()
                .border_1()
                .border_color(rgb(LINE))
                .child(
                    div()
                        .text_size(px(self.config.font_size * 24. / 14.))
                        .mb_2()
                        .text_color(rgb(ACCENT))
                        .child("Keyboard controls"),
                )
                .child(
                    div()
                        .text_size(px(self.config.font_size))
                        .mb_4()
                        .text_color(rgb(MUTED))
                        .child(
                            "Remap any command in config.toml [keys]. F1 or Escape closes help.",
                        ),
                )
                .children(descriptions.into_iter().map(|(commands, description)| {
                    let keys = commands
                        .split(" / ")
                        .filter_map(|c| bindings.get(c))
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("  ·  ");
                    div()
                        .flex()
                        .gap_4()
                        .py_2()
                        .border_b_1()
                        .border_color(rgb(LINE))
                        .child(div().w(relative(0.4)).text_color(rgb(ACCENT)).child(keys))
                        .child(div().flex_1().child(description))
                })),
        )
    }
}
impl Render for Player {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Player")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, action: &RunCommand, _, cx| {
                if !this.help
                    || matches!(
                        action.command.as_str(),
                        "help" | "dismiss" | "quit" | "font_up" | "font_down" | "font_reset"
                    )
                {
                    this.command(&action.command, cx);
                }
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .font_family(self.config.font_family.clone())
            .text_size(px(self.config.font_size))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.library_panel(cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(self.queue_panel(cx))
                            .child(self.transport(cx)),
                    ),
            )
            .child(
                div()
                    .h(px(28. * self.config.font_size / 14.))
                    .flex()
                    .items_center()
                    .px_4()
                    .text_size(px(self.config.font_size * 12. / 14.))
                    .text_color(rgb(MUTED))
                    .overflow_hidden()
                    .border_t_1()
                    .border_color(rgb(LINE))
                    .child(self.mode_status()),
            )
            .when(self.help, |s| s.child(self.help_panel()))
    }
}

// A shared layout keeps headers and rows aligned as the library panel resizes.
fn queue_columns(
    [track, title, artist, album, length, format]: [String; 6],
    font_size: f32,
) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .px_2()
        .gap_2()
        .overflow_hidden()
        .child(
            div()
                .w(px(34. * font_size / 14.))
                .flex_shrink_0()
                .text_right()
                .truncate()
                .child(track),
        )
        .child(div().flex_1().min_w_0().truncate().child(title))
        .child(
            div()
                .w(relative(0.20))
                .flex_shrink_0()
                .truncate()
                .child(artist),
        )
        .child(
            div()
                .w(relative(0.22))
                .flex_shrink_0()
                .truncate()
                .child(album),
        )
        .child(
            div()
                .w(px(54. * font_size / 14.))
                .flex_shrink_0()
                .text_right()
                .truncate()
                .child(length),
        )
        .child(
            div()
                .w(px(56. * font_size / 14.))
                .flex_shrink_0()
                .truncate()
                .child(format),
        )
}
struct ControlTooltip(&'static str, SharedString, f32);
impl Render for ControlTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .bg(rgb(LINE))
            .text_color(rgb(TEXT))
            .rounded_md()
            .font_family(self.1.clone())
            .text_size(px(self.2))
            .child(self.0)
    }
}
