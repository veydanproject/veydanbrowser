<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import '@fontsource-variable/manrope/index.css';
  import '@fontsource-variable/jetbrains-mono/index.css';
  import '$lib/styles/tokens.css';
  import '$lib/styles/base.css';
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n';
  import { theme } from '$lib/theme';
  import Icon from '$lib/Icon.svelte';
  import { api } from '$lib/api';
  import type { Profile } from '$lib/types';
  import PasswordGenerator from '$lib/components/PasswordGenerator.svelte';
  import TotpGenerator from '$lib/components/TotpGenerator.svelte';
  import SSHSessionBar from '$lib/components/ssh/SSHSessionBar.svelte';
  import SSHTerminal from '$lib/components/ssh/SSHTerminal.svelte';
  import { sshStore } from '$lib/store/ssh.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { listen } from '@tauri-apps/api/event';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import UIInspector from '$lib/inspector/UIInspector.svelte';
  import { inspectorApp } from '$lib/inspector/inspector.svelte';

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

  async function refreshRunning() {
    try {
      runningIds = await api.profiles.runningIds();
    } catch {}
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

    // Preload profiles and TOTP for running indicator and badge resolution
    profilesStore.ensureLoaded();
    totpStore.ensureLoaded();
    sshStore.ensureLoaded();
    refreshRunning();

    // Fallback: refresh on window focus (handles crash/manual kill/hot reload)
    window.addEventListener('focus', refreshRunning);

    // Native back button support for WebKitGTK (Tauri on Linux)
    window.addEventListener('keydown', handleKeyBack);
    window.addEventListener('mouseup', handleMouseBack);

    // Main: react to backend events
    const unlisten = listen<{ running_ids: string[] }>('profiles://running-changed', (e) => {
      runningIds = e.payload.running_ids;
    });

    return () => {
      unsub();
      window.removeEventListener('focus', refreshRunning);
      window.removeEventListener('keydown', handleKeyBack);
      window.removeEventListener('mouseup', handleMouseBack);
      unlisten.then((fn) => fn());
    };
  });

  function isActive(href: string) {
    if (href === '/') return $page.url.pathname === '/';
    return $page.url.pathname.startsWith(href);
  }

  function getWorkspaceHref(p: Profile) {
    return p.workspace_id ? `/workspace/${p.workspace_id}` : '/';
  }
</script>

<div class="layout">
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
      <a href="/notes" class="nav-link" class:active={isActive('/notes')}>
        <Icon name="file-text" size={14} />
        {$t('nav_notes')}
      </a>
      <a href="/settings" class="nav-link" class:active={isActive('/settings')}>
        <Icon name="settings" size={14} />
        {$t('nav_settings')}
      </a>
    </nav>

    <div class="topbar-right">
      <button class="theme-toggle" onclick={() => (totpOpen = !totpOpen)} title={$t('totp_title')}>
        <Icon name="shield" size={15} />
      </button>
      <button class="theme-toggle" onclick={() => (pwgenOpen = !pwgenOpen)} title={$t('pwgen_title')}>
        <Icon name="key" size={15} />
      </button>
      <!-- Light theme is disabled for now: the redesign palette is dark-only.
           toggleTheme stays wired for when the light palette lands. -->
      <button class="theme-toggle" disabled title={$t('theme_light_soon')}>
        {#if $theme === 'dark'}
          <Icon name="sun" size={15} />
        {:else}
          <Icon name="moon" size={15} />
        {/if}
      </button>
    </div>
   </div>
  </header>

  <main class="content">
    {@render children()}
  </main>

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
  <!-- Dock is last in DOM — at the very bottom below the SSH bar -->
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
</div>

<PasswordGenerator bind:open={pwgenOpen} />
<TotpGenerator bind:open={totpOpen} context="global" />
<UIInspector />

<style>
  /* Design tokens live in $lib/styles/tokens.css; global reset + primitives
     (.btn, .icon-btn, .form-*, .page, .card, .badge, .tab, .empty-state,
     .spinner, …) live in $lib/styles/base.css. Both imported at top of script. */

  /* ── Layout ── */
  .layout {
    display: flex;
    flex-direction: column;
    height: calc(100dvh - var(--inspector-bar-height, 0px));
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

  /* Gradient tile behind the logo (design: 34×34, radius 9, purple gradient) */
  .brand-tile {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    background: var(--accent-grad);
    box-shadow: var(--shadow-logo);
    flex-shrink: 0;
  }

  .brand-logo {
    width: 22px;
    height: 22px;
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
