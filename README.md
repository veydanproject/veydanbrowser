# Veydan Browser

> **Operations workspace for multi-account work** — isolated browser profiles, proxies, an SSH console, notes and 2FA tools, organized into workspaces in one local-first desktop app.

Veydan Browser is a desktop workspace for people who operate many accounts, proxies and servers in parallel. Everything the daily workflow needs lives in one place: **workspaces** with Kanban, table and topology views; **isolated browser profiles**; **proxy management** with one-click health checks; a full **SSH terminal** with saved connections, an **SSH key manager** and 2FA/TOTP auto-fill; a dual-pane **SFTP file manager**; **Markdown notes** with search and version history; and built-in **TOTP and password tools**.

Browser profiles are powered by [Camoufox](https://camoufox.com/) (a hardened, anti-fingerprinting Firefox fork): each profile is a fully separated identity — its own fingerprint, user agent, timezone, locale, WebRTC policy, geolocation, cookies and proxy — so accounts never cross-contaminate. That isolation is one feature of the toolkit, not the whole product.

Built with **Tauri 2** (Rust) and **Svelte 5 / SvelteKit** (TypeScript). Lightweight, native, cross-platform, and stores everything locally in SQLite — no cloud, no telemetry.

---

## Features

### 🗂 Workspaces
- Group profiles into **workspaces** with their own color, icon and notes.
- Multiple views: a **Kanban board** (columns driven by tags), a **table view**, and a **topology graph** of profiles and proxies.
- Per-profile tags, notes and launch tracking.

### 🎭 Isolated browser profiles
- **Per-profile fingerprinting** powered by Camoufox: spoofed canvas / audio / font seeds, WebGL vendor & renderer, screen metrics, and navigator properties.
- **OS presets** — Windows 10, Windows 11, macOS and Linux (Firefox), each with a matching user agent and platform (Windows/macOS presets also carry a default GPU).
- Configurable **user agent, platform, timezone, locale & languages, screen resolution, WebRTC policy (disable / proxy-only / real IP), and geolocation** (latitude/longitude).
- Per-profile **default search engine** (DuckDuckGo, Google, Brave, Startpage), **browsing-history toggle** (off = always-private mode), and a **color stripe** in the browser chrome so windows are easy to tell apart.
- **Auto-download of Camoufox** — the app fetches and manages the Camoufox binary for you, with live download progress and resume.
- **Cookie import/export** in EditThisCookie format — import from clipboard-style JSON, export straight from the profile's cookie store.
- **Profile export/import** as portable ZIP archives, optionally bundling the proxy (with or without its password) and the browser profile files.

### 🌐 Proxies
- Manage **HTTP / HTTPS / SOCKS5 / SSH** proxies with credentials, country & city labels, and tags.
- One-click **proxy check** — reports the exit IP, country and city.
- **Bulk import** — paste a list of proxies (`scheme://user:pass@host:port`, `host:port:user:pass` and similar line formats, IPv6 included) with per-line validation before anything is saved.
- **SSH proxies** with TOFU (trust-on-first-use) host-key fingerprinting: the server key is pinned on first connect and connections are blocked on mismatch. A local proxy bridge is spun up automatically for SSH tunnels.

### 🖥 SSH terminal & console
A full built-in SSH client with an interactive terminal (xterm.js) — not just a tunnel, but a complete console system:
- **Saved connections** — store named SSH hosts (host, port, user) locally, searchable by name / host / user, with per-connection settings.
- **Flexible auth** — password, OpenSSH private key (inline or from the key manager), or key + passphrase; keyboard-interactive is supported for servers that challenge you.
- **SSH key manager** — generate keys in-app (**Ed25519**, **RSA** 2048/3072/4096, **ECDSA** P-256/P-384/P-521) or import existing OpenSSH private keys, including passphrase-protected ones. Keys are stored with their comment and SHA-256 fingerprint and can be attached to any saved connection instead of pasting key material by hand.
- **2FA / TOTP auto-fill** — link a connection to a stored TOTP secret and the app answers keyboard-interactive challenges automatically: password prompts from the saved password, one-time codes generated on the fly. Anything it can't answer is relayed to an in-terminal prompt overlay so you can type it yourself.
- **Connect through a proxy or jump host** — route any session through a saved proxy: **SOCKS5**, **HTTP/HTTPS** (CONNECT tunnel), or another **SSH server as a jump host** (ProxyJump-style), with host-key TOFU on the jump.
- **Multiple concurrent sessions** — a session bar tracks every open terminal with live status (connecting / connected / error); switch between them instantly, and hidden sessions keep buffering output in the background.
- **Real terminal experience** — resizable / maximizable terminal drawer, a proper PTY (`xterm-256color`) with live resize forwarded to the server, clickable links, scrollback, and one-click reconnect.
- **Context-aware** — connections can be global or bound to specific workspaces and profiles, so the right hosts surface in the right place; open the console from the global view, a workspace, or a profile.
- Per-connection **default terminal size**, plus **connect timeout and keepalive** settings (honored by SFTP sessions).

### 📁 File manager (SFTP)
A dual-pane file manager for moving files between your machine and your servers:
- **Two panes, any direction** — each pane can browse the **local filesystem** or any saved SSH connection over **SFTP**; copy (`F5`) or move (`F6`) the selection to the other pane, `Tab` switches panes.
- **Robust transfers** — recursive folder upload/download with a live transfer strip (progress, speed, current file, cancel), **overwrite-or-skip** conflict handling, and **resume**: interrupted transfers continue from a partial file instead of restarting; completed files are moved into place atomically with permissions preserved.
- **Full file operations** — create files and folders, rename, delete, and a **permissions (chmod) dialog**; hidden-files toggle, sorting, multi-select, keyboard shortcuts (`F2`/`F5`/`F6`/`F7`/`Del`/`Ctrl+A`) and right-click context menus.
- **Same transport stack as the terminal** — connect directly or through a SOCKS5 / HTTP proxy or an SSH jump host, with TOFU host-key pinning; password and 2FA/TOTP prompts are answered from the saved connection or relayed to an in-app prompt.

### 📝 Notes
A full-featured Markdown note-taking system, not just a text box:
- **Markdown notes** stored as plain-text files on disk (Markdown content with a small frontmatter header), so they stay portable and readable outside the app — open a note's folder or edit it in an external editor at any time; external edits are picked up by a **file watcher** and re-synced live.
- **Full-text search** (SQLite FTS) across every note, backed by a manifest for fast indexing and re-indexing.
- **Version history** — saves snapshot the previous content (Zstd-compressed, throttled and change-gated so trivial re-saves don't pile up) with revision numbers. Browse past revisions, view a **line-by-line diff**, **restore** any version, or **three-way merge** changes.
- **Organization** — colored **tags**, **folders**, plus **pin** and **archive**.
- **Context bindings** — attach a note to a workspace or profile so relevant notes surface where you need them.
- **Crash-safe drafts** — in-progress edits are auto-saved and can be recovered (or discarded) after an unexpected close.
- **Configurable storage location** for the notes directory.

### 🔐 Built-in tools
- **TOTP / 2FA generator** — store raw Base32 secrets or import `otpauth://` URIs; SHA-1/SHA-256/SHA-512 with configurable digits and period; secrets stay in the backend and are never sent to the UI.
- **Password generator** with configurable rules.
- **Integrated terminal** (xterm.js) — see the **SSH terminal & console** section above for the full SSH client.

### 🎨 Experience
- **Bilingual UI** — English and Russian.
- Light/dark **theming** with a token-based design system.
- **Auto-updates** — the app checks for new releases and installs cryptographically signed updates in-place, with an update banner and live download progress. (On Linux `.deb`/`.rpm` installs the banner points to the new release instead — those are updated through the package manager.)
- Fast, native desktop windows via Tauri's WebView.

---

## Tech stack

| Layer      | Technology |
|------------|------------|
| Frontend   | Svelte 5 (SvelteKit 2), TypeScript, Vite 8 |
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
  lib/components/        UI: profiles, proxies, notes, SSH, files, TOTP, Kanban…
  lib/store/            Reactive state stores
  routes/               Pages: workspace, proxies, files, notes, terminal, settings
src-tauri/              Rust / Tauri backend
  src/browser/          Camoufox launch & profile handling
  src/commands/         Tauri commands (profiles, proxies, ssh, ssh_keys, sftp, totp, notes…)
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
