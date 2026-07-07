#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export SCRIPT_DIR

# One-time .dev-prefix setup + build env vars + project-local toolchain.
source "$SCRIPT_DIR/build-env.sh"

# Free port 1420 if a stale process holds it
fuser -k 1420/tcp 2>/dev/null || true

# --- Taskbar icon for the dev build (Wayland/KDE, GNOME, …) ------------------
# Wayland compositors don't read a window's embedded icon like X11 did; they
# resolve the taskbar icon purely by matching the window's app_id to an
# installed .desktop file. `tauri dev` installs nothing, so the app_id
# (`veydanbrowser`, = the binary basename) matches no desktop file and the DE
# falls back to a generated letter-avatar placeholder instead of our logo.
# Drop a desktop file + icons into the user's XDG dirs so the logo resolves.
# Idempotent and best-effort — never abort the dev run over it.
install_dev_icon() {
  local app_id="veydanbrowser"
  local apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
  local icons_base="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"
  local src="$SCRIPT_DIR/src-tauri/icons"

  mkdir -p "$apps_dir"
  # size -> source png (hicolor needs each size in its own dir)
  local map=(
    "32x32:$src/32x32.png"
    "64x64:$src/64x64.png"
    "128x128:$src/128x128.png"
    "256x256:$src/128x128@2x.png"
    "512x512:$src/icon.png"
  )
  local entry size file
  for entry in "${map[@]}"; do
    size="${entry%%:*}"; file="${entry#*:}"
    [ -f "$file" ] || continue
    mkdir -p "$icons_base/$size/apps"
    cp -f "$file" "$icons_base/$size/apps/$app_id.png"
  done

  cat > "$apps_dir/$app_id.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Veydan Browser
Comment=Multi-Accounting Workspace (dev)
Exec=$SCRIPT_DIR/dev.sh
Icon=$app_id
Terminal=false
Categories=Network;WebBrowser;Security;
StartupNotify=true
StartupWMClass=$app_id
NoDisplay=true
EOF

  # Refresh caches where the tools exist (KDE reads live, GNOME likes a nudge)
  command -v update-desktop-database >/dev/null 2>&1 && \
    update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -q -t -f "$icons_base" >/dev/null 2>&1 || true
}
install_dev_icon || true

exec pnpm tauri dev
