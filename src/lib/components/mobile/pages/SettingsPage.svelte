<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { theme, type Theme } from '$lib/theme';
  import { api as shared } from '$lib/api';
  import type { HostInfo } from '$lib/types';
  import { api, onSyncStatus, type SyncStatus } from '$lib/mobile/api';
  import { t, locale, type Locale } from '$lib/mobile/i18n';
  import { APPS, loadDefaultApp, saveDefaultApp } from '$lib/mobile/apps';
  import PickerSheet from '$lib/components/mobile/PickerSheet.svelte';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import BugReportDialog from '$lib/components/BugReportDialog.svelte';
  import { formatError } from '$lib/utils';
  import { notesStore } from '$lib/store/notes.svelte';
  import { totpStore } from '$lib/store/totp.svelte';

  const locales: { id: Locale; label: string }[] = [
    { id: 'en', label: 'English' },
    { id: 'ru', label: 'Русский' },
  ];

  const demoLocales: { id: 'en' | 'ru'; label: string }[] = [
    { id: 'en', label: 'English' },
    { id: 'ru', label: 'Русский' },
  ];

  let picker = $state<'none' | 'app' | 'theme' | 'lang' | 'demoLang'>('none');
  let defaultApp = $state(loadDefaultApp());
  let info = $state<HostInfo | null>(null);
  let sync = $state<SyncStatus | null>(null);
  let demoLocale = $state<'en' | 'ru'>('ru');
  let demoBusy = $state(false);
  let clearBusy = $state(false);
  let pendingData = $state<'load' | 'clear' | null>(null);
  let dataMsg = $state('');
  let dataError = $state('');
  let bugOpen = $state(false);

  const themeOptions = $derived([
    { id: 'light', label: $t('settings_theme_light'), icon: 'sun' },
    { id: 'dark', label: $t('settings_theme_dark'), icon: 'moon' },
  ]);
  const appOptions = $derived([
    { id: '', label: $t('settings_default_home'), icon: 'home' },
    ...APPS.filter((a) => a.id !== 'settings').map((a) => ({ id: a.id, label: $t(a.title), icon: a.icon })),
  ]);
  const defaultAppLabel = $derived(appOptions.find((o) => o.id === defaultApp)?.label ?? '');
  const themeLabel = $derived(themeOptions.find((o) => o.id === $theme)?.label ?? '');
  const localeLabel = $derived(locales.find((l) => l.id === $locale)?.label ?? '');
  const demoLocaleLabel = $derived(demoLocales.find((l) => l.id === demoLocale)?.label ?? '');
  const syncLabel = $derived(
    sync?.running ? $t('settings_sync_running')
    : sync?.enabled && sync.joined ? $t('settings_sync_connected')
    : $t('settings_sync_disconnected'),
  );

  function pickApp(id: string) {
    defaultApp = id;
    saveDefaultApp(id);
  }

  async function refreshStores() {
    await Promise.all([
      notesStore.refresh(),
      notesStore.refreshTags(),
      notesStore.refreshFolders(),
      notesStore.refreshSmartViews(),
      totpStore.refresh(),
    ]);
  }

  function askLoadDemo() {
    if (demoBusy || clearBusy) return;
    pendingData = 'load';
  }

  function askClearData() {
    if (demoBusy || clearBusy) return;
    pendingData = 'clear';
  }

  async function confirmDataAction() {
    const action = pendingData;
    pendingData = null;
    if (action === 'load') await loadDemoData();
    else if (action === 'clear') await clearAppData();
  }

  async function loadDemoData() {
    if (demoBusy || clearBusy) return;
    demoBusy = true;
    dataMsg = '';
    dataError = '';
    try {
      await shared.demo.seed(demoLocale);
      await refreshStores();
      dataMsg = $t('settings_demo_done');
    } catch (e) {
      dataError = formatError(e);
    } finally {
      demoBusy = false;
    }
  }

  async function clearAppData() {
    if (demoBusy || clearBusy) return;
    clearBusy = true;
    dataMsg = '';
    dataError = '';
    try {
      await shared.app.clearData();
      await refreshStores();
      dataMsg = $t('settings_clear_done');
    } catch (e) {
      dataError = formatError(e);
    } finally {
      clearBusy = false;
    }
  }

  onMount(() => {
    void shared.system.hostInfo().then((h) => (info = h));
    void api.sync.status().then((s) => (sync = s)).catch(() => {});
    const un = onSyncStatus(() => api.sync.status().then((s) => (sync = s)).catch(() => {}));
    return () => un.then((f) => f());
  });
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    <h1 class="m-title">{$t('app_settings')}</h1>
  </div>

  <div class="m-body">
  <div class="m-section">{$t('settings_main')}</div>
  <div class="m-list">
    <button type="button" class="m-row" onclick={() => (picker = 'app')}>
      <span class="m-row-label">{$t('settings_default_app')}</span>
      <span class="m-row-value">{defaultAppLabel}</span>
      <span class="chev"><Icon name="chevron-right" size={16} /></span>
    </button>
    <button type="button" class="m-row" onclick={() => (picker = 'theme')}>
      <span class="m-row-label">{$t('settings_theme')}</span>
      <span class="m-row-value">{themeLabel}</span>
      <span class="chev"><Icon name="chevron-right" size={16} /></span>
    </button>
    <button type="button" class="m-row" onclick={() => (picker = 'lang')}>
      <span class="m-row-label">{$t('settings_language')}</span>
      <span class="m-row-value">{localeLabel}</span>
      <span class="chev"><Icon name="chevron-right" size={16} /></span>
    </button>
  </div>

  <div class="m-section">{$t('settings_sync_section')}</div>
  <div class="m-list">
    <a class="m-row" href="/settings/sync">
      <span class="m-row-label">{$t('settings_sync_section')}</span>
      <span class="m-row-value" class:ok={sync?.enabled && sync?.joined}>{sync ? syncLabel : ''}</span>
      <span class="chev"><Icon name="chevron-right" size={16} /></span>
    </a>
  </div>

  <div class="m-section">{$t('settings_about')}</div>
  <div class="m-list">
    <div class="m-row about-head">
      <img src="/logo.png" alt="" class="about-logo" />
      <div class="about-text">
        <span class="m-row-label">Veydan</span>
        <span class="m-row-value mono">{info ? `${info.version} · ${info.os}/${info.arch}` : ''}</span>
      </div>
    </div>
    <button type="button" class="m-row" onclick={() => (bugOpen = true)}>
      <span class="m-row-label">{$t('settings_bug_report')}</span>
      <span class="chev"><Icon name="chevron-right" size={16} /></span>
    </button>
  </div>

  <BugReportDialog bind:open={bugOpen} />

  <div class="demo-foot">
    <button type="button" class="demo-chip" onclick={() => (picker = 'demoLang')}>
      {$t('settings_demo_locale')}: {demoLocaleLabel}
    </button>
    <button type="button" class="demo-link" disabled={demoBusy || clearBusy} onclick={askLoadDemo}>
      {demoBusy ? $t('settings_demo_loading') : $t('settings_demo_load')}
    </button>
    <button type="button" class="demo-link danger" disabled={demoBusy || clearBusy} onclick={askClearData}>
      {clearBusy ? $t('settings_clear_clearing') : $t('settings_clear_data')}
    </button>
  </div>
  {#if dataMsg}<p class="ok-msg">{dataMsg}</p>{/if}
  {#if dataError}<p class="err-msg">{dataError}</p>{/if}
  </div>
</div>

{#if pendingData}
  <Dialog
    open={true}
    onclose={() => (pendingData = null)}
    title={pendingData === 'clear' ? $t('settings_clear_data') : $t('settings_demo_load')}
  >
    <p>{pendingData === 'clear' ? $t('settings_clear_confirm') : $t('settings_demo_confirm')}</p>
    {#snippet footer()}
      <button class="btn btn-ghost btn-sm" onclick={() => (pendingData = null)}>
        {$t('settings_backup_cancel')}
      </button>
      <button
        class="btn btn-sm"
        class:btn-danger={pendingData === 'clear'}
        class:btn-primary={pendingData !== 'clear'}
        onclick={confirmDataAction}
      >
        {pendingData === 'clear' ? $t('settings_clear_data') : $t('settings_demo_load')}
      </button>
    {/snippet}
  </Dialog>
{/if}

<PickerSheet open={picker === 'app'} title={$t('settings_default_app')} options={appOptions} value={defaultApp} onclose={() => (picker = 'none')} onpick={pickApp} />
<PickerSheet open={picker === 'theme'} title={$t('settings_theme')} options={themeOptions} value={$theme} onclose={() => (picker = 'none')} onpick={(id) => ($theme = id as Theme)} />
<PickerSheet open={picker === 'lang'} title={$t('settings_language')} options={locales} value={$locale} onclose={() => (picker = 'none')} onpick={(id) => ($locale = id as Locale)} />
<PickerSheet
  open={picker === 'demoLang'}
  title={$t('settings_demo_locale')}
  options={demoLocales}
  value={demoLocale}
  onclose={() => (picker = 'none')}
  onpick={(id) => { demoLocale = id as 'en' | 'ru'; }}
/>

<style>
  .ok { color: var(--success-text); font-weight: 600; }
  .about-head { gap: var(--sp-3); }
  .about-logo { width: 40px; height: 40px; object-fit: contain; flex-shrink: 0; }
  .about-text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
  .demo-foot {
    display: flex; flex-wrap: wrap; align-items: center; gap: 0.15rem 0.5rem;
    padding: var(--sp-4) var(--sp-4) var(--sp-6);
  }
  .demo-chip, .demo-link {
    border: none; background: transparent; color: var(--text-faint);
    font-size: 12px; line-height: 1.2; padding: 0.1rem 0.15rem; cursor: pointer;
  }
  .demo-chip:hover, .demo-link:hover:not(:disabled) { color: var(--text); text-decoration: underline; }
  .demo-link.danger:hover:not(:disabled) { color: var(--danger-text); }
  .demo-link:disabled { opacity: 0.45; cursor: default; }
  .ok-msg { margin: 0 var(--sp-4) var(--sp-2); font-size: 13px; color: var(--success-text); }
  .err-msg { margin: 0 var(--sp-4) var(--sp-2); font-size: 13px; color: var(--danger-text); }
</style>
