{
  description = "UmlCraft, a UML modeling tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # The Rust that `rust-version` in Cargo.toml names, which nixpkgs may not have yet.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      rustVersion = (nixpkgs.lib.importTOML ./Cargo.toml).workspace.package.rust-version;
      # nixpkgs with the overlay, and a Rust platform of exactly that version.
      withRust =
        pkgs:
        let
          rust = pkgs.rust-bin.stable."${rustVersion}.0".minimal;
        in
        {
          inherit rust;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        };
      forAll =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            }
          )
        );
      umlcraftFor =
        pkgs: pkgs.callPackage ./packaging/nix/package.nix { inherit (withRust pkgs) rustPlatform; };
    in
    {
      packages = forAll (pkgs: rec {
        umlcraft = umlcraftFor pkgs;
        default = umlcraft;
      });

      # For a NixOS configuration: `nixpkgs.overlays = [ umlcraft.overlays.default ];`, then
      # `environment.systemPackages = [ pkgs.umlcraft ];`.
      overlays.default = final: _: {
        umlcraft = umlcraftFor (final.extend rust-overlay.overlays.default);
      };

      # `nix develop`: Rust and the libraries GPUI builds and runs with, for `cargo run`.
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.umlcraft ];
          packages = [
            ((pkgs.rust-bin.stable."${rustVersion}.0".default).override {
              extensions = [
                "clippy"
                "rustfmt"
                "rust-analyzer"
                "rust-src"
              ];
            })
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
