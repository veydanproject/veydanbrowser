<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { locale, t } from '$lib/i18n';
  import { theme } from '$lib/theme';
  import { inspectorApp } from '$lib/inspector/inspector.svelte';
  import Icon from '$lib/Icon.svelte';
  import { api } from '$lib/api';
  import type { Locale } from '$lib/i18n';
  import type { CamoufoxStatus } from '$lib/types';
  import { formatError } from '$lib/utils';

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  const languages: { value: Locale; label: string; native: string }[] = [
    { value: 'en', label: 'English', native: 'English' },
    { value: 'ru', label: 'Russian', native: 'Русский' },
  ];

  // About / license links. Set REPO_URL once the public repo is live to reveal
  // the source-code, summary and third-party-licenses links.
  const REPO_URL: string = '';
  const LICENSE_URL = 'https://polyformproject.org/licenses/perimeter/1.0.1';
  const CAMOUFOX_URL = 'https://camoufox.com/';
  const summaryUrl = REPO_URL ? `${REPO_URL}/blob/main/LICENSE-SUMMARY.md` : '';
  const thirdPartyUrl = REPO_URL ? `${REPO_URL}/blob/main/THIRD-PARTY-LICENSES.md` : '';

  async function openExternal(url: string) {
    if (!url) return;
    if (isTauri) {
      await api.system.openUrl(url).catch((e) => console.error(e));
    } else {
      window.open(url, '_blank', 'noopener');
    }
  }

  let camoufox = $state<CamoufoxStatus | null>(null);
  let downloading = $state(false);
  let extracting = $state(false);
  let downloadError = $state('');
  let downloadDone = $state('');
  let progress = $state<{ percent: number; downloaded: number; total: number } | null>(null);
  let unlisteners: (() => void)[] = [];

  let appVersion = $state('');
  let latestCamoufox = $state<string | null>(null);
  let checkingUpdate = $state(false);
  let checkUpdateError = $state('');

  // Notes dir
  let notesDir = $state('');
  let notesDirIsCustom = $state(false);
  let notesDirSaving = $state(false);
  let notesDirError = $state('');

  onMount(async () => {
    camoufox = await api.camoufox.status().catch(() => null);

    if (isTauri) {
      const { getVersion } = await import('@tauri-apps/api/app');
      appVersion = await getVersion().catch(() => '');
    }

    // Load notes dir
    try {
      const info = await api.notes.getDir();
      notesDir = info.current;
      notesDirIsCustom = info.is_custom;
    } catch {}

    // Restore state if download was already running
    const dlState = await api.camoufox.downloadState().catch(() => null);
    if (dlState?.state === 'downloading') {
      downloading = true;
      if (dlState.downloaded && dlState.total) {
        progress = { percent: dlState.percent ?? 0, downloaded: dlState.downloaded, total: dlState.total };
      }
    }

    if (isTauri) {
      const { listen } = await import('@tauri-apps/api/event');

      unlisteners.push(await listen<{ state: string; downloaded: number; total: number; percent: number }>(
        'camoufox://progress',
        (e) => {
          downloading = true;
          extracting = false;
          progress = { percent: e.payload.percent, downloaded: e.payload.downloaded, total: e.payload.total };
        }
      ));

      unlisteners.push(await listen('camoufox://extracting', () => {
        extracting = true;
        progress = null;
      }));

      unlisteners.push(await listen<string>('camoufox://done', async (e) => {
        downloading = false;
        extracting = false;
        progress = null;
        downloadDone = e.payload;
        camoufox = await api.camoufox.status().catch(() => null);
        latestCamoufox = null;
      }));

      unlisteners.push(await listen<string>('camoufox://error', (e) => {
        downloading = false;
        extracting = false;
        progress = null;
        if (!e.payload.includes('cancelled')) {
          downloadError = e.payload;
        }
      }));
    }
  });

  onDestroy(() => unlisteners.forEach(fn => fn()));

  function formatMb(bytes: number) {
    return (bytes / 1024 / 1024).toFixed(0) + ' MB';
  }

  async function downloadCamoufox() {
    downloadError = '';
    downloadDone = '';
    try {
      await api.camoufox.download();
      downloading = true;
    } catch (e) {
      downloadError = formatError(e);
    }
  }

  async function cancelDownload() {
    await api.camoufox.cancel().catch(() => {});
    downloading = false;
    extracting = false;
    progress = null;
  }

  async function checkForUpdates() {
    checkingUpdate = true;
    checkUpdateError = '';
    latestCamoufox = null;
    try {
      latestCamoufox = await api.camoufox.latestVersion();
    } catch (e) {
      checkUpdateError = formatError(e);
    } finally {
      checkingUpdate = false;
    }
  }

  function isUpToDate(): boolean {
    if (!latestCamoufox || !camoufox?.camoufox_tag) return false;
    return latestCamoufox === camoufox.camoufox_tag;
  }

  async function browseNotesDir() {
    if (!isTauri) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: 'Select notes folder' });
      if (selected && typeof selected === 'string') {
        notesDir = selected;
      }
    } catch {}
  }

  async function saveNotesDir() {
    notesDirSaving = true;
    notesDirError = '';
    try {
      const info = await api.notes.setDir(notesDir.trim() || null);
      notesDir = info.current;
      notesDirIsCustom = info.is_custom;
    } catch (e) {
      notesDirError = formatError(e);
    } finally {
      notesDirSaving = false;
    }
  }

  async function resetNotesDir() {
    notesDirSaving = true;
    notesDirError = '';
    try {
      const info = await api.notes.setDir(null);
      notesDir = info.current;
      notesDirIsCustom = info.is_custom;
    } catch (e) {
      notesDirError = formatError(e);
    } finally {
      notesDirSaving = false;
    }
  }
</script>

<div class="page" style="--page-max: var(--page-max-narrow)">
  <h1>{$t('settings_title')}</h1>

  <!-- Camoufox -->
  <div class="card">
    <div class="card-title">Camoufox</div>

    {#if camoufox === null}
      <p class="muted">{$t('settings_camoufox_checking')}</p>
    {:else if camoufox.installed}
      <div class="status-row">
        <span class="badge badge-ok">{$t('settings_camoufox_installed')}</span>
      </div>

      <div class="version-table">
        {#if camoufox.camoufox_tag}
          <div class="version-row">
            <span class="version-label">{$t('settings_camoufox_tag')}</span>
            <span class="version-value">{camoufox.camoufox_tag}</span>
          </div>
        {/if}
        {#if camoufox.version}
          <div class="version-row">
            <span class="version-label">{$t('settings_camoufox_firefox_version')}</span>
            <span class="version-value">{camoufox.version}</span>
          </div>
        {/if}
        {#if camoufox.path}
          <div class="version-row">
            <span class="version-label">Path</span>
            <span class="version-value path-value">{camoufox.path}</span>
          </div>
        {/if}
      </div>

      <div class="btn-row">
        <button class="btn btn-primary btn-sm" disabled={downloading} onclick={downloadCamoufox}>
          {downloading ? $t('settings_camoufox_btn_updating') : $t('settings_camoufox_btn_update')}
        </button>
        <button class="btn btn-ghost btn-sm" disabled={checkingUpdate || downloading} onclick={checkForUpdates}>
          {checkingUpdate ? $t('settings_camoufox_checking_update') : $t('settings_camoufox_check_update')}
        </button>
        {#if downloading}
          <button class="btn btn-ghost btn-sm" onclick={cancelDownload}>Cancel</button>
        {/if}
      </div>

      {#if latestCamoufox}
        {#if isUpToDate()}
          <p class="ok-msg">{$t('settings_camoufox_up_to_date')}</p>
        {:else}
          <p class="warn-msg">{$t('settings_camoufox_update_available', { version: latestCamoufox })}</p>
        {/if}
      {/if}
      {#if checkUpdateError}
        <div class="error-msg">{checkUpdateError}</div>
      {/if}
    {:else}
      <div class="status-row">
        <span class="badge badge-warn">{$t('settings_camoufox_not_installed')}</span>
      </div>
      <p class="muted small">{$t('settings_camoufox_download_hint')}</p>
      <div class="btn-row">
        <button class="btn btn-primary btn-sm" disabled={downloading} onclick={downloadCamoufox}>
          {downloading ? $t('settings_camoufox_btn_downloading') : $t('settings_camoufox_btn_download')}
        </button>
        {#if downloading}
          <button class="btn btn-ghost btn-sm" onclick={cancelDownload}>Cancel</button>
        {/if}
      </div>
    {/if}

    {#if extracting}
      <p class="muted small">Extracting… please wait</p>
    {/if}

    {#if downloading && progress}
      <div class="progress-wrap">
        <div class="progress-bar">
          <div class="progress-fill" style="width: {progress.percent}%"></div>
        </div>
        <span class="progress-label">
          {formatMb(progress.downloaded)} / {formatMb(progress.total)} · {progress.percent}%
        </span>
      </div>
    {/if}

    {#if downloadDone}
      <p class="ok-msg">{$t('settings_camoufox_success', { version: downloadDone })}</p>
    {/if}
    {#if downloadError}
      <div class="error-msg">{downloadError}</div>
    {/if}
  </div>

  <!-- Language -->
  <div class="card">
    <div class="card-title">{$t('settings_section_language')}</div>
    <p class="muted">{$t('settings_language_label')}</p>
    <div class="lang-options">
      {#each languages as lang}
        <button
          class="lang-btn"
          class:active={$locale === lang.value}
          onclick={() => locale.set(lang.value)}
        >
          <span class="lang-flag">{lang.value === 'en' ? '🇬🇧' : '🇷🇺'}</span>
          <span class="lang-native">{lang.native}</span>
          {#if $locale === lang.value}<span class="lang-check">✓</span>{/if}
        </button>
      {/each}
    </div>
  </div>

  <!-- Theme -->
  <div class="card">
    <div class="card-title">{$t('settings_section_theme')}</div>
    <div class="theme-options">
      <button class="theme-opt" class:active={$theme === 'dark'} onclick={() => theme.set('dark')}>
        <span class="theme-icon"><Icon name="moon" size={15} /></span>
        <span>Dark</span>
        {#if $theme === 'dark'}<span class="lang-check">✓</span>{/if}
      </button>
      <!-- Light theme is disabled while the redesign is dark-only -->
      <button class="theme-opt" disabled title={$t('theme_light_soon')}>
        <span class="theme-icon"><Icon name="sun" size={15} /></span>
        <span>Light</span>
      </button>
    </div>
  </div>

  <!-- Developer Tools -->
  <div class="card">
    <div class="card-title">{$t('inspector_developer_tools')}</div>
    <div class="dev-tools-row">
      <div class="dev-tools-info">
        <span>{$t('inspector_toggle')}</span>
        <span class="muted">{$t('inspector_hotkey_hint')}</span>
      </div>
      <button
        class="toggle"
        class:on={inspectorApp.enabled}
        onclick={() => inspectorApp.toggle()}
        aria-pressed={inspectorApp.enabled}
        aria-label={$t('inspector_toggle')}
      ></button>
    </div>
  </div>

  <!-- Notes -->
  <div class="card">
    <div class="card-title">{$t('settings_notes_section')}</div>
    <p class="muted">{$t('settings_notes_folder_hint')}</p>
    <div class="dir-row">
      <input
        class="dir-input"
        type="text"
        bind:value={notesDir}
        placeholder={$t('settings_notes_folder_placeholder')}
        readonly={!isTauri}
      />
      {#if isTauri}
        <button class="btn btn-ghost btn-sm btn-icon" onclick={browseNotesDir} title={$t('settings_notes_browse')}>
          <Icon name="folder-open" size={14} />
        </button>
      {/if}
    </div>
    <div class="btn-row">
      <button class="btn btn-primary btn-sm" disabled={notesDirSaving} onclick={saveNotesDir}>
        {notesDirSaving ? $t('settings_notes_saving') : $t('settings_notes_save')}
      </button>
      {#if notesDirIsCustom}
        <button class="btn btn-ghost btn-sm" disabled={notesDirSaving} onclick={resetNotesDir}>
          {$t('settings_notes_reset')}
        </button>
      {/if}
    </div>
    {#if notesDirIsCustom}
      <p class="muted small">{$t('settings_notes_custom_active')}</p>
    {/if}
    {#if notesDirError}
      <div class="error-msg">{notesDirError}</div>
    {/if}
  </div>

  <!-- About -->
  <div class="card">
    <div class="card-title">{$t('settings_section_about')}</div>

    <div class="about-head">
      <span class="about-logo">
        <img src="/logo.png" alt="" />
      </span>
      <div class="about-head-text">
        <div class="about-app">{$t('settings_about_app')}</div>
        <div class="about-tagline">{$t('settings_about_tagline')}</div>
      </div>
    </div>

    <div class="version-table">
      <div class="version-row">
        <span class="version-label">{$t('settings_about_version')}</span>
        <span class="version-value">{appVersion ? `v${appVersion}` : '—'}</span>
      </div>
      <div class="version-row">
        <span class="version-label">{$t('settings_about_license')}</span>
        <span class="version-value">PolyForm Perimeter 1.0.1</span>
      </div>
    </div>

    <p class="about-note">{$t('settings_about_license_note')}</p>

    <div class="about-links">
      <button type="button" class="link-btn" onclick={() => openExternal(LICENSE_URL)}>
        <Icon name="external-link" size={14} />
        <span>{$t('settings_about_link_license')}</span>
      </button>
      {#if REPO_URL}
        <button type="button" class="link-btn" onclick={() => openExternal(summaryUrl)}>
          <Icon name="external-link" size={14} />
          <span>{$t('settings_about_link_summary')}</span>
        </button>
        <button type="button" class="link-btn" onclick={() => openExternal(thirdPartyUrl)}>
          <Icon name="external-link" size={14} />
          <span>{$t('settings_about_link_thirdparty')}</span>
        </button>
        <button type="button" class="link-btn" onclick={() => openExternal(REPO_URL)}>
          <Icon name="external-link" size={14} />
          <span>{$t('settings_about_link_repo')}</span>
        </button>
      {/if}
      <button type="button" class="link-btn" onclick={() => openExternal(CAMOUFOX_URL)}>
        <Icon name="external-link" size={14} />
        <span>{$t('settings_about_built_on')} {$t('settings_about_link_camoufox')}</span>
      </button>
    </div>

    <div class="about-copyright">{$t('settings_about_copyright')}</div>
  </div>
</div>

<style>
  /* max-width comes from global .page via --page-max (set inline) */

  /* bare h1 (not inside .page-header) — global .page-header h1 doesn't apply */
  h1 { font-size: var(--fs-2xl); font-weight: var(--fw-extrabold); letter-spacing: -0.6px; margin-bottom: var(--sp-2); }

  /* local deltas over global .card: bg, internal flex layout, gap, shadow */
  .card {
    padding: var(--sp-6);
    display: flex; flex-direction: column; gap: var(--sp-4);
  }

  /* uppercase label variant — differs from global .card-title */
  .card-title {
    font-size: var(--fs-2xs); font-weight: var(--fw-bold); color: var(--text-dim);
    text-transform: uppercase; letter-spacing: 1px;
  }

  .status-row { display: flex; align-items: center; gap: var(--sp-3); }

  /* badges use global .badge / .badge-ok / .badge-warn */

  /* .muted uses global color; keep font-size delta */
  .muted { font-size: var(--fs-base); }
  .small { font-size: var(--fs-sm); }
  .ok-msg { font-size: var(--fs-sm); color: var(--success-text); }
  .warn-msg { font-size: var(--fs-sm); color: var(--warn-text); }

  .version-table { display: flex; flex-direction: column; gap: 0.35rem; }
  .version-row { display: flex; align-items: baseline; gap: var(--sp-2); }
  .version-label { font-size: 0.82rem; color: var(--text-faint); min-width: 120px; flex-shrink: 0; }
  .version-value { font-size: var(--fs-sm); color: var(--text-body); font-family: var(--font-mono); }
  .path-value { font-size: var(--fs-2xs); word-break: break-all; }

  /* About section */
  .about-head { display: flex; align-items: center; gap: 13px; }
  .about-head-text { display: flex; flex-direction: column; gap: 2px; }
  .about-logo {
    display: flex; align-items: center; justify-content: center;
    width: 44px; height: 44px; border-radius: var(--radius-md);
    background: var(--accent-grad); box-shadow: var(--shadow-logo);
    flex-shrink: 0;
  }
  .about-logo img { width: 28px; height: 28px; object-fit: contain; }
  .about-app { font-size: var(--fs-lg); font-weight: var(--fw-extrabold); letter-spacing: -0.3px; color: var(--text); }
  .about-tagline { font-size: var(--fs-sm); color: var(--text-3); }
  .about-note { font-size: var(--fs-sm); color: var(--text-3); line-height: 1.5; margin: 0; }
  .about-links { display: flex; flex-wrap: wrap; gap: var(--sp-2) var(--sp-4); }
  .link-btn {
    display: inline-flex; align-items: center; gap: 0.4rem;
    background: none; border: none; padding: 0; cursor: pointer;
    color: var(--accent-text-3); font-size: var(--fs-sm); font-weight: 500;
  }
  .link-btn:hover { color: var(--accent-text); }
  .link-btn:hover { text-decoration: underline; }
  .about-copyright { font-size: var(--fs-xs); color: var(--text-3); }

  .btn-sm { padding: 0.35rem var(--sp-3); font-size: var(--fs-sm); }
  .btn-row { display: flex; gap: var(--sp-2); align-items: center; }

  .progress-wrap { display: flex; flex-direction: column; gap: 0.3rem; }
  .progress-bar { height: 5px; background: var(--surface-2); border-radius: 999px; overflow: hidden; }
  .progress-fill { height: 100%; background: var(--accent); border-radius: 999px; transition: width 0.2s ease; }
  .progress-label { font-size: var(--fs-2xs); color: var(--text-2); font-family: var(--font-mono); }

  .lang-options { display: flex; gap: 0.625rem; }

  /* Segment buttons per design: 46px, radius 11, accent tint when active */
  .lang-btn {
    display: flex; align-items: center; gap: 9px;
    height: var(--control-h-lg); padding: 0 20px; background: var(--surface-3);
    border: 1px solid var(--border); border-radius: var(--radius-field);
    color: var(--text-body); font-size: 0.9rem; font-weight: var(--fw-semibold); cursor: pointer;
    transition: all 0.15s; min-width: 130px;
  }
  .lang-btn:hover { border-color: var(--border-2); color: var(--text); }
  .lang-btn.active { border-color: var(--accent-border); background: var(--accent-bg); color: var(--accent-text); }

  .lang-flag { font-size: var(--fs-md); }
  .lang-native { flex: 1; text-align: left; }
  .lang-check { color: var(--accent-text); font-weight: 700; }

  .dev-tools-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
  }

  .dev-tools-info {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    font-size: var(--fs-sm);
    color: var(--text);
  }

  .theme-options { display: flex; gap: 0.625rem; }

  .theme-opt {
    display: flex; align-items: center; gap: 9px;
    height: var(--control-h-lg); padding: 0 20px; background: var(--surface-3);
    border: 1px solid var(--border); border-radius: var(--radius-field);
    color: var(--text-body); font-size: 0.9rem; font-weight: var(--fw-semibold); cursor: pointer;
    transition: all 0.15s; min-width: 110px;
  }
  .theme-opt:hover:not(:disabled) { border-color: var(--border-2); color: var(--text); }
  .theme-opt.active { border-color: var(--accent-border); background: var(--accent-bg); color: var(--accent-text); }
  .theme-opt:disabled { opacity: 0.4; cursor: not-allowed; }
  .theme-icon { font-size: var(--fs-md); display: flex; }

  .dir-row {
    display: flex; align-items: center; gap: 0.4rem;
  }
  .dir-input {
    flex: 1; height: 44px; background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 0 var(--sp-3);
    font-size: 0.82rem; color: var(--text-body); font-family: var(--font-mono);
    outline: none; text-overflow: ellipsis;
  }
  .dir-input:focus { border-color: var(--accent-border); }
  .btn-icon { width: 44px; height: 44px; justify-content: center; padding: 0; }
  .error-msg { font-size: var(--fs-sm); color: var(--danger-text); }
</style>
