#!/usr/bin/env bash
# Builds Lapacho and installs it into the user's home — binary, icons and
# desktop entry. No root, no packaging: everything lands under ~/.local.
#
# This exists because the pieces drifted apart once already: the icons were
# regenerated in the repo but the copies under ~/.local/share/icons stayed on
# the old artwork, so the tray showed one logo and the launcher another. One
# script means "installed" can't mean three different vintages at once.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
icons="$repo/apps/desktop/src-tauri/icons"
bin_dir="$HOME/.local/bin"
icon_dir="$HOME/.local/share/icons/hicolor"
apps_dir="$HOME/.local/share/applications"

# `cargo tauri build`, not `cargo build`: the frontend is embedded at compile
# time, so a plain cargo build would ship whatever stale WASM is in ui/dist.
# The env scrubbing is trunk's colour-detection quirk (see README).
echo "==> building (release)"
( cd "$repo/apps/desktop/src-tauri" \
  && env -u NO_COLOR -u CARGO_TERM_COLOR TRUNK_COLOR=always CARGO_TERM_COLOR=never \
     cargo tauri build )

echo "==> installing binary"
mkdir -p "$bin_dir"
# Unlink before copying: overwriting a running executable in place fails with
# ETXTBSY, and this script is most useful while the old version is still up.
# The running process keeps its own inode and dies happy at the next restart.
rm -f "$bin_dir/lapacho"
install -m755 "$repo/target/release/lapacho-desktop" "$bin_dir/lapacho"

echo "==> installing icons"
install -Dm644 "$icons/32x32.png"       "$icon_dir/32x32/apps/lapacho.png"
install -Dm644 "$icons/128x128.png"     "$icon_dir/128x128/apps/lapacho.png"
install -Dm644 "$icons/128x128@2x.png"  "$icon_dir/256x256/apps/lapacho.png"
install -Dm644 "$icons/lapacho-source.svg" "$icon_dir/scalable/apps/lapacho.svg"

echo "==> installing desktop entry"
mkdir -p "$apps_dir"
# WEBKIT_DISABLE_DMABUF_RENDERER=1 works around WebKitGTK rendering a blank
# window under some compositors; drop it if your setup doesn't need it.
#
# The redirect is not cosmetic. Launched from autostart there is no terminal,
# so every diagnostic the app writes — failed clipboard access, keyring
# fallback, tray rebuild errors, the latency instrumentation — went to a
# stderr nobody could read. A crash at startup left no trace at all. The
# previous run is kept as .log.1, because the run you need to read is usually
# the one that just died.
#
# Safe to persist: no eprintln! in this codebase prints clipboard content,
# only errors, content types and timings. Keep it that way — this file is on
# disk, unencrypted, which is exactly what the buffer is not allowed to be.
#
# Set LAPACHO_TRACE=1 here to add the per-copy latency lines; off by default
# so the log stays small enough to actually read.
cat > "$apps_dir/lapacho.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Lapacho
Comment=Secure clipboard manager (Quebracho Digital)
Exec=sh -c 'l="${XDG_STATE_HOME:-$HOME/.local/state}/lapacho"; mkdir -p "$l"; [ -f "$l/lapacho.log" ] && mv -f "$l/lapacho.log" "$l/lapacho.log.1"; exec env WEBKIT_DISABLE_DMABUF_RENDERER=1 lapacho >"$l/lapacho.log" 2>&1'
Icon=lapacho
Terminal=false
Categories=Utility;Security;
Keywords=clipboard;security;paste;
StartupNotify=false
X-GNOME-Autostart-enabled=true
EOF

# Without these the desktop keeps serving the previous icon from cache — the
# exact failure this script was written to stop repeating.
gtk-update-icon-cache -f -t "$icon_dir" >/dev/null 2>&1 || true
update-desktop-database "$apps_dir" >/dev/null 2>&1 || true

echo "==> done. Restart a running Lapacho to pick up the new binary."
echo "    log: ${XDG_STATE_HOME:-$HOME/.local/state}/lapacho/lapacho.log (previous run: .log.1)"
echo "    timings: add LAPACHO_TRACE=1 to the Exec line, or run 'LAPACHO_TRACE=1 lapacho' from a shell"
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "    note: $bin_dir is not on your PATH, so the desktop entry won't find it." ;;
esac
