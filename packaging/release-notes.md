## Downloads

| System | File |
| --- | --- |
| Windows 10/11 (x64) | `UmlCraft-*-windows-x64.zip`: unpack and start `umlcraft.exe` |
| macOS 11 or later (Apple Silicon and Intel) | `UmlCraft-*-macos-universal.dmg`: drag UmlCraft into Applications |
| Linux (x64, glibc 2.35 or later) | `UmlCraft-*-linux-x64.tar.gz`: unpack and start `umlcraft` |

`SHA256SUMS.txt` lists the checksums of all files.

The builds are not signed yet:

- **Windows:** SmartScreen warns about an unknown publisher. Choose *More info* › *Run anyway*.
- **macOS:** Gatekeeper says the app is damaged or from an unidentified developer. After copying
  it to Applications, run `xattr -dr com.apple.quarantine /Applications/UmlCraft.app` once.
- **Linux:** needs Vulkan, xkbcommon, Wayland or X11 and Fontconfig, e.g. on Debian/Ubuntu
  `sudo apt install libvulkan1 libxkbcommon-x11-0 libwayland-client0 libfontconfig1`.

Exporting to Astah projects needs an installed Astah Professional, UML or SysML. On Windows it is
found under Program Files; elsewhere set `ASTAH_HOME` to its install folder.
