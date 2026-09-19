<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import '@fontsource-variable/manrope/index.css';
  import '@fontsource-variable/jetbrains-mono/index.css';
  import '$lib/styles/tokens.css';
  import '$lib/styles/base.css';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import type { Snippet } from 'svelte';
  import { t, locale } from '$lib/i18n';
  import type { TrayLabels } from '$lib/api';
  import { theme, toggleTheme } from '$lib/theme';
  import Icon from '$lib/Icon.svelte';
  import { api, isNotesWindow, windowLabel } from '$lib/api';
  import type { Profile } from '$lib/types';
  import PasswordGenerator from '$lib/components/PasswordGenerator.svelte';
  import TotpGenerator from '$lib/components/TotpGenerator.svelte';
  import SSHSessionBar from '$lib/components/ssh/SSHSessionBar.svelte';
  import SSHTerminal from '$lib/components/ssh/SSHTerminal.svelte';
  import { sshStore } from '$lib/store/ssh.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { updaterStore } from '$lib/store/updater.svelte';
  import UpdateBanner from '$lib/components/UpdateBanner.svelte';
  import { listen } from '@tauri-apps/api/event';
  import type { Window } from '@tauri-apps/api/window';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import UIInspector from '$lib/inspector/UIInspector.svelte';
  import { inspectorApp } from '$lib/inspector/inspector.svelte';
  import WindowControls from '$lib/components/WindowControls.svelte';
  import ResizeHandles from '$lib/components/ResizeHandles.svelte';

  let { children }: { children: Snippet } = $props();

  let runningIds = $state<string[]>([]);
  let pwgenOpen = $state(false);
  let totpOpen = $state(false);

  let runningProfiles = $derived(
    profilesStore.list.filter((p) => runningIds.includes(p.id))
  );

  // Terminal bottom offset = all in-flow elements below it
  const ITEM_H = 36;
  let terminalBottom = $derived(
    ITEM_H + // SSH session bar (when sessions exist — terminal only shows when sessions exist)
    (runningProfiles.length > 0 ? ITEM_H : 0) // dock
  );

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const standaloneNotes = isNotesWindow();

  // Client-side decorations are used only on Linux (to dodge the KWin
  // hide()/show() decoration bug). Windows/macOS keep their native window
  // chrome, so the custom titlebar/controls/resize/frame render only here.
  const isCsd =
    isTauri && typeof navigator !== 'undefined' && /linux/i.test(navigator.userAgent);

  // Custom-titlebar (CSD) dragging: cache the window so startDragging() fires
  // synchronously inside the mousedown (Wayland needs the live grab).
  let appWindow: Window | null = null;
  let maximized = $state(false);
  const NON_DRAG = 'button, a, input, select, textarea, [data-no-drag]';

  function onTitlebarMouseDown(e: MouseEvent) {
    if (!isCsd || e.button !== 0 || !appWindow) return;
    if ((e.target as HTMLElement).closest(NON_DRAG)) return;
    appWindow.startDragging();
  }
  function onTitlebarDblClick(e: MouseEvent) {
    if (!isCsd || !appWindow) return;
    if ((e.target as HTMLElement).closest(NON_DRAG)) return;
    appWindow.toggleMaximize();
  }

  async function refreshRunning() {
    try {
      runningIds = await api.profiles.runningIds();
    } catch {}
  }

  // Build the tray menu strings for the active locale. Parametrized entries
  // ({n}) are passed as raw templates — the backend fills in the count.
  function buildTrayLabels(): TrayLabels {
    const tt = get(t);
    return {
      show: tt('tray_show'),
      hide: tt('tray_hide'),
      quit: tt('tray_quit'),
      running: tt('tray_running'),
      stop_all: tt('tray_stop_all'),
      no_running: tt('tray_no_running'),
      launch_profile: tt('tray_launch_profile'),
      no_profiles: tt('tray_no_profiles'),
      section_workspaces: tt('nav_workspaces'),
      section_proxies: tt('nav_proxies'),
      section_terminal: tt('nav_terminal'),
      section_files: tt('nav_files'),
      section_notes: tt('nav_notes'),
      password_generator: tt('tray_password_generator'),
      quick_capture: tt('tray_quick_capture'),
      tooltip: tt('tray_tooltip'),
    };
  }

  // Tray menu and browser extension follow the app language
  function syncTrayLabels() {
    if (!isTauri) return;
    api.settings.setTrayLabels(buildTrayLabels()).catch(() => {});
    api.settings.setLocale(get(locale)).catch(() => {});
  }

  function handleKeyBack(e: KeyboardEvent) {
    if (e.ctrlKey && e.shiftKey && e.key === 'D') {
      e.preventDefault();
      inspectorApp.toggle();
      return;
    }
    if (e.altKey && e.key === 'ArrowLeft') {
      e.preventDefault();
      window.history.back();
    }
  }

  function handleMouseBack(e: MouseEvent) {
    if (e.button === 3) {
      e.preventDefault();
      window.history.back();
    }
  }

  onMount(() => {
    const unsub = theme.subscribe((val) => {
      document.body.dataset.theme = val;
    });

    // Cache the window handle for titlebar dragging (CSD) + track maximized so
    // the rounded frame + shadow are dropped when the window fills the screen.
    let unlistenMax: (() => void) | undefined;
    if (isTauri) {
      import('@tauri-apps/api/window')
        .then(async ({ getCurrentWindow }) => {
          const win = getCurrentWindow();
          appWindow = win;
          try {
            maximized = await win.isMaximized();
            unlistenMax = await win.onResized(async () => {
              try { maximized = await win.isMaximized(); } catch {}
            });
          } catch {}
        })
        .catch(() => {});
    }

    // Notes popout: only theme + window chrome. Main window owns tray / dock / updater.
    if (!standaloneNotes) {
      profilesStore.ensureLoaded();
      totpStore.ensureLoaded();
      sshStore.ensureLoaded();
      refreshRunning();
      window.addEventListener('focus', refreshRunning);
    }

    // Native back button support for WebKitGTK (Tauri on Linux)
    window.addEventListener('keydown', handleKeyBack);
    window.addEventListener('mouseup', handleMouseBack);

    const unlisten = standaloneNotes
      ? Promise.resolve(() => {})
      : listen<{ running_ids: string[] }>('profiles://running-changed', (e) => {
          runningIds = e.payload.running_ids;
        });

    const unsubLocale = standaloneNotes
      ? () => {}
      : locale.subscribe(() => syncTrayLabels());
    if (!standaloneNotes) syncTrayLabels();

    const trayUnlisteners = standaloneNotes
      ? []
      : [
          listen<string>('tray://navigate', (e) => goto(e.payload)),
          listen<string>('tray://launch-profile', (e) => {
            api.profiles.launch(e.payload).catch((err) => console.error(err));
          }),
          listen<string>('tray://stop-profile', (e) => {
            api.profiles.stop(e.payload).catch((err) => console.error(err));
          }),
          listen('tray://stop-all', () => {
            runningIds.forEach((id) => api.profiles.stop(id).catch(() => {}));
          }),
          listen('tray://open-pwgen', () => { pwgenOpen = true; }),
        ];

    let updateTimer: ReturnType<typeof setTimeout> | undefined;
    if (!standaloneNotes) {
      updaterStore.init();
      updateTimer = setTimeout(() => updaterStore.check(true), 5000);
    }

    return () => {
      unsub();
      if (updateTimer) clearTimeout(updateTimer);
      window.removeEventListener('focus', refreshRunning);
      window.removeEventListener('keydown', handleKeyBack);
      window.removeEventListener('mouseup', handleMouseBack);
      unlisten.then((fn) => fn());
      unsubLocale();
      trayUnlisteners.forEach((p) => p.then((fn) => fn()));
      unlistenMax?.();
    };
  });

  function isActive(href: string) {
    if (href === '/') return $page.url.pathname === '/';
    return $page.url.pathname.startsWith(href);
  }

  function getWorkspaceHref(p: Profile) {
    return p.workspace_id ? `/workspace/${p.workspace_id}` : '/';
  }

  const notesFullWidth = $derived(
    standaloneNotes || $page.url.pathname.startsWith('/notes')
  );
</script>

<div class="app-frame" class:csd={isCsd} class:maximized={maximized}>
 <div class="layout">
  {#if isCsd}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="titlebar">
      <div class="titlebar-title">
        <img src="/logo.png" alt="" class="titlebar-logo" />
        <span>{standaloneNotes ? $t(windowLabel() === 'quick-capture' ? 'quick_capture_title' : 'nav_notes') : 'Veydan Browser'}</span>
      </div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="titlebar-drag" onmousedown={onTitlebarMouseDown} ondblclick={onTitlebarDblClick}></div>
      {#if !standaloneNotes}
        <button
          class="titlebar-btn"
          onclick={() => api.notes.openWindow($t('nav_notes'))}
          aria-label={$t('notes_btn_open_window')}
          title={$t('notes_btn_open_window')}
        >
          <Icon name="file-text" size={15} />
        </button>
        <button
          class="titlebar-btn"
          class:active={isActive('/settings')}
          onclick={() => goto('/settings')}
          aria-label={$t('nav_settings')}
          title={$t('nav_settings')}
        >
          <Icon name="settings" size={15} />
        </button>
      {/if}
      <WindowControls />
    </div>
  {/if}
  {#if !standaloneNotes}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header class="topbar">
     <div class="topbar-inner">
      <a href="/" class="topbar-brand">
        <span class="brand-tile">
          <img src="/logo.png" alt="Veydan Browser" class="brand-logo" />
        </span>
        <span class="brand-name">Veydan Browser</span>
      </a>

      <nav class="topbar-nav">
        <a href="/" class="nav-link" class:active={isActive('/')}>
          <Icon name="layers" size={14} />
          {$t('nav_workspaces')}
        </a>
        <a href="/proxies" class="nav-link" class:active={isActive('/proxies')}>
          <Icon name="globe" size={14} />
          {$t('nav_proxies')}
        </a>
        <a href="/terminal" class="nav-link" class:active={isActive('/terminal')}>
          <Icon name="terminal" size={14} />
          {$t('nav_terminal')}
        </a>
        <a href="/files" class="nav-link" class:active={isActive('/files')}>
          <Icon name="folder" size={14} />
          {$t('nav_files')}
        </a>
        <a href="/notes" class="nav-link" class:active={isActive('/notes')}>
          <Icon name="file-text" size={14} />
          {$t('nav_notes')}
        </a>
      </nav>

      <div class="topbar-right">
        {#if !isCsd}
          <button
            class="theme-toggle"
            onclick={() => api.notes.openWindow($t('nav_notes'))}
            title={$t('notes_btn_open_window')}
          >
            <Icon name="file-text" size={15} />
          </button>
          <button class="theme-toggle" onclick={() => goto('/settings')} title={$t('nav_settings')}>
            <Icon name="settings" size={15} />
          </button>
        {/if}
        <button class="theme-toggle" onclick={() => (totpOpen = !totpOpen)} title={$t('totp_title')}>
          <Icon name="shield" size={15} />
        </button>
        <button class="theme-toggle" onclick={() => (pwgenOpen = !pwgenOpen)} title={$t('pwgen_title')}>
          <Icon name="key" size={15} />
        </button>
        <button class="theme-toggle" onclick={toggleTheme} title={$t('theme_toggle')}>
          {#if $theme === 'dark'}
            <Icon name="sun" size={15} />
          {:else}
            <Icon name="moon" size={15} />
          {/if}
        </button>
      </div>
     </div>
    </header>

    <UpdateBanner />
  {/if}

  <main class="content" class:content--notes={notesFullWidth}>
    {@render children()}
  </main>

  {#if !standaloneNotes}
    <!-- Slot clips terminal animation — overflow:hidden prevents sliding through dock/session bar -->
    <div class="ssh-terminal-slot" style="bottom:{terminalBottom}px">
      {#each sshStore.sessions as s (s.session_id)}
        <SSHTerminal
          sessionId={s.session_id}
          visible={sshStore.activeTerminalId === s.session_id}
          bottomOffset={terminalBottom}
          onMinimize={() => { sshStore.activeTerminalId = null; }}
          onDisconnect={() => { sshStore.activeTerminalId = null; }}
        />
      {/each}
    </div>
    <SSHSessionBar />
    {#if runningProfiles.length > 0}
      <div class="dock">
        <span class="dock-label">
          <span class="dock-dot"></span>
          {runningProfiles.length} running
        </span>
        <div class="dock-sessions">
          {#each runningProfiles as p (p.id)}
            <a href={getWorkspaceHref(p)} class="dock-item" title={p.name}>
              <Icon name="globe" size={13} />
              <span class="dock-name">{p.name}</span>
            </a>
          {/each}
        </div>
      </div>
    {/if}
  {/if}
 </div>
</div>

{#if isCsd}
  <ResizeHandles />
{/if}
{#if !standaloneNotes}
  <PasswordGenerator bind:open={pwgenOpen} />
  <TotpGenerator bind:open={totpOpen} context="global" />
{/if}
<UIInspector />

<style>
  /* Design tokens live in $lib/styles/tokens.css; global reset + primitives
     (.btn, .icon-btn, .form-*, .page, .card, .badge, .tab, .empty-state,
     .spinner, …) live in $lib/styles/base.css. Both imported at top of script. */

  /* ── Window frame (CSD) ──
     The OS draws no decoration (decorations:false), so we render the frame
     ourselves: the window is transparent and the app lives inside a rounded,
     shadowed panel with a small transparent gutter for the drop shadow — a
     native-looking framed window. The frame is dropped when maximized. */
  :global(html),
  :global(body) {
    background: transparent !important;
    overflow: hidden;
  }

  .app-frame {
    position: fixed;
    inset: 0;
    background: var(--bg);
  }
  /* CSD (Linux): rounded, shadowed panel with a gutter for the drop shadow. */
  .app-frame.csd {
    inset: var(--frame-gap, 9px);
    border-radius: 11px;
    overflow: hidden;
    box-shadow:
      0 0 0 1px var(--border),
      0 14px 44px rgba(0, 0, 0, 0.5);
  }
  .app-frame.csd.maximized {
    inset: 0;
    border-radius: 0;
    box-shadow: none;
  }

  /* Dedicated titlebar strip (the window's own bar, above the app toolbar). */
  .titlebar {
    display: flex;
    align-items: stretch;
    height: 34px;
    flex-shrink: 0;
    padding-left: 12px;
    background: var(--chrome);
    border-bottom: 1px solid var(--border);
  }
  .titlebar-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.78rem;
    font-weight: var(--fw-semibold);
    color: var(--text-2);
    letter-spacing: 0.1px;
    -webkit-user-select: none;
    user-select: none;
  }
  .titlebar-logo {
    width: 18px;
    height: 18px;
    object-fit: contain;
  }
  /* Empty flexible middle — the primary drag area. */
  .titlebar-drag {
    flex: 1;
  }

  /* Settings gear in the titlebar (flat, full-height, like the window buttons) */
  .titlebar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    align-self: stretch;
    width: 42px;
    border: none;
    background: transparent;
    color: var(--text-soft);
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }
  .titlebar-btn:hover {
    background: var(--surface-hover);
    color: var(--text);
  }
  .titlebar-btn.active {
    color: var(--accent-text);
  }

  /* ── Layout ── */
  .layout {
    display: flex;
    flex-direction: column;
    height: calc(100% - var(--inspector-bar-height, 0px));
    margin-top: var(--inspector-bar-height, 0px);
  }

  /* ── Top Bar ── */
  .topbar {
    height: var(--topbar-h);
    background: color-mix(in srgb, var(--bg-2) 85%, transparent);
    -webkit-backdrop-filter: blur(10px);
    backdrop-filter: blur(10px);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    padding: 0 var(--rail-pad-x);
    flex-shrink: 0;
    z-index: 10;
  }

  /* Inner rail — aligns topbar contents with the page content rail below */
  .topbar-inner {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    width: 100%;
    max-width: var(--page-max);
    margin-inline: auto;
  }

  .topbar-brand {
    display: flex;
    align-items: center;
    gap: 11px;
    text-decoration: none;
    color: var(--text);
    flex-shrink: 0;
  }

  .brand-tile {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    flex-shrink: 0;
  }

  .brand-logo {
    width: 34px;
    height: 34px;
    object-fit: contain;
  }

  .brand-name {
    font-weight: var(--fw-bold);
    font-size: 1rem;
    letter-spacing: -0.2px;
  }

  .topbar-nav {
    display: flex;
    align-items: center;
    gap: 0.2rem;
    flex: 1;
  }

  .nav-link {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding: 0 14px;
    border-radius: var(--radius);
    text-decoration: none;
    color: var(--text-2);
    font-size: var(--fs-base);
    font-weight: var(--fw-semibold);
    transition: all 0.15s;
  }

  .nav-link:hover { background: var(--surface); color: var(--text); }
  .nav-link.active { background: var(--accent-bg); color: var(--accent-text); }

  .topbar-right {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-left: auto;
  }

  .theme-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 38px;
    height: 38px;
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: 9px;
    color: var(--text-soft);
    cursor: pointer;
    transition: all 0.15s;
  }
  .theme-toggle:hover:not(:disabled) { background: var(--surface-hover); border-color: var(--border-2); color: var(--text); }

  /* ── Content ── */
  .content {
    flex: 1;
    overflow-y: auto;
    scrollbar-gutter: stable both-edges;  /* symmetric so content rail shares the topbar rail's center line */
    padding: 1.5rem var(--rail-pad-x);
    background: var(--bg);
    min-height: 0;
  }
  .content--notes {
    padding: 0.75rem;
    scrollbar-gutter: auto;
  }

  /* ── SSH Terminal Slot ── */
  .ssh-terminal-slot {
    position: fixed;
    left: 0;
    right: 0;
    top: calc(var(--topbar-h) + var(--inspector-bar-height, 0px));
    /* bottom is set via inline style = terminalBottom */
    overflow: hidden;
    pointer-events: none;
    z-index: 50;
  }

  /* ── Dock ── */
  .dock {
    height: var(--dock-h);
    background: var(--bg-2);
    border-top: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0 1rem;
    flex-shrink: 0;
    overflow: hidden;
  }

  .dock-label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: var(--fs-2xs);
    color: var(--text-2);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .dock-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--success);
    animation: pulse 2s infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .dock-sessions {
    display: flex;
    gap: 0.35rem;
    overflow-x: auto;
    flex: 1;
  }

  .dock-sessions::-webkit-scrollbar { display: none; }

  .dock-item {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.55rem;
    background: var(--success-bg);
    border: 1px solid color-mix(in srgb, var(--success) 25%, var(--border));
    border-radius: 999px;
    font-size: var(--fs-2xs);
    color: var(--success-text);
    white-space: nowrap;
    text-decoration: none;
    transition: filter 0.15s;
    flex-shrink: 0;
  }

  .dock-item:hover { filter: brightness(1.15); }
  .dock-name { max-width: 100px; overflow: hidden; text-overflow: ellipsis; }
</style>
