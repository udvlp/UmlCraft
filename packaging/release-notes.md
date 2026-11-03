## Downloads

| System | File |
| --- | --- |
| Windows 10/11 (x64) | `UmlCraft-*-windows-x64-setup.exe`: installs for you alone without administrator rights, or for all users, and opens `.ucraft` files with UmlCraft |
| Windows 10/11 (x64), without installing | `UmlCraft-*-windows-x64.zip`: unpack and start `umlcraft.exe` |
| macOS 11 or later (Apple Silicon and Intel) | `UmlCraft-*-macos-universal.dmg`: drag UmlCraft into Applications |
| Debian, Ubuntu (x64) | `UmlCraft-*-linux-x64.deb`: `sudo apt install ./UmlCraft-*-linux-x64.deb` |
| Fedora, openSUSE (x64) | `UmlCraft-*-linux-x64.rpm`: `sudo dnf install ./UmlCraft-*-linux-x64.rpm` or `sudo zypper install ./UmlCraft-*-linux-x64.rpm` |
| Linux (x64), any distribution | `UmlCraft-*-linux-x64.AppImage`: make it executable and start it |
| Linux (x64), Flatpak | `UmlCraft-*-linux-x64.flatpak`: `flatpak install --user ./UmlCraft-*-linux-x64.flatpak` |
| Linux (x64), Snap | `UmlCraft-*-linux-x64.snap`: `sudo snap install --dangerous ./UmlCraft-*-linux-x64.snap` |
| Linux (x64, glibc 2.35 or later) | `UmlCraft-*-linux-x64.tar.gz`: unpack and start `umlcraft` |
| NixOS (x64), built from source | no file: `nix run github:udvlp/UmlCraft/v@VERSION@` or `nix profile install github:udvlp/UmlCraft/v@VERSION@`, or the flake as an input of your NixOS configuration, with its `overlays.default` and `pkgs.umlcraft` |

`SHA256SUMS.txt` lists the checksums of all files.

The builds are not signed yet:

- **Windows:** SmartScreen warns about an unknown publisher. Choose *More info* › *Run anyway*.
- **macOS:** Gatekeeper says the app is damaged or from an unidentified developer. After copying
  it to Applications, run `xattr -dr com.apple.quarantine /Applications/UmlCraft.app` once.
- **Linux:** the `.deb` and `.rpm` bring the libraries UmlCraft needs, Flatpak and Snap bring
  their own. The AppImage and the `.tar.gz` need Vulkan, xkbcommon, Wayland or X11 and
  Fontconfig, e.g. on Debian/Ubuntu
  `sudo apt install libvulkan1 libxkbcommon-x11-0 libwayland-client0 libfontconfig1`.
  The Snap is not in the Snap Store yet, hence `--dangerous`; Flatpak and Snap do not update
  themselves.
- **NixOS:** Nix builds UmlCraft from this release's source; a newer release comes with its own
  tag, the update button opens its page. It needs a Vulkan driver
  (`hardware.graphics.enable = true`).

Exporting to Astah projects needs an installed Astah Professional, UML or SysML. On Windows it is
found under Program Files; elsewhere set `ASTAH_HOME` to its install folder. The Flatpak and the
Snap cannot start Astah from their sandbox; for the export use one of the other Linux packages.
