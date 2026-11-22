{ pkgs }:
let
  libraries = with pkgs; [
    alsa-lib
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
    libxcb
    vulkan-loader
    libGL
    fontconfig
    freetype
    openssl
  ];
in
{
  nativeBuildInputs = with pkgs; [
    pkg-config
    cmake
    rustPlatform.bindgenHook
  ];
  buildInputs = libraries;
  # GPUI loads some platform libraries with dlopen, outside the ELF RPATH.
  runtimeLibraries = libraries;
}
