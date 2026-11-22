# Cîntec

A native Rust + GPUI music player for Linux and macOS, built around keyboard control and the directory layout of your music collection. mpv handles audio decoding and output; zbus provides MPRIS and systemd-logind integration on Linux.

The library and album art occupy the left panel. The right panel contains the queue and, below it, transport controls, title, artist, album, elapsed/total time, sample rate, bitrate, volume, and a bottom progress bar. The library tree starts with the configured root’s contents; the root itself is hidden and cannot be selected or collapsed. Folder addition always traverses every descendant, including collapsed folders. The queue supports duplicates, removal, reordering, and repeat modes. Each track occupies one row with columns for album track number (right-aligned), title, artist, album, duration, and format. Metadata is read in the background with ffprobe, including tracks that have not played yet. Missing title tags fall back to the filename; missing artist/album/track tags stay empty. The queue has no queue-position numbers or file paths. A narrow accent border marks the current track, and tree indentation guides show folder depth. Controls and tree markers use embedded SVG icons, so no icon font or network download is needed. The transport row places elapsed/total time on the left, playback controls in the center, and volume on the right. Sample rate and bitrate appear right-aligned alongside the artist/album line. Audio parameters are explicitly refreshed after each load/reconfiguration, even when the next track has the same sample rate, with ffprobe metadata as a fallback. The repeat button follows the volume buttons and cycles Off → All → One. The queue header shows track count and summed duration; unknown lengths are explicitly counted until they can be read.

FLAC, Opus, Ogg Vorbis, MP3, AAC/M4A, WAV, AIFF, ALAC, APE, WavPack and WMA are recognized; actual decoding depends on your mpv/FFmpeg build. Artwork uses embedded images extracted with FFmpeg first. If none is available, it searches the track folder for case-insensitive `cover.*` or `folder.*` images (JPEG, PNG, WebP, GIF, BMP, TIFF, AVIF), preferring `cover` images. The cover sits below the library tree in a square spanning the panel width and resizes with the panel. Images retain their aspect ratio. Title and artist/album are centered horizontally above the playback controls. There is no lyrics integration. Discord integration is not implemented.

## Build from source

Use a current stable Rust toolchain from [rustup](https://rustup.rs/), including Cargo. Install the dependencies for your platform below, then run these commands from the repository root:

```sh
rustup update stable
cargo +stable build --release --locked
./target/release/cintec --library "$HOME/Music"
```

Keep `Cargo.lock` and `vendor/xattr` with the source. The first build downloads the locked Rust dependencies. `mpv`, `ffmpeg`, and `ffprobe` must remain on PATH when running Cîntec; FFmpeg packages supply both FFmpeg tools. Their paths can also be set in `config.toml`. Linux requires a Wayland or X11 desktop session and a compatible Vulkan driver. macOS uses Metal.

### Arch Linux

```sh
sudo pacman -Syu --needed base-devel cmake pkgconf clang llvm \
  alsa-lib fontconfig freetype2 libxkbcommon libxkbcommon-x11 wayland \
  libx11 libxcursor libxi libxrandr libxcb vulkan-icd-loader libglvnd \
  openssl dbus mpv ffmpeg
```

Install the Vulkan driver appropriate for your GPU as well, such as `vulkan-radeon` or `vulkan-intel` for supported AMD or Intel hardware.

### Ubuntu / Debian

```sh
sudo apt-get update
sudo apt-get install build-essential cmake pkg-config clang libclang-dev \
  libasound2-dev libfontconfig1-dev libfreetype6-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libx11-xcb-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxcb1-dev libvulkan1 libgl1-mesa-dev libssl-dev \
  dbus mpv ffmpeg
```

For supported Intel/AMD graphics, install `mesa-vulkan-drivers`; NVIDIA hardware needs the appropriate NVIDIA driver. On Ubuntu, enable the Universe repository if `mpv` or `ffmpeg` is unavailable. Use rustup rather than an older distribution Rust package.

### Fedora

```sh
sudo dnf install gcc gcc-c++ make cmake pkgconf-pkg-config clang clang-devel llvm-devel \
  alsa-lib-devel fontconfig-devel freetype-devel \
  libxkbcommon-devel libxkbcommon-x11-devel wayland-devel \
  libX11-devel libXcursor-devel libXi-devel libXrandr-devel libxcb-devel \
  vulkan-loader mesa-libGL-devel openssl-devel dbus mpv ffmpeg-free
```

For supported Intel/AMD graphics, install `mesa-vulkan-drivers`. Fedora's `ffmpeg-free` provides `ffmpeg` and `ffprobe`; codec availability depends on your installed multimedia packages. [Fedora package information](https://packages.fedoraproject.org/pkgs/ffmpeg/ffmpeg-free/).

### macOS

Install the full Xcode application, open it once to finish installing its components, and select its developer tools. Command Line Tools alone do not supply everything GPUI needs to compile its Metal shaders. See the [upstream macOS build prerequisites](https://zed.dev/docs/development/macos#dependencies).

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
brew install cmake pkgconf llvm mpv ffmpeg
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
```

If your Xcode installation supplies Metal as a separate component, install it with `xcodebuild -downloadComponent MetalToolchain`. Then use the common Cargo build commands above. Build natively on Apple Silicon or Intel; no universal binary is required. Launch the executable from your terminal so it inherits Homebrew's PATH. MPRIS and automatic logind suspend recovery are Linux integrations; native macOS media-key/Now Playing and sleep notifications are not implemented. Ctrl-Alt-R remains available to recreate the audio backend manually.

### Install a Cargo build

```sh
mkdir -p "$HOME/.local/bin"
install -m755 target/release/cintec "$HOME/.local/bin/cintec"
```

Add `~/.local/bin` to PATH. On Linux, optionally install the desktop entry:

```sh
mkdir -p "$HOME/.local/share/applications"
install -m644 cintec.desktop "$HOME/.local/share/applications/cintec.desktop"
```

## GitHub releases

The [release workflow](.github/workflows/release.yml) builds native archives for Linux x86-64/ARM64 and macOS Intel/Apple Silicon. Push a version tag matching `Cargo.toml` to create a GitHub release:

```sh
git tag v0.1.0
git push origin v0.1.0
```

Update the manifest and lockfile before tagging a new version. All four builds and their tests must pass before publication. Tags with a prerelease suffix, such as `v0.2.0-rc.1`, create prereleases. Re-running a tag workflow replaces its assets. Running the workflow manually on a branch produces downloadable Actions artifacts without publishing a release. Publication uses the built-in `GITHUB_TOKEN`; no personal access token is needed.

Each release includes `.tar.gz` archives and `SHA256SUMS`. Extract the archive for your OS and architecture and install its `cintec` binary as above, using the extracted path instead of `target/release/cintec`. Archives include this README and the configuration example, plus the desktop entry on Linux. Install mpv, FFmpeg, and the platform runtime libraries separately using the dependency instructions above. Linux archives are built on Ubuntu 22.04 and require glibc 2.35 or newer with compatible system libraries; use a source build or the Nix package on other systems. macOS archives are unsigned terminal executables, not notarized app bundles.

## Install as a Nix flake package

The flake exports `packages.<system>.cintec`, `packages.<system>.default`, matching `apps`, and `overlays.default`, for `x86_64-linux` and `aarch64-linux`.

```sh
nix build .#cintec
nix run . -- --library "$HOME/Music"
```

The package installs a desktop entry and wraps Cîntec with mpv, FFmpeg, and GPUI's runtime libraries. It runs without `nix develop` or additional nix-ld configuration. GPU/Vulkan drivers, fonts, and the Wayland/X11 and D-Bus sessions come from the host system.

Add this input to your NixOS system flake (replace the local path, or use your repository URL once published):

```nix
inputs.cintec = {
  url = "path:/absolute/path/to/cintec";
  inputs.nixpkgs.follows = "nixpkgs";
};
```

Capture `cintec` in your existing `outputs` arguments and add a module to your existing system configuration:

```nix
outputs = { nixpkgs, cintec, ... }: {
  nixosConfigurations.my-host = nixpkgs.lib.nixosSystem {
    system = "x86_64-linux";
    modules = [
      ./configuration.nix
      ({ pkgs, ... }: {
        environment.systemPackages = [
          cintec.packages.${pkgs.stdenv.hostPlatform.system}.default
        ];
      })
    ];
  };
};
```

Alternatively, add `cintec.overlays.default` to `nixpkgs.overlays` and install `pkgs.cintec`. Home Manager can use the same package output in `home.packages`. Following your system's `nixpkgs` input shares its package set and toolchain; use a recent nixpkgs with Rust 2024 support.

**Source preparation:** `Cargo.lock` is included and required for the Nix package. Use `cargo build --locked` to preserve the resolved dependency versions.

Keep `Cargo.lock` and `flake.lock` in version control. The package vendors crates using the checksums in `Cargo.lock`, so there is no placeholder `cargoHash` to replace. When using a Git-backed flake, stage the new source/Nix files and lockfiles before building; Nix omits untracked files. A local input locked by another flake must be refreshed with `nix flake update cintec` after source changes.

## Development on NixOS

```sh
nix develop
cargo run --release -- --library "$HOME/Music"
```

The development shell supplies the Rust toolchain, build dependencies, mpv, FFmpeg and runtime libraries. A binary built directly with Cargo should still be launched from this environment; use the Nix package above for installation.

Default library: the XDG music directory, falling back to `~/Music`. `--library` overrides the configured root. A missing directory is reported in the status line; creating it and pressing Ctrl-R rescans. The scanner skips symlinks to avoid loops and duplicate subtrees, and reports unreadable paths. Ordering is directories first, then filename order. It does not index tags or flatten albums.

## Keys

All functionality is available without a mouse. F1 displays the active bindings, including custom mappings.

| Keys | Action |
| --- | --- |
| j/k, Down/Up | Navigate the focused panel |
| h/l, Left/Right | Collapse/go to parent, expand/enter folder |
| Home/End, g/G | First/last entry |
| Ctrl-D/Ctrl-U, PageDown/PageUp | Move twelve entries |
| Tab, Shift-Tab | Switch library/queue |
| v, Shift-V | Toggle visual selection; navigation extends the highlighted range |
| a | Append selected files/folders recursively, including a visual range |
| Enter | Library: append and play selection; queue: play selected entry |
| Space | Play/pause |
| s | Stop |
| n/p | Next/previous; previous restarts after three seconds |
| Alt-Left/Alt-Right | Seek backward/forward five seconds |
| - / + or = | Volume down/up by five percentage points |
| Ctrl-minus / Ctrl-plus (or Ctrl-=) | Decrease/increase interface font size |
| Ctrl-0 | Reset font size to 14 px |
| [ / ] | Narrow/widen library by 24 pixels |
| d, Delete | Remove selected queue entry or visual range |
| Shift-K/Shift-J | Move selected queue entry or visual range up/down |
| Shift-D, Ctrl-Shift-Backspace | Clear the entire queue |
| r | Cycle no repeat → playlist → track |
| Ctrl-R | Rescan library |
| Ctrl-Alt-R | Recreate audio backend and restore playback position/state |
| F1, ? | Keyboard help |
| Escape | Dismiss help or cancel visual selection |
| Ctrl-Q | Save and quit |

Visual mode selects a contiguous range from an anchor to the cursor, like Vim's linewise selection. Press `v` (or Shift-V), then use j/k, arrows, paging, or Home/End to extend or shrink it. In the library, `a` recursively adds every selected file/folder, avoiding duplicate paths when a selected folder includes another selected entry. This does not remove duplicates already in the queue. In the queue, `d` removes the range and exits visual mode; Shift-J/K moves the entire block one row while preserving its order and keeping visual mode active. Moving tracks, or deleting other tracks, preserves the currently playing entry. Deleting the sounding track (including as part of a visual range or clearing the queue) removes it from the queue while preserving its audio, metadata, artwork, position, pause state, and MPRIS identity until EOF. With repeat Off it then stops; with All it starts at the queue head if nonempty; with One it repeats the sounding track, even if removed. Explicit Next starts the queue head after a removal. Other queue edits leave the sounding track alone. A removed sounding track is also saved in the session and restored by manual playback or suspend recovery. `v` or Escape cancels selection; switching panels or rescanning the tree also cancels it. Playback advancing automatically does not move a visual selection. The status line shows VISUAL and the selected count. Shift-D always clears the full queue, even in visual mode.

Clicks select rows; double-click opens a library folder or plays a queue entry. Transport and volume controls also accept clicks.

## Configuration and saved state

Configuration on Linux: `$XDG_CONFIG_HOME/cintec/config.toml` (usually `~/.config/cintec/config.toml`). On macOS: `~/Library/Application Support/cintec/config.toml`. See `config.example.toml`. To change the interface font, exit Cintec, set the top-level `font_family = "Inter"` (or another installed family such as `Noto Sans` or `Iosevka`), then restart. It defaults to `sans-serif`. Adjust size live with Ctrl-minus / Ctrl-plus (Ctrl-= also works); Ctrl-0 resets it to 14 px. Size changes are saved automatically and also work while help is open. The top-level `font_size = 14.0` setting accepts 10–32 logical pixels. Row heights, icons, indentation, and fixed queue columns scale with it. Keep font settings above `[keys]`. The optional `ffprobe = "ffprobe"` setting selects the metadata probe executable; the Nix package supplies it with FFmpeg. The application writes a default configuration and saves volume, panel width, and font size. Exit before editing configuration externally; it is loaded at startup and written during playback.

Map command names to arrays of GPUI key strings in `[keys]`. Entries replace the default mapping for that command; an empty array disables it. Use distinct keys across commands. The F1 screen lists command names and active bindings. For example:

```toml
[keys]
enqueue = ["a", "ctrl-enter"]
narrow = ["ctrl-left"]
widen = ["ctrl-right"]
```

Queue, current entry (including a removed sounding track), and position are atomically saved under `$XDG_STATE_HOME/cintec/session.json` (normally `~/.local/state/cintec/session.json`), every five seconds, after edits, before logind suspend, and on exit. On macOS, the session file is `~/Library/Application Support/cintec/session.json`. Playback is not started automatically on application launch; Space resumes the saved entry and position.

## Desktop control (Linux)

MPRIS service: `org.mpris.MediaPlayer2.cintec`. Supports play/pause/stop, next/previous, relative/absolute seeks, volume, repeat mode, local-file OpenUri, track metadata, and artwork. Changes are published using PropertiesChanged and Seeked. Track IDs remain stable while queue entries are reordered. Shuffle and playback speeds other than 1× are explicitly unsupported. One instance owns the MPRIS service; a second instance can run but reports that MPRIS is unavailable.

```sh
playerctl --player=cintec metadata
playerctl --player=cintec play-pause
playerctl --player=cintec position 10+
playerctl --player=cintec volume 0.5
```

The Nix package installs `cintec.desktop` automatically. For a manual Cargo installation, install it to `~/.local/share/applications/`, put the executable on PATH, and provide its runtime environment.

## Suspend and resume (Linux)

A Rust backend alone does not identify the cause of termusic's wake problem; it could involve an audio stream, device, sound-server connection, or application recovery logic. Cîntec avoids relying on a pre-suspend audio stream surviving:

1. Subscribe to logind's `PrepareForSleep` signal on the system bus. Request a **delay** inhibitor, allowing a brief save/cleanup window without blocking normal laptop sleep.
2. Before sleep, snapshot position and playing/paused/stopped state, persist the session, terminate mpv, and release the inhibitor. The watcher waits at most two seconds for the UI acknowledgment.
3. After wake, allow one second for audio services/endpoints to return, then start a fresh mpv process. Restore the same queue entry, seek once the file is loaded, and preserve paused/stopped state.
4. Retry process startup with backoff capped at 16 seconds. Playback errors during the first ten seconds of recovery also retry, covering a device that appears later. Ctrl-Alt-R performs the same backend recreation manually.

If the system bus or logind is unavailable, the UI reports it; playback remains available and manual recovery still works. Delay-inhibitor access is optional: signal monitoring continues if requesting one fails. Recovery uses the system's default audio endpoint, which can change after wake.

Research references: [logind sleep signals and inhibitors](https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html), [mpv IPC and audio documentation](https://mpv.io/manual/stable/). These define the recovery mechanism; they do not establish the root cause of your termusic issue.

## Dependency compatibility

GPUI 0.2.2 pulls in `xattr 0.2.3` through `gpui_http_client` and `zed-async-tar`. That old crate references the removed Linux `libc::ENOATTR` alias. A small local patch in `vendor/xattr` uses `libc::ENODATA` on Linux/Android and retains `ENOATTR` on macOS/BSD. The root `[patch.crates-io]` applies it to both Cargo and Nix builds, with libc left at the locked version. Keep the vendor directory with the source; do not edit Cargo's registry cache. See `vendor/xattr/CINTEC-PATCH.md` for provenance and the retained licenses.
