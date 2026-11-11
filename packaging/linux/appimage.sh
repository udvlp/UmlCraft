#!/usr/bin/env bash
# Builds the AppImage, e.g. `appimage.sh umlcraft dist/UmlCraft-0.5.0-linux-x64.AppImage`.
# Needs rsvg-convert, patchelf, appimagetool on the PATH, its runtime file in APPIMAGE_RUNTIME
# and the libraries listed in `bundled` below installed.
#
# The binary links xkbcommon, which not every system has (a Wayland-only desktop may lack
# libxkbcommon-x11), so these libraries come along; the binary finds them through its RUNPATH.
# The other libraries it needs (glibc, xcb, Vulkan, Wayland, FreeType, Fontconfig) are part of
# every graphical desktop and on the AppImage exclude list: bundled graphics libraries would
# clash with the system's drivers.
set -euo pipefail

binary=$1
output=$2
here=$(dirname "$0")
id=com.umlcraft.app
appdir="$(dirname "$output")/UmlCraft.AppDir"
bundled=(libxkbcommon.so.0 libxkbcommon-x11.so.0 libxcb-xkb.so.1)

rm -rf "$appdir"
bash "$here/install-desktop-files.sh" "$binary" "$appdir/usr"
mkdir -p "$appdir/usr/lib"
for lib in "${bundled[@]}"; do
    path=$(ldconfig -p | awk -v lib="$lib" '$1 == lib && /x86-64/ { print $NF; exit }')
    if [[ -z "$path" ]]; then
        echo "appimage.sh: $lib is not installed" >&2
        exit 1
    fi
    cp -L "$path" "$appdir/usr/lib/$lib"
    # A RUNPATH only serves the file's own dependencies: libxkbcommon-x11 finds libxcb-xkb
    # through its own.
    patchelf --set-rpath '$ORIGIN' "$appdir/usr/lib/$lib"
done
patchelf --set-rpath '$ORIGIN/../lib' "$appdir/usr/bin/umlcraft"
ln -s usr/bin/umlcraft "$appdir/AppRun"
ln -s "usr/share/applications/$id.desktop" "$appdir/$id.desktop"
ln -s "usr/share/icons/hicolor/256x256/apps/$id.png" "$appdir/$id.png"
ln -s "$id.png" "$appdir/.DirIcon"
# appimagetool looks for the metadata under its older name.
ln -s "$id.metainfo.xml" "$appdir/usr/share/metainfo/$id.appdata.xml"
appimagetool --runtime-file "$APPIMAGE_RUNTIME" "$appdir" "$output"
