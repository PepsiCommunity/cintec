use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

pub const FORMATS: &[&str] = &[
    "flac", "opus", "ogg", "mp3", "m4a", "aac", "wav", "aiff", "aif", "alac", "ape", "wv", "wma",
    "mp4", "oga",
];

#[derive(Clone, Debug)]
pub struct Node {
    pub path: PathBuf,
    pub children: Vec<Node>,
    pub directory: bool,
}
impl Node {
    pub fn scan(path: &Path, warnings: &mut Vec<String>) -> Option<Self> {
        let meta = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(e) => {
                warnings.push(format!("{}: {e}", path.display()));
                return None;
            }
        };
        // Never follow symlinks: directory cycles and duplicate trees are avoided.
        if meta.file_type().is_symlink() {
            return None;
        }
        let directory = meta.is_dir();
        if !directory && !is_audio(path) {
            return None;
        }
        let mut children = Vec::new();
        if directory {
            match fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                if let Some(node) = Self::scan(&entry.path(), warnings) {
                                    children.push(node);
                                }
                            }
                            Err(e) => warnings.push(e.to_string()),
                        }
                    }
                }
                Err(e) => warnings.push(format!("{}: {e}", path.display())),
            }
            children.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
            });
        }
        Some(Self {
            path: path.into(),
            children,
            directory,
        })
    }
    pub fn tracks(&self, out: &mut Vec<PathBuf>) {
        if self.directory {
            for child in &self.children {
                child.tracks(out);
            }
        } else {
            out.push(self.path.clone());
        }
    }
    /// The configured root is a container; render only its children at depth 0.
    pub fn visible_contents<'a>(
        &'a self,
        expanded: &HashSet<PathBuf>,
        out: &mut Vec<(&'a Node, usize)>,
    ) {
        for child in &self.children {
            child.visible(expanded, 0, out);
        }
    }
    pub fn visible<'a>(
        &'a self,
        expanded: &HashSet<PathBuf>,
        depth: usize,
        out: &mut Vec<(&'a Node, usize)>,
    ) {
        out.push((self, depth));
        if expanded.contains(&self.path) {
            for child in &self.children {
                child.visible(expanded, depth + 1, out);
            }
        }
    }
}
/// Collect recursively from multiple selected nodes without duplicating paths
/// when selected folders overlap selected descendants.
pub fn selected_tracks<'a>(nodes: impl IntoIterator<Item = &'a Node>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for node in nodes {
        node.tracks(&mut paths);
    }
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| FORMATS.contains(&x.to_ascii_lowercase().as_str()))
}
pub fn label(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("cintec-library-test-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn hidden_root_starts_at_children_and_cannot_be_collapsed() {
        let dir = TestDirectory::new();
        fs::create_dir_all(dir.path().join("album")).unwrap();
        fs::write(dir.path().join("album/a.flac"), b"").unwrap();
        fs::write(dir.path().join("b.opus"), b"").unwrap();
        let tree = Node::scan(dir.path(), &mut vec![]).unwrap();
        let mut rows = vec![];
        tree.visible_contents(&HashSet::new(), &mut rows);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, 0);
        assert_ne!(rows[0].0.path, dir.path());
        let mut expanded = HashSet::new();
        expanded.insert(dir.path().join("album"));
        rows.clear();
        tree.visible_contents(&expanded, &mut rows);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].1, 1);
        assert_eq!(label(&rows[1].0.path), "a.flac");
    }
    #[test]
    fn selected_subfolder_does_not_enqueue_siblings() {
        let dir = TestDirectory::new();
        fs::create_dir_all(dir.path().join("one/deep")).unwrap();
        fs::create_dir_all(dir.path().join("two")).unwrap();
        for name in ["one/deep/a.flac", "two/b.opus"] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        let root = Node::scan(dir.path(), &mut vec![]).unwrap();
        let selected = root
            .children
            .iter()
            .find(|n| label(&n.path) == "one")
            .unwrap();
        let mut tracks = vec![];
        selected.tracks(&mut tracks);
        assert_eq!(tracks, vec![dir.path().join("one/deep/a.flac")]);
    }
    #[test]
    fn visual_folder_and_descendant_selection_deduplicates_in_traversal_order() {
        let dir = TestDirectory::new();
        fs::create_dir_all(dir.path().join("album/disc")).unwrap();
        fs::write(dir.path().join("album/disc/one.flac"), b"").unwrap();
        fs::write(dir.path().join("album/two.opus"), b"").unwrap();
        fs::write(dir.path().join("other.mp3"), b"").unwrap();
        let root = Node::scan(dir.path(), &mut vec![]).unwrap();
        let folder = &root.children[0];
        let nested = &folder.children[0];
        let track = &nested.children[0];
        let selected = selected_tracks([folder, nested, track, &root.children[1]]);
        assert_eq!(
            selected,
            vec![
                dir.path().join("album/disc/one.flac"),
                dir.path().join("album/two.opus"),
                dir.path().join("other.mp3")
            ]
        );
        // A later enqueue operation can intentionally add the same files again.
        assert_eq!(selected_tracks([track]), vec![track.path.clone()]);
    }
    #[test]
    fn recursive_queue_includes_collapsed_grandchildren_and_ignores_symlink_cycles() {
        let dir = TestDirectory::new();
        fs::create_dir_all(dir.path().join("album/disc2")).unwrap();
        for name in [
            "root.FLAC",
            "album/one.opus",
            "album/disc2/two.mp3",
            "notes.txt",
        ] {
            fs::write(dir.path().join(name), b"").unwrap();
        }
        std::os::unix::fs::symlink(dir.path(), dir.path().join("album/cycle")).unwrap();
        let root = Node::scan(dir.path(), &mut vec![]).unwrap();
        let mut tracks = vec![];
        root.tracks(&mut tracks);
        assert_eq!(tracks.len(), 3);
        assert_eq!(label(&tracks[0]), "two.mp3");
        let mut rows = vec![];
        root.visible(&HashSet::new(), 0, &mut rows);
        assert_eq!(rows.len(), 1);
    }
}
