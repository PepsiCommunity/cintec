{
  lib,
  rustPlatform,
  makeWrapper,
  mpv,
  ffmpeg,
  dependencies,
}:
let
  manifest = builtins.fromTOML (builtins.readFile ../Cargo.toml);
in
rustPlatform.buildRustPackage {
  pname = manifest.package.name;
  version = manifest.package.version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      lib.cleanSourceFilter path type
      && !(builtins.elem (builtins.baseNameOf path) [
        "target"
        "result"
        ".direnv"
        ".agents"
        ".aws"
        ".codex"
        "__pycache__"
      ]);
  };

  # Registry checksums in Cargo.lock pin every vendored crate. No cargoHash
  # needs updating when the dependency lock changes.
  cargoLock.lockFile =
    if builtins.pathExists ../Cargo.lock then ../Cargo.lock
    else throw "Cintec: Cargo.lock is missing. Run cargo generate-lockfile in the source checkout and include Cargo.lock in the flake source.";

  nativeBuildInputs = dependencies.nativeBuildInputs ++ [ makeWrapper ];
  inherit (dependencies) buildInputs;
  dontUseCmakeConfigure = true;
  strictDeps = true;

  postInstall = ''
    install -Dm644 cintec.desktop "$out/share/applications/cintec.desktop"
    substituteInPlace "$out/share/applications/cintec.desktop" \
      --replace-fail 'Exec=cintec' "Exec=$out/bin/cintec"

    install -Dm644 README.md "$out/share/doc/cintec/README.md"
    install -Dm644 config.example.toml "$out/share/doc/cintec/config.example.toml"

    wrapProgram "$out/bin/cintec" \
      --prefix PATH : ${lib.makeBinPath [ mpv ffmpeg ]} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath dependencies.runtimeLibraries}
  '';

  meta = {
    description = manifest.package.description;
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "cintec";
  };
}
