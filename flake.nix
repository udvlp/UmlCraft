{
  description = "UmlCraft, a UML modeling tool";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        umlcraft = pkgs.callPackage ./packaging/nix/package.nix { };
        default = umlcraft;
      });

      # For a NixOS configuration: `nixpkgs.overlays = [ umlcraft.overlays.default ];`, then
      # `environment.systemPackages = [ pkgs.umlcraft ];`.
      overlays.default = final: _: { umlcraft = final.callPackage ./packaging/nix/package.nix { }; };

      # `nix develop`: Rust and the libraries GPUI builds and runs with, for `cargo run`.
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.umlcraft ];
          packages = with pkgs; [
            clippy
            rustfmt
            rust-analyzer
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
            with pkgs;
            [
              libGL
              vulkan-loader
              wayland
            ]
          );
        };
      });
    };
}
