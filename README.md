# Veydan Browser

> **Anti-detect browser & fingerprint profile manager** — run dozens of isolated browser identities, each with its own fingerprint, proxy and cookies.

Veydan Browser is a desktop application for creating and managing isolated browser profiles built on top of [Camoufox](https://camoufox.com/) (a hardened, anti-fingerprinting Firefox fork). Each profile is a fully separated identity — its own fingerprint (canvas, WebGL, audio, fonts), user agent, timezone, locale, screen resolution, WebRTC policy, geolocation and proxy — so you can work with many accounts in parallel without cross-contamination.

Built with **Tauri 2** (Rust) and **SvelteKit 5** (TypeScript). Lightweight, native, cross-platform, and stores everything locally in SQLite — no cloud, no telemetry.

---

## Features

### 🎭 Profiles & fingerprints
- **Per-profile fingerprinting** powered by Camoufox: spoofed canvas / audio / font seeds, WebGL vendor & renderer, screen metrics, and navigator properties.
- **OS presets** — Windows 10, Windows 11, macOS and Linux (Firefox), each with a matching user agent, platform and default GPU.
- Configurable **user agent, platform, timezone, locale & languages, screen resolution, WebRTC mode, and geolocation** (latitude/longitude).
- **Auto-download of Camoufox** — the app fetches and manages the Camoufox binary for you, with live download progress.
- **Cookie import/export** in EditThisCookie / antidetect-browser format (supports `mozLz4`-packed cookie stores).
- **Profile export/import** as portable archives (optionally bundling proxy and profile files, compressed with LZ4/Zstd).

### 🗂 Workspaces
- Group profiles into **workspaces** with their own color, icon and notes.
- Multiple views: a **Kanban board** (columns driven by tags), a **table view**, and a **topology graph** of profiles and proxies.
- Per-profile tags, notes and launch tracking.

### 🌐 Proxies
- Manage **HTTP / HTTPS / SOCKS5 / SSH** proxies with credentials, country & city labels, and tags.
- One-click **proxy check** — reports the exit IP, country and city.
- **SSH proxies** with TOFU (trust-on-first-use) host-key fingerprinting: the server key is pinned on first connect and connections are blocked on mismatch. A local proxy bridge is spun up automatically for SSH tunnels.

### 🖥 SSH terminal & console
A full built-in SSH client with an interactive terminal (xterm.js) — not just a tunnel, but a complete console system:
- **Saved connections** — store named SSH hosts (host, port, user) locally, searchable by name / host / user, with per-connection settings.
- **Flexible auth** — password, OpenSSH private key, or key + passphrase; keyboard-interactive is supported for servers that challenge you.
- **2FA / TOTP auto-fill** — link a connection to a stored TOTP secret and the app answers keyboard-interactive challenges automatically: password prompts from the saved password, one-time codes generated on the fly. Anything it can't answer is relayed to an in-terminal prompt overlay so you can type it yourself.
- **Connect through a proxy or jump host** — route any session through a saved proxy: **SOCKS5**, **HTTP/HTTPS** (CONNECT tunnel), or another **SSH server as a jump host** (ProxyJump-style), with host-key TOFU on the jump.
- **Multiple concurrent sessions** — a session bar tracks every open terminal with live status (connecting / connected / error); switch between them instantly, and hidden sessions keep buffering output in the background.
- **Real terminal experience** — resizable / maximizable terminal drawer, a proper PTY (`xterm-256color`) with live resize forwarded to the server, clickable links, scrollback, and one-click reconnect.
- **Context-aware** — connections can be global or bound to specific workspaces and profiles, so the right hosts surface in the right place; open the console from the global view, a workspace, or a profile.
- Per-connection **connect timeout, keepalive, default terminal size and theme**.

### 📝 Notes
A full-featured Markdown note-taking system, not just a text box:
- **Markdown notes** stored as plain `.md` files on disk (with YAML frontmatter), so they stay portable and readable outside the app — open a note's folder or edit it in an external editor at any time.
- **Full-text search** (SQLite FTS) across every note, backed by a manifest for fast indexing and re-indexing.
- **Version history** — every save snapshots the previous content (Zstd-compressed) with revision numbers. Browse past revisions, view a **line-by-line diff**, **restore** any version, or **three-way merge** changes.
- **Organization** — colored **tags**, **folders**, plus **pin** and **archive**.
- **Context bindings** — attach a note to a workspace, profile or proxy so relevant notes surface where you need them.
- **Crash-safe drafts** — in-progress edits are auto-saved and can be recovered (or discarded) after an unexpected close.
- **Configurable storage location** for the notes directory.

### 🔐 Built-in tools
- **TOTP / 2FA generator** — store `otpauth` secrets and generate time-based codes.
- **Password generator** with configurable rules.
- **Integrated terminal** (xterm.js) — see the **SSH terminal & console** section above for the full SSH client.

### 🎨 Experience
- **Bilingual UI** — English and Russian.
- Light/dark **theming** with a token-based design system.
- Fast, native desktop windows via Tauri's WebView.

---

## Tech stack

| Layer      | Technology |
|------------|------------|
| Frontend   | SvelteKit 5, TypeScript, Vite 8 |
| Backend    | Rust, Tauri 2 |
| Database   | SQLite (`sqlx` + `rusqlite`) |
| Browser    | Camoufox (anti-fingerprint Firefox) |
| Terminal   | xterm.js |

---

## Getting started

### Prerequisites
- **Rust** (stable) and **Node.js** with **pnpm**
- Linux desktop dependencies: WebKitGTK 4.1, GTK 3, libsoup 3 (the provided `dev.sh` can bootstrap a self-contained toolchain and system libraries into the project directory without root)

### Run in development
```bash
pnpm install
make dev          # or: bash dev.sh
```

### Build a release
```bash
pnpm tauri build
```
This produces native bundles (`.deb`, `.rpm`, and more) under `src-tauri/target`.

### Handy commands
```bash
make dev          # start the dev build
make clean        # stop running instances and remove build artifacts
make update       # run the update script
```

---

## Project structure

```
src/                    SvelteKit frontend
  lib/components/        UI: profiles, proxies, notes, SSH, TOTP, Kanban…
  lib/store/            Reactive state stores
  routes/               Pages: workspaces, proxies, notes, terminal, settings
src-tauri/              Rust / Tauri backend
  src/browser/          Camoufox launch & profile handling
  src/commands/         Tauri commands (profiles, proxies, ssh, totp, notes…)
  src/proxy/            HTTP/SOCKS/SSH proxy logic & checks
  src/fingerprint.rs    Fingerprint presets
```

---

## License

Copyright © 2026 **Veydan Project**.

Veydan Browser is **source-available** software, licensed under the
[PolyForm Perimeter License 1.0.1](https://polyformproject.org/licenses/perimeter/1.0.1).

- ✅ Free to read, build, run, modify and redistribute for almost any purpose.
- ❌ You may **not** use it to provide a product that competes with Veydan Browser.

This is not OSI "open source" — it is open and free with a no-compete boundary.
See the [`LICENSE`](LICENSE) file for the full, legally binding terms, or
[`LICENSE-SUMMARY.md`](LICENSE-SUMMARY.md) for a plain-language summary in
English, German, French, Chinese and Russian. Third-party dependency licenses
are listed in [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md).
