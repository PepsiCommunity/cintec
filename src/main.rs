mod config;
mod desktop;
mod icons;
mod library;
mod metadata;
mod playback;
mod recovery;
mod selection;
mod ui;

use anyhow::{Context, Result};
use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
fn main() -> Result<()> {
    let mut config = config::Config::load()?;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--library") => {
                config.library = args
                    .next()
                    .context("--library requires a directory")?
                    .into()
            }
            Some("--help" | "-h") => {
                println!(
                    "Cintec — keyboard-first music player\n\nUsage: cintec [--library DIRECTORY]\n\nF1: keyboard help. Config: {}",
                    config::paths()?.0.display()
                );
                return Ok(());
            }
            _ => anyhow::bail!("Unknown argument: {}", arg.to_string_lossy()),
        }
    }
    if !config.library.is_absolute() {
        config.library = std::env::current_dir()?.join(&config.library);
    }
    let bindings = config.bindings();
    Application::new()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.bind_keys(bindings.iter().flat_map(|(command, keys)| {
                keys.iter().map(move |key| {
                    gpui::KeyBinding::new(
                        key,
                        ui::RunCommand {
                            command: command.clone(),
                        },
                        Some("Player"),
                    )
                })
            }));
            let bounds = Bounds::centered(None, size(px(1180.), px(760.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(760.), px(500.))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Cintec".into()),
                        ..Default::default()
                    }),
                    app_id: Some("cintec".into()),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| ui::Player::new(config, window, cx)),
            )
            .expect("Cannot create GPUI window");
            cx.activate(true);
        });
    Ok(())
}
