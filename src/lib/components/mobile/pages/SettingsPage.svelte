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

  const locales: { id: Locale; label: string }[] = [
    { id: 'en', label: 'English' },
    { id: 'ru', label: 'Русский' },
  ];

  let picker = $state<'none' | 'app' | 'theme' | 'lang'>('none');
  let defaultApp = $state(loadDefaultApp());
  let info = $state<HostInfo | null>(null);
  let sync = $state<SyncStatus | null>(null);

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
  const syncLabel = $derived(
    sync?.running ? $t('settings_sync_running')
    : sync?.enabled && sync.joined ? $t('settings_sync_connected')
    : $t('settings_sync_disconnected'),
  );

  function pickApp(id: string) {
    defaultApp = id;
    saveDefaultApp(id);
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
  </div>
  </div>
</div>

<PickerSheet open={picker === 'app'} title={$t('settings_default_app')} options={appOptions} value={defaultApp} onclose={() => (picker = 'none')} onpick={pickApp} />
<PickerSheet open={picker === 'theme'} title={$t('settings_theme')} options={themeOptions} value={$theme} onclose={() => (picker = 'none')} onpick={(id) => ($theme = id as Theme)} />
<PickerSheet open={picker === 'lang'} title={$t('settings_language')} options={locales} value={$locale} onclose={() => (picker = 'none')} onpick={(id) => ($locale = id as Locale)} />

<style>
  .ok { color: var(--success-text); font-weight: 600; }
  .about-head { gap: var(--sp-3); }
  .about-logo { width: 40px; height: 40px; object-fit: contain; flex-shrink: 0; }
  .about-text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .mono { font-family: var(--font-mono); font-size: 12px; }
</style>
