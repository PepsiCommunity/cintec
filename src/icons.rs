//! Small original SVG icons embedded in the executable (no icon font required).
use gpui::{AssetSource, SharedString, Styled, Svg, px, rgb, svg};
use std::borrow::Cow;
const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/repeat.svg",
        include_bytes!("../assets/icons/repeat.svg"),
    ),
    (
        "icons/repeat-one.svg",
        include_bytes!("../assets/icons/repeat-one.svg"),
    ),
    (
        "icons/repeat-off.svg",
        include_bytes!("../assets/icons/repeat-off.svg"),
    ),
    ("icons/play.svg", include_bytes!("../assets/icons/play.svg")),
    (
        "icons/pause.svg",
        include_bytes!("../assets/icons/pause.svg"),
    ),
    (
        "icons/previous.svg",
        include_bytes!("../assets/icons/previous.svg"),
    ),
    ("icons/next.svg", include_bytes!("../assets/icons/next.svg")),
    ("icons/stop.svg", include_bytes!("../assets/icons/stop.svg")),
    (
        "icons/volume-down.svg",
        include_bytes!("../assets/icons/volume-down.svg"),
    ),
    (
        "icons/volume-up.svg",
        include_bytes!("../assets/icons/volume-up.svg"),
    ),
    (
        "icons/folder.svg",
        include_bytes!("../assets/icons/folder.svg"),
    ),
    (
        "icons/folder-open.svg",
        include_bytes!("../assets/icons/folder-open.svg"),
    ),
    (
        "icons/chevron-right.svg",
        include_bytes!("../assets/icons/chevron-right.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../assets/icons/chevron-down.svg"),
    ),
    (
        "icons/music.svg",
        include_bytes!("../assets/icons/music.svg"),
    ),
];
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| (*name).into())
            .collect())
    }
}
pub fn icon(name: &str, size: f32, color: u32) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_shrink_0()
        .text_color(rgb(color))
}
