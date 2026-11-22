# Local compatibility patch

Source: xattr 0.2.3 from crates.io, checksum
`6d1526bbe5aaeb5eb06885f4d987bcdfa5e23187055de9b83fe00156a821fabc`.
Upstream: https://github.com/Stebalien/xattr

GPUI 0.2.2 pulls this version through gpui_http_client and zed-async-tar.
In src/sys/mod.rs, use libc::ENODATA for missing attributes on Linux and
Android; retain libc::ENOATTR on macOS and BSD. This avoids relying on the
removed Linux ENOATTR alias in newer libc versions. ENOSTR is unrelated.

All other upstream source and both licenses are preserved. The root
[patch.crates-io] entry applies this patch to both Cargo and Nix builds.
