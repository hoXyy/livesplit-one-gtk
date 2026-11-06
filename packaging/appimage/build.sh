#!/usr/bin/env bash
#
# Builds an x86_64 AppImage into "$CARGO_TARGET_DIR/appimage" (default: target/appimage).
#
# Run this on Ubuntu 24.04, or in a container of it. The build system's glibc sets
# the oldest distro the AppImage can run on, and 24.04 is the oldest Ubuntu with
# GTK 4.14 and libadwaita 1.5. Packages needed:
#
#   libgtk-4-dev libadwaita-1-dev librsvg2-common adwaita-icon-theme dpkg-dev
#   file pkg-config curl
#
# Optional environment variables:
#   CARGO_PROFILE  Cargo profile to build (default: max-opt)
#   VERSION        Version in the output file name (default: from Cargo.toml)

set -euo pipefail

app_id=org.livesplit.LiveSplitOne
bin_name=livesplit-one-gtk

# Lets updaters such as Gear Lever find new builds. Gear Lever only looks at GitHub's
# latest release, so this has to say "latest" rather than a specific tag.
update_information='gh-releases-zsync|hoXyy|livesplit-one-gtk|latest|livesplit-one-gtk-*x86_64.AppImage.zsync'

# Pinned, because linuxdeploy's "continuous" release and the plugin's master branch
# can change at any time. The plugin hook patched below depends on this exact version.
linuxdeploy_url=https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
linuxdeploy_sha256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
gtk_plugin_url=https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/7a3fbc31a9e5075073ff8790f26effbac5f84453/linuxdeploy-plugin-gtk.sh
gtk_plugin_sha256=b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171

cd "$(dirname -- "${BASH_SOURCE[0]}")/../.."

profile="${CARGO_PROFILE:-max-opt}"
version="${VERSION:-$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)}"
target_dir="$(realpath -m "${CARGO_TARGET_DIR:-target}")"
work_dir="$target_dir/appimage"
tools_dir="$work_dir/tools"
app_dir="$work_dir/AppDir"
icons_dir="$work_dir/icons"
output="$work_dir/livesplit-one-gtk-$version-x86_64.AppImage"

fetch_tool() {
    local url="$1" sha256="$2" dest="$3"
    if ! echo "$sha256  $dest" | sha256sum --check --status 2>/dev/null; then
        curl --fail --location --silent --show-error --output "$dest" "$url"
        echo "$sha256  $dest" | sha256sum --check --quiet
    fi
    chmod +x "$dest"
}

mkdir -p "$tools_dir"
fetch_tool "$linuxdeploy_url" "$linuxdeploy_sha256" "$tools_dir/linuxdeploy-x86_64.AppImage"
# linuxdeploy finds plugins next to itself, by file name.
fetch_tool "$gtk_plugin_url" "$gtk_plugin_sha256" "$tools_dir/linuxdeploy-plugin-gtk.sh"

cargo build --locked --profile "$profile"

rm -rf "$app_dir" "$icons_dir" "$work_dir"/livesplit-one-gtk-*.AppImage*
mkdir -p "$icons_dir"
# linuxdeploy matches icons to the desktop file's Icon= entry by file name.
cp icons/icon.png "$icons_dir/$app_id.png"
cp icons/icon.svg "$icons_dir/$app_id.svg"

# linuxdeploy's own AppImage needs FUSE to run, which CI and containers don't have.
export APPIMAGE_EXTRACT_AND_RUN=1
export DEPLOY_GTK_VERSION=4
linuxdeploy="$tools_dir/linuxdeploy-x86_64.AppImage"

"$linuxdeploy" \
    --appdir "$app_dir" \
    --executable "$target_dir/$profile/$bin_name" \
    --desktop-file "packaging/$app_id.desktop" \
    --icon-file "$icons_dir/$app_id.png" \
    --icon-file "$icons_dir/$app_id.svg" \
    --plugin gtk

# The plugin doesn't bundle an icon theme, and the host might not have one. Cursors
# are left out because they make up much of the theme and come from the host anyway.
mkdir -p "$app_dir/usr/share/icons"
cp --archive /usr/share/icons/Adwaita "$app_dir/usr/share/icons/"
rm -rf "$app_dir/usr/share/icons/Adwaita/cursors"

# The plugin's hook forces X11 and a fixed theme. Forcing X11 would stop Wayland
# sessions from being detected, which is what selects the evdev hotkey path, and
# libadwaita follows the desktop's light/dark preference without any help.
hook="$app_dir/apprun-hooks/linuxdeploy-plugin-gtk.sh"
for pattern in '^COLOR_SCHEME=' '^APPIMAGE_GTK_THEME=' '^export GTK_THEME=' '^export GDK_BACKEND='; do
    grep --quiet "$pattern" "$hook" || {
        echo "error: '$pattern' not found in the GTK plugin's hook" >&2
        exit 1
    }
done
sed -i \
    -e '/^COLOR_SCHEME=/,/^APPIMAGE_GTK_THEME=/d' \
    -e '/^export GTK_THEME=/d' \
    -e '/^export GDK_BACKEND=/d' \
    "$hook"

cat >>"$hook" <<'EOF'
# Don't load the host's GIO modules, which are built against the host's GLib.
export GIO_MODULE_DIR="$APPDIR/usr/lib/gio/modules"
# Use the desktop's file dialogs when it provides them.
export GDK_DEBUG="${GDK_DEBUG:+$GDK_DEBUG,}portals"
EOF

# The .zsync file is written to the current directory, not next to the output.
cd "$work_dir"
OUTPUT="$output" LDAI_OUTPUT="$output" \
    LDAI_UPDATE_INFORMATION="$update_information" \
    "$linuxdeploy" --appdir "$app_dir" --output appimage

echo "Built $output"
