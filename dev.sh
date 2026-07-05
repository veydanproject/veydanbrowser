#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export SCRIPT_DIR
DEV_PREFIX="$SCRIPT_DIR/.dev-prefix"
LIBDIR="$DEV_PREFIX/usr/lib/x86_64-linux-gnu"
PKG_DIR="$LIBDIR/pkgconfig"

# One-time setup: extract deb packages into .dev-prefix/ inside the project.
# We fetch both the -dev packages (headers + .pc files, needed to build) and
# the webkit runtime packages (the .so.0 shared objects, needed to run) so the
# project is fully self-contained even on hosts where WebKitGTK isn't installed.
if [ ! -f "$PKG_DIR/webkit2gtk-4.1.pc" ]; then
  echo ">> First-time setup: extracting dev packages into .dev-prefix/ ..."
  mkdir -p "$DEV_PREFIX" /tmp/_rb_debs

  # Root -dev packages the build links against. We resolve their full recursive
  # -dev dependency closure below so every header + .pc file in the GTK/WebKit
  # stack lands in the prefix (on Debian trixie these are split across many
  # packages, e.g. glib's .pc moved into libgio-2.0-dev).
  dev_roots=(
    libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev
    libsoup-3.0-dev libgtk-3-dev librsvg2-dev
    libayatana-appindicator3-dev
    libsqlite3-dev libpsl-dev libkrb5-dev libnghttp2-dev
    libbrotli-dev libsysprof-capture-4-dev
    libssl-dev libdbus-1-dev libsystemd-dev
  )

  # Runtime packages: shared objects not part of a typical desktop base install
  # (WebKit + its private deps). Everything else (GTK, cairo, glib, openssl, …)
  # already ships as runtime on the host; we only add the missing .so symlinks.
  runtime_pkgs=(
    libwebkit2gtk-4.1-0 libjavascriptcoregtk-4.1-0
    libmanette-0.2-0 libenchant-2-2 libhidapi-hidraw0
  )

  echo ">> Resolving recursive -dev dependency closure ..."
  dev_closure=$(apt-cache depends --recurse --no-recommends --no-suggests \
    --no-conflicts --no-breaks --no-replaces --no-enhances "${dev_roots[@]}" \
    2>/dev/null | grep -E '^\w' | grep -E '\-dev$' | sort -u)

  cd /tmp/_rb_debs
  echo ">> Downloading packages ..."
  apt-get download $dev_closure "${runtime_pkgs[@]}"
  for f in *.deb; do
    dpkg-deb -x "$f" "$DEV_PREFIX/"
  done
  cd "$SCRIPT_DIR"

  # krb5-gssapi.pc is a dangling symlink in the deb — create the real file
  mkdir -p "$PKG_DIR/mit-krb5"
  cat > "$PKG_DIR/mit-krb5/krb5-gssapi.pc" << 'EOF'
prefix=/usr
libdir=${prefix}/lib/x86_64-linux-gnu
includedir=${prefix}/include
Name: krb5-gssapi
Description: Kerberos 5 GSSAPI
Version: 1.21.3
Libs: -L${libdir} -lgssapi_krb5
Cflags: -I${includedir}
EOF

  # The linker needs bare lib*.so files. The -dev debs ship them as relative
  # symlinks (lib*.so -> lib*.so.N), but for libs whose runtime lives on the
  # host the target .so.N isn't in the prefix. Repoint every such dangling
  # symlink at the host's runtime copy so -L$LIBDIR resolves it.
  for l in "$LIBDIR"/*.so; do
    [ -L "$l" ] || continue
    base=$(basename "$(readlink "$l")")
    if [ ! -e "$LIBDIR/$base" ] && [ -e "/usr/lib/x86_64-linux-gnu/$base" ]; then
      ln -sf "/usr/lib/x86_64-linux-gnu/$base" "$l"
    fi
  done

  rm -rf /tmp/_rb_debs
  echo ">> Setup complete."
fi

# Build environment
export PKG_CONFIG_PATH="$PKG_DIR:$DEV_PREFIX/usr/share/pkgconfig:/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig"
# Include both the plain and the multiarch header dirs — Debian ships some
# arch-specific headers (e.g. openssl/opensslconf.h) under x86_64-linux-gnu/.
export CFLAGS="-I$DEV_PREFIX/usr/include -I$DEV_PREFIX/usr/include/x86_64-linux-gnu"
export CXXFLAGS="$CFLAGS"
export RUSTFLAGS="-L $DEV_PREFIX/usr/lib/x86_64-linux-gnu -L /usr/lib/x86_64-linux-gnu"
# Runtime: let the loader find the WebKit .so.0 we extracted into the prefix
export LD_LIBRARY_PATH="$LIBDIR:$LD_LIBRARY_PATH"
# WebKit spawns helper binaries (WebKitNetworkProcess, WebKitWebProcess, …) from
# a /usr path compiled into the library with no env override. We run the app in
# a bubblewrap sandbox that bind-mounts our prefix copy onto that path — wired
# in as cargo's "runner" so only the app (not the build) is sandboxed.
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="$SCRIPT_DIR/webkit-run.sh"
# Don't let WebKit start its own (nested) bwrap sandbox inside ours.
export WEBKIT_DISABLE_SANDBOX=1
# OpenSSL: use the headers from the prefix and the host's runtime .so, and
# skip the vendored build. Setting both dirs makes openssl-sys skip pkg-config
# (whose prefix=/usr would otherwise point at non-existent system headers).
export OPENSSL_LIB_DIR="$LIBDIR"
export OPENSSL_INCLUDE_DIR="$DEV_PREFIX/usr/include"
export OPENSSL_NO_VENDOR=1

# Project-local Rust + Node + pnpm (installs into .toolchains/ on first run)
source "$SCRIPT_DIR/toolchain.sh"

cd "$SCRIPT_DIR"

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
