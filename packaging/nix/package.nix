# UmlCraft for NixOS and Nix, built from source: the app `umlcraft`, the MCP server
# `umlcraft-cli` and the desktop integration (launcher, icon, AppStream metadata, file types).
# Used by `flake.nix`; on its own: `nix-build -E 'with import <nixpkgs> {}; callPackage ./packaging/nix/package.nix {}'`.
{
  lib,
  rustPlatform,
  pkg-config,
  fontconfig,
  freetype,
  libGL,
  libx11,
  libxcb,
  libxkbcommon,
  vulkan-loader,
  wayland,
}:

let
  cargoToml = lib.importTOML ../../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "umlcraft";
  inherit (cargoToml.workspace.package) version;

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../crates
      ../../locales
      ../../assets
      ../../packaging/linux
      # The Astah export compiles this Java tool with Astah's own Java when it runs.
      ../../tools/asta-export
    ];
  };

  cargoLock.lockFile = ../../Cargo.lock;

  cargoBuildFlags = [
    "-p"
    "umlcraft-app"
    "-p"
    "umlcraft-cli"
  ];

  nativeBuildInputs = [ pkg-config ];

  buildInputs = [
    fontconfig
    freetype
    libx11
    libxcb
    libxkbcommon
    wayland
  ];

  # The workspace tests run in CI; some of them need Astah's files, which the sandbox lacks.
  doCheck = false;

  postInstall = ''
    linux=packaging/linux
    id=com.umlcraft.app
    install -Dm644 $linux/$id.desktop $out/share/applications/$id.desktop
    install -Dm644 $linux/$id.metainfo.xml $out/share/metainfo/$id.metainfo.xml
    install -Dm644 $linux/$id.mime.xml $out/share/mime/packages/$id.xml
    install -Dm644 $linux/$id.svg $out/share/icons/hicolor/scalable/apps/$id.svg
  '';

  # GPUI loads Vulkan, Wayland and OpenGL at run time; NixOS has no global library path.
  postFixup = ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        libGL
        vulkan-loader
        wayland
      ]
    } $out/bin/umlcraft
  '';

  meta = {
    description = "UML modeling tool: class, sequence, activity and other diagrams, Astah projects, structograms";
    homepage = "https://github.com/udvlp/UmlCraft";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "umlcraft";
    platforms = lib.platforms.linux;
  };
}
