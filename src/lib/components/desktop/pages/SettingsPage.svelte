<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { locale, t } from '$lib/i18n';
  import {
    theme,
    themeCustom,
    resolvedOverride,
    hasThemeOverrides,
    setThemeOverride,
    resetThemeOverrides,
  } from '$lib/theme';
  import ColorField from '$lib/components/ui/ColorField.svelte';
  import { inspectorApp } from '$lib/inspector/inspector.svelte';
  import Icon from '$lib/Icon.svelte';
  import { api } from '$lib/api';
  import type { BackupConfig, BackupFileInfo } from '$lib/api';
  import type { Locale } from '$lib/i18n';
  import type { CamoufoxStatus } from '$lib/types';
  import { formatError, formatBytes } from '$lib/utils';
  import { updaterStore } from '$lib/store/updater.svelte';
  import CustomSelect from '$lib/components/CustomSelect.svelte';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import NoteCaptureRules from '$lib/components/notes/NoteCaptureRules.svelte';
  import NoteAttachmentPolicy from '$lib/components/notes/NoteAttachmentPolicy.svelte';
  import NoteLockSettings from '$lib/components/notes/NoteLockSettings.svelte';
  import SyncSettings from '$lib/components/SyncSettings.svelte';

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  const THEME_PRESETS = {
    chrome: {
      dark: ['#0b0b10', '#14141c', '#1c1c26', '#0e1620', '#16120e', '#1a1224'],
      light: ['#fafafd', '#ffffff', '#f0eef8', '#ebe8f2', '#f4f1ea', '#efe8f0'],
    },
    accent: ['#8b7bff', '#6d5cf0', '#60a5fa', '#2dd4bf', '#f472b6', '#34d399'],
    bg: {
      dark: ['#08080c', '#0c0c12', '#101018', '#0a1016', '#12100c', '#100c16'],
      light: ['#f4f3f8', '#ffffff', '#eeeef4', '#f6f4ee', '#f2eef8', '#eaeaf0'],
    },
  } as const;

  const languages: { value: Locale; label: string; native: string }[] = [
    { value: 'en', label: 'English', native: 'English' },
    { value: 'ru', label: 'Russian', native: 'Русский' },
  ];

  const REPO_URL: string = 'https://github.com/veydanproject/veydanbrowser';
  const RELEASES_URL = `${REPO_URL}/releases/latest`;
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

  // Quick capture shortcut
  let quickShortcut = $state('');
  let quickShortcutSaving = $state(false);
  let quickShortcutError = $state('');

  async function saveQuickShortcut() {
    quickShortcutSaving = true;
    quickShortcutError = '';
    try {
      quickShortcut = await api.notes.quickCaptureShortcutSet(quickShortcut);
    } catch (e) {
      quickShortcutError = `${$t('settings_quick_capture_invalid')}: ${formatError(e)}`;
    } finally {
      quickShortcutSaving = false;
    }
  }

  // Notes dir
  let notesDir = $state('');
  let notesDirIsCustom = $state(false);
  let notesDirSaving = $state(false);
  let notesDirError = $state('');

  // System tray
  let trayMinimize = $state(false);
  let trayClose = $state(false);
  let trayStartHidden = $state(false);

  // Backup
  let backupCfg = $state<BackupConfig>({
    dir: null,
    password: null,
    schedule_enabled: false,
    schedule_mode: 'interval',
    interval_hours: 24,
    time: '03:00',
    weekday: 0,
    keep: 5,
    last_run: null,
  });
  let backupList = $state<BackupFileInfo[]>([]);
  let backupSaving = $state(false);
  let backupSaved = $state(false);
  let backupError = $state('');
  let backupRunning = $state(false);
  let backupPhase = $state('');
  let backupProgress = $state(0);
  let backupDone = $state('');
  // Restore dialog
  let restoreTarget = $state<BackupFileInfo | null>(null);
  let restorePassword = $state('');
  let restoreError = $state('');
  let restoring = $state(false);
  let restorePhase = $state('');
  let restorePercent = $state(0);

  const modeOptions = $derived([
    { value: 'interval', label: $t('settings_backup_mode_interval') },
    { value: 'daily', label: $t('settings_backup_mode_daily') },
    { value: 'weekly', label: $t('settings_backup_mode_weekly') },
  ]);
  const weekdayOptions = $derived(
    [0, 1, 2, 3, 4, 5, 6].map((d) => ({
      value: String(d),
      label: $t(`settings_backup_day_${d}` as any),
    }))
  );

  async function loadBackup() {
    try {
      backupCfg = await api.backup.getConfig();
    } catch {}
    await refreshBackupList();
  }

  async function refreshBackupList() {
    try {
      backupList = await api.backup.list();
    } catch {
      backupList = [];
    }
  }

  async function browseBackupDir() {
    if (!isTauri) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: $t('settings_backup_folder') });
      if (selected && typeof selected === 'string') {
        backupCfg.dir = selected;
      }
    } catch {}
  }

  async function saveBackupConfig() {
    backupSaving = true;
    backupSaved = false;
    backupError = '';
    try {
      await api.backup.setConfig($state.snapshot(backupCfg));
      backupSaved = true;
      setTimeout(() => (backupSaved = false), 2000);
      await refreshBackupList();
    } catch (e) {
      backupError = formatError(e);
    } finally {
      backupSaving = false;
    }
  }

  async function runBackupNow() {
    backupError = '';
    backupDone = '';
    // Persist current config first so the backend uses the latest dir/password.
    await saveBackupConfig();
    if (backupError) return;
    backupRunning = true;
    backupProgress = 0;
    backupPhase = '';
    try {
      await api.backup.runNow();
    } catch (e) {
      backupRunning = false;
      backupError = formatError(e);
    }
  }

  function openRestore(b: BackupFileInfo) {
    restoreTarget = b;
    restorePassword = backupCfg.password ?? '';
    restoreError = '';
  }

  function closeRestore() {
    if (restoring) return;
    restoreTarget = null;
  }

  async function confirmRestore() {
    if (!restoreTarget) return;
    restoreError = '';
    restoring = true;
    restorePhase = '';
    restorePercent = 0;
    try {
      // On success the app restarts, so this call never resolves.
      await api.backup.restore(restoreTarget.path, restorePassword);
    } catch (e) {
      restoring = false;
      restoreError = formatError(e);
    }
  }

  async function saveTray() {
    if (!isTauri) return;
    try {
      await api.settings.setTray({
        minimize_to_tray: trayMinimize,
        close_to_tray: trayClose,
        start_hidden: trayStartHidden,
      });
    } catch (e) {
      console.error(e);
    }
  }

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
    try {
      quickShortcut = await api.notes.quickCaptureShortcutGet();
    } catch {}

    // Load tray settings
    if (isTauri) {
      try {
        const tray = await api.settings.getTray();
        trayMinimize = tray.minimize_to_tray;
        trayClose = tray.close_to_tray;
        trayStartHidden = tray.start_hidden;
      } catch {}
    }

    // Load backup config + list
    await loadBackup();

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

      // ── Backup events ──
      unlisteners.push(await listen<{ phase: string; percent: number }>('backup://progress', (e) => {
        backupRunning = true;
        backupPhase = e.payload.phase;
        backupProgress = e.payload.percent;
      }));

      unlisteners.push(await listen<string>('backup://done', async () => {
        backupRunning = false;
        backupProgress = 100;
        backupDone = $t('settings_backup_done');
        setTimeout(() => (backupDone = ''), 4000);
        await Promise.all([refreshBackupList(), (async () => { backupCfg = await api.backup.getConfig().catch(() => backupCfg); })()]);
      }));

      unlisteners.push(await listen<{ phase: string; percent: number }>('backup://restore-progress', (e) => {
        restorePhase = e.payload.phase;
        restorePercent = e.payload.percent;
      }));

      unlisteners.push(await listen<string>('backup://error', (e) => {
        backupRunning = false;
        backupError = e.payload;
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
      <button class="theme-opt" class:active={$theme === 'light'} onclick={() => theme.set('light')}>
        <span class="theme-icon"><Icon name="sun" size={15} /></span>
        <span>Light</span>
        {#if $theme === 'light'}<span class="lang-check">✓</span>{/if}
      </button>
    </div>
    <div class="theme-customize">
      <ColorField
        label={$t('settings_theme_chrome')}
        value={resolvedOverride($themeCustom, $theme, 'chrome')}
        presets={[...THEME_PRESETS.chrome[$theme]]}
        onchange={(hex) => setThemeOverride('chrome', hex)}
      />
      <ColorField
        label={$t('settings_theme_accent')}
        value={resolvedOverride($themeCustom, $theme, 'accent')}
        presets={[...THEME_PRESETS.accent]}
        onchange={(hex) => setThemeOverride('accent', hex)}
      />
      <ColorField
        label={$t('settings_theme_bg')}
        value={resolvedOverride($themeCustom, $theme, 'bg')}
        presets={[...THEME_PRESETS.bg[$theme]]}
        onchange={(hex) => setThemeOverride('bg', hex)}
      />
      <button
        type="button"
        class="btn btn-ghost btn-sm theme-reset"
        disabled={!hasThemeOverrides($themeCustom, $theme)}
        onclick={resetThemeOverrides}
      >
        {$t('settings_theme_reset')}
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

  <!-- System tray -->
  <div class="card">
    <div class="card-title">{$t('settings_tray_section')}</div>
    <div class="dev-tools-row">
      <div class="dev-tools-info">
        <span>{$t('settings_tray_minimize')}</span>
        <span class="muted">{$t('settings_tray_minimize_hint')}</span>
      </div>
      <button
        class="toggle"
        class:on={trayMinimize}
        disabled={!isTauri}
        onclick={() => { trayMinimize = !trayMinimize; saveTray(); }}
        aria-pressed={trayMinimize}
        aria-label={$t('settings_tray_minimize')}
      ></button>
    </div>
    <div class="dev-tools-row">
      <div class="dev-tools-info">
        <span>{$t('settings_tray_close')}</span>
        <span class="muted">{$t('settings_tray_close_hint')}</span>
      </div>
      <button
        class="toggle"
        class:on={trayClose}
        disabled={!isTauri}
        onclick={() => { trayClose = !trayClose; saveTray(); }}
        aria-pressed={trayClose}
        aria-label={$t('settings_tray_close')}
      ></button>
    </div>
    <div class="dev-tools-row">
      <div class="dev-tools-info">
        <span>{$t('settings_tray_start_hidden')}</span>
        <span class="muted">{$t('settings_tray_start_hidden_hint')}</span>
      </div>
      <button
        class="toggle"
        class:on={trayStartHidden}
        disabled={!isTauri}
        onclick={() => { trayStartHidden = !trayStartHidden; saveTray(); }}
        aria-pressed={trayStartHidden}
        aria-label={$t('settings_tray_start_hidden')}
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

  <!-- Note attachments -->
  <div class="card">
    <div class="card-title">{$t('settings_att_section')}</div>
    <p class="muted">{$t('settings_att_hint')}</p>
    <NoteAttachmentPolicy />
  </div>

  <!-- Browser capture rules -->
  <div class="card">
    <div class="card-title">{$t('settings_capture_section')}</div>
    <p class="muted">{$t('settings_capture_hint')}</p>
    <NoteCaptureRules />
  </div>

  <!-- Quick capture shortcut -->
  <div class="card">
    <div class="card-title">{$t('settings_quick_capture_section')}</div>
    <p class="muted">{$t('settings_quick_capture_hint')}</p>
    <div class="dir-row">
      <input
        class="dir-input"
        type="text"
        bind:value={quickShortcut}
        placeholder="CmdOrCtrl+Shift+N"
        readonly={!isTauri}
      />
    </div>
    <div class="btn-row">
      <button class="btn btn-primary btn-sm" disabled={quickShortcutSaving || !isTauri} onclick={saveQuickShortcut}>
        {quickShortcutSaving ? $t('settings_notes_saving') : $t('settings_notes_save')}
      </button>
    </div>
    {#if quickShortcutError}
      <div class="error-msg">{quickShortcutError}</div>
    {/if}
  </div>

  <!-- Notes lock -->
  <div class="card">
    <div class="card-title">{$t('settings_lock_section')}</div>
    <p class="muted">{$t('settings_lock_hint')}</p>
    <NoteLockSettings />
  </div>

  <!-- Backup -->
  <div class="card">
    <div class="card-title">{$t('settings_backup_section')}</div>
    <p class="muted">{$t('settings_backup_hint')}</p>

    <!-- Destination folder -->
    <div class="field">
      <span class="field-label">{$t('settings_backup_folder')}</span>
      <div class="dir-row">
        <input
          class="dir-input"
          type="text"
          bind:value={backupCfg.dir}
          placeholder={$t('settings_backup_folder_placeholder')}
          readonly={!isTauri}
        />
        {#if isTauri}
          <button class="btn btn-ghost btn-sm btn-icon" onclick={browseBackupDir} title={$t('settings_backup_browse')}>
            <Icon name="folder-open" size={14} />
          </button>
        {/if}
      </div>
    </div>

    <!-- Password -->
    <div class="field">
      <span class="field-label">{$t('settings_backup_password')}</span>
      <input
        class="field-input"
        type="password"
        bind:value={backupCfg.password}
        placeholder={$t('settings_backup_password_placeholder')}
        autocomplete="off"
      />
      <p class="muted small">{$t('settings_backup_password_note')}</p>
    </div>

    <!-- Schedule -->
    <div class="dev-tools-row">
      <div class="dev-tools-info">
        <span>{$t('settings_backup_schedule')}</span>
        <span class="muted">{$t('settings_backup_schedule_hint')}</span>
      </div>
      <button
        class="toggle"
        class:on={backupCfg.schedule_enabled}
        onclick={() => { backupCfg.schedule_enabled = !backupCfg.schedule_enabled; }}
        aria-pressed={backupCfg.schedule_enabled}
        aria-label={$t('settings_backup_schedule')}
      ></button>
    </div>

    {#if backupCfg.schedule_enabled}
      <div class="sched-grid">
        <div class="field">
          <span class="field-label">{$t('settings_backup_mode')}</span>
          <CustomSelect
            options={modeOptions}
            value={backupCfg.schedule_mode}
            onchange={(v) => (backupCfg.schedule_mode = (v as BackupConfig['schedule_mode']) ?? 'interval')}
          />
        </div>

        {#if backupCfg.schedule_mode === 'interval'}
          <div class="field">
            <span class="field-label">{$t('settings_backup_interval_hours')}</span>
            <input class="field-input" type="number" min="1" bind:value={backupCfg.interval_hours} />
          </div>
        {:else}
          <div class="field">
            <span class="field-label">{$t('settings_backup_time')}</span>
            <input class="field-input" type="time" bind:value={backupCfg.time} />
          </div>
          {#if backupCfg.schedule_mode === 'weekly'}
            <div class="field">
              <span class="field-label">{$t('settings_backup_weekday')}</span>
              <CustomSelect
                options={weekdayOptions}
                value={String(backupCfg.weekday)}
                onchange={(v) => (backupCfg.weekday = Number(v ?? 0))}
              />
            </div>
          {/if}
        {/if}

        <div class="field">
          <span class="field-label">{$t('settings_backup_keep')}</span>
          <input class="dir-input" type="number" min="0" bind:value={backupCfg.keep} />
        </div>
      </div>
    {/if}

    <div class="btn-row">
      <button class="btn btn-primary btn-sm" disabled={backupSaving} onclick={saveBackupConfig}>
        {backupSaving ? $t('settings_backup_saving') : $t('settings_backup_save')}
      </button>
      <button
        class="btn btn-ghost btn-sm"
        disabled={!isTauri || backupRunning || !backupCfg.dir || !backupCfg.password}
        onclick={runBackupNow}
      >
        {backupRunning ? $t('settings_backup_running') : $t('settings_backup_run_now')}
      </button>
      {#if backupSaved}<span class="ok-msg">✓</span>{/if}
    </div>

    {#if backupRunning}
      <div class="progress-wrap">
        <div class="progress-bar">
          <div class="progress-fill" style="width: {backupProgress}%"></div>
        </div>
        <span class="progress-label">{backupPhase} · {backupProgress}%</span>
      </div>
    {/if}
    {#if backupDone}<p class="ok-msg">{backupDone}</p>{/if}
    {#if backupError}<div class="error-msg">{backupError}</div>{/if}

    <!-- Existing backups -->
    <div class="field">
      <span class="field-label">{$t('settings_backup_existing')}</span>
      {#if backupList.length === 0}
        <p class="muted small">{$t('settings_backup_none')}</p>
      {:else}
        <div class="backup-list">
          {#each backupList as b (b.path)}
            <div class="backup-item">
              <div class="backup-meta">
                <span class="backup-name">{b.name}</span>
                <span class="muted small">{formatBytes(b.size)}</span>
              </div>
              {#if isTauri}
                <button class="btn btn-ghost btn-sm" onclick={() => openRestore(b)}>
                  {$t('settings_backup_restore')}
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <div class="version-table">
      <div class="version-row">
        <span class="version-label">{$t('settings_backup_last_run')}</span>
        <span class="version-value">
          {backupCfg.last_run ? new Date(backupCfg.last_run).toLocaleString() : $t('settings_backup_never')}
        </span>
      </div>
    </div>
  </div>

  <!-- Sync (beta) -->
  <div class="card">
    <div class="card-title">{$t('settings_sync_section')} <span class="badge badge-warn">{$t('settings_sync_beta')}</span></div>
    <p class="muted">{$t('settings_sync_hint')}</p>
    <SyncSettings />
  </div>

  <!-- App updates -->
  <div class="card">
    <div class="card-title">{$t('settings_update_section')}</div>

    <div class="version-table">
      <div class="version-row">
        <span class="version-label">{$t('settings_about_version')}</span>
        <span class="version-value">{appVersion ? `v${appVersion}` : '—'}</span>
      </div>
    </div>

    <div class="btn-row">
      {#if updaterStore.status === 'available' && updaterStore.supported}
        <button class="btn btn-primary btn-sm" onclick={() => updaterStore.install()}>
          {$t('settings_update_install')}
        </button>
      {/if}
      <button
        class="btn btn-ghost btn-sm"
        disabled={updaterStore.status === 'checking' ||
          updaterStore.status === 'downloading' ||
          updaterStore.status === 'installing'}
        onclick={() => updaterStore.check(false)}
      >
        {updaterStore.status === 'checking'
          ? $t('settings_update_checking')
          : $t('settings_update_check')}
      </button>
    </div>

    {#if updaterStore.status === 'upToDate'}
      <p class="ok-msg">{$t('settings_update_up_to_date')}</p>
    {:else if updaterStore.status === 'available'}
      <p class="warn-msg">{$t('settings_update_available', { version: updaterStore.version })}</p>
      {#if !updaterStore.supported}
        <p class="muted small">{$t('settings_update_unsupported')}</p>
        <div class="btn-row">
          <button class="btn btn-ghost btn-sm" onclick={() => openExternal(RELEASES_URL)}>
            <Icon name="external-link" size={14} />
            {$t('settings_update_open_releases')}
          </button>
        </div>
      {/if}
    {:else if updaterStore.status === 'downloading'}
      <div class="progress-wrap">
        <div class="progress-bar">
          <div class="progress-fill" style="width: {updaterStore.progress}%"></div>
        </div>
        <span class="progress-label">
          {$t('settings_update_downloading')} · {updaterStore.progress}%
        </span>
      </div>
    {:else if updaterStore.status === 'installing'}
      <p class="muted small">{$t('settings_update_installing')}</p>
    {:else if updaterStore.status === 'error'}
      <div class="error-msg">{updaterStore.error}</div>
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

{#if restoreTarget}
  <Dialog open={true} onclose={closeRestore} title={$t('settings_backup_restore_title')}>
    <div class="restore-body">
      <p class="backup-name">{restoreTarget.name}</p>
      <p class="warn-msg">{$t('settings_backup_restore_warn')}</p>
      <input
        class="field-input"
        type="password"
        bind:value={restorePassword}
        placeholder={$t('settings_backup_password')}
        autocomplete="off"
        disabled={restoring}
      />
      {#if restoring}
        <div class="progress-wrap">
          <div class="progress-bar">
            <div class="progress-fill" style="width: {restorePercent}%"></div>
          </div>
          <span class="progress-label">{restorePhase} · {restorePercent}%</span>
        </div>
      {/if}
      {#if restoreError}<div class="error-msg">{restoreError}</div>{/if}
    </div>
    {#snippet footer()}
      <button class="btn btn-ghost btn-sm" disabled={restoring} onclick={closeRestore}>
        {$t('settings_backup_cancel')}
      </button>
      <button class="btn btn-danger btn-sm" disabled={!restorePassword || restoring} onclick={confirmRestore}>
        {restoring ? $t('settings_backup_restoring') : $t('settings_backup_restore_confirm')}
      </button>
    {/snippet}
  </Dialog>
{/if}

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
    width: 44px; height: 44px;
    flex-shrink: 0;
  }
  .about-logo img { width: 44px; height: 44px; object-fit: contain; }
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

  /* Separate stacked toggle rows within one card (e.g. the tray section) */
  .dev-tools-row + .dev-tools-row {
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border);
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
  .theme-customize {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    margin-top: var(--sp-4);
    padding-top: var(--sp-4);
    border-top: 1px solid var(--border);
  }
  .theme-reset { align-self: flex-start; }

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

  /* Backup section */
  .field { display: flex; flex-direction: column; gap: var(--sp-2); }
  .field-label { font-size: var(--fs-sm); color: var(--text); font-weight: var(--fw-semibold); }
  .field .dir-input { width: 100%; }

  /* Standard form input (password / number / time) — matches CustomSelect box */
  .field-input {
    width: 100%; height: var(--control-h-lg);
    background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius-field); padding: 0 var(--sp-3);
    font-size: var(--fs-base); color: var(--text); font-family: inherit;
    outline: none; transition: border-color var(--dur-fast), box-shadow var(--dur-fast);
  }
  .field-input:focus { border-color: var(--accent-border); box-shadow: 0 0 0 3px var(--accent-bg); }

  .sched-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--sp-3);
  }

  .backup-list { display: flex; flex-direction: column; gap: var(--sp-2); }
  .backup-item {
    display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-3); border: 1px solid var(--border); border-radius: var(--radius);
  }
  .backup-meta { display: flex; flex-direction: column; gap: 0.1rem; min-width: 0; }
  .backup-name {
    font-size: var(--fs-sm); color: var(--text-body); font-family: var(--font-mono);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .restore-body { display: flex; flex-direction: column; gap: var(--sp-3); }
</style>
