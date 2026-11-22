{
  description = "Cintec: a keyboard-first Rust + GPUI music player";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
      dependenciesFor = pkgs: import ./nix/dependencies.nix { inherit pkgs; };
      packageFor = pkgs: pkgs.callPackage ./nix/package.nix {
        dependencies = dependenciesFor pkgs;
      };
    in
    {
      packages = eachSystem (system:
        let cintec = packageFor (pkgsFor system);
        in {
          inherit cintec;
          default = cintec;
        });

      apps = eachSystem (system:
        let
          cintec = {
            type = "app";
            program = nixpkgs.lib.getExe self.packages.${system}.cintec;
            meta.description = "Keyboard-first directory music player";
          };
        in {
          inherit cintec;
          default = cintec;
        });

      overlays.default = final: _prev: {
        cintec = packageFor final;
      };

      checks = eachSystem (system: {
        package = self.packages.${system}.cintec;
      });

      devShells = eachSystem (system:
        let
          pkgs = pkgsFor system;
          dependencies = dependenciesFor pkgs;
        in {
          default = pkgs.mkShell {
            nativeBuildInputs = dependencies.nativeBuildInputs;
            buildInputs = dependencies.buildInputs;
            packages = with pkgs; [
              cargo rustc rustfmt clippy
              mpv ffmpeg dbus playerctl python3
            ];
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath dependencies.runtimeLibraries;
            RUST_BACKTRACE = "1";
          };
        });
    };
}
