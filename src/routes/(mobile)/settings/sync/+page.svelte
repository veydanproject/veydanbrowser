<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { api, formatError, onSyncProgress, onSyncStatus, syncProgressText, type SyncConfig, type SyncProbe, type SyncProgress, type SyncStatus } from '$lib/mobile/api';
  import { t, locale } from '$lib/mobile/i18n';

  let cfg = $state<SyncConfig | null>(null);
  let status = $state<SyncStatus | null>(null);
  let probe = $state<SyncProbe | null>(null);
  let progress = $state<SyncProgress | null>(null);
  let passphrase = $state('');
  let busy = $state(false);
  let error = $state('');
  let toast = $state('');
  let toastTimer: ReturnType<typeof setTimeout>;

  const running = $derived(!!status?.running);
  const progressLine = $derived(progress ? syncProgressText($t, progress) : '');

  const intervals = [30, 60, 300, 900, 3600];

  function showToast(msg: string) {
    clearTimeout(toastTimer);
    toast = msg;
    toastTimer = setTimeout(() => (toast = ''), 2000);
  }

  function intervalLabel(sec: number): string {
    return sec < 60 ? $t('sync_interval_sec', { n: String(sec) }) : $t('sync_interval_min', { n: String(sec / 60) });
  }

  function formatTime(iso: string | null): string {
    if (!iso) return $t('notes_sync_never');
    return new Date(iso).toLocaleString($locale, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
  }

  async function load() {
    try {
      [cfg, status] = await Promise.all([api.sync.getConfig(), api.sync.status()]);
    } catch (e) {
      error = formatError(e);
    }
  }

  async function save() {
    if (cfg) await api.sync.setConfig(cfg);
  }

  /** Save the form, then run `fn`; errors land in the banner. */
  async function run(fn: () => Promise<void>) {
    error = '';
    busy = true;
    try {
      await save();
      await fn();
    } catch (e) {
      error = formatError(e);
    } finally {
      busy = false;
    }
  }

  const doProbe = () =>
    run(async () => {
      probe = await api.sync.probe();
    });

  const doCreate = () =>
    run(async () => {
      status = await api.sync.createVault(passphrase);
      passphrase = '';
      probe = null;
      showToast($t('settings_sync_created'));
    });

  const doJoin = () =>
    run(async () => {
      status = await api.sync.joinVault(passphrase);
      passphrase = '';
      probe = null;
      showToast($t('settings_sync_joined'));
    });

  const doSyncNow = () =>
    run(async () => {
      status = await api.sync.runNow();
      showToast($t('settings_sync_done'));
    });

  const doLeave = () =>
    run(async () => {
      if (!confirm($t('settings_sync_leave_confirm'))) return;
      status = await api.sync.leave();
      if (cfg) cfg.enabled = false;
      showToast($t('settings_sync_left'));
    });

  const toggleEnabled = () =>
    run(async () => {
      if (cfg) cfg.enabled = !cfg.enabled;
    });

  onMount(() => {
    load();
    const unStatus = onSyncStatus(() =>
      api.sync.status().then((s) => {
        status = s;
        if (!s.running) progress = null;
      }).catch(() => {}),
    );
    const unProgress = onSyncProgress((p) => (progress = p));
    return () => {
      unStatus.then((f) => f());
      unProgress.then((f) => f());
    };
  });
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/settings" aria-label={$t('common_back')}>
      <Icon name="chevron-left" size={24} />
    </a>
    <h1 class="m-title">{$t('settings_sync_section')}</h1>
  </div>

  <div class="m-body">
  {#if error}
    <div class="m-error">{error}</div>
  {/if}

  {#if cfg && status}
    {#if status.joined}
      <div class="m-section">{$t('settings_sync_status')}</div>
      <div class="m-list group">
        <div class="m-row">
          <span class="m-row-label">{$t('settings_sync_vault_id')}</span>
          <span class="m-row-value mono">{status.vault_id?.slice(0, 12)}</span>
        </div>
        <div class="m-row">
          <span class="m-row-label">{$t('settings_sync_device_id')}</span>
          <span class="m-row-value mono">{status.device_id}</span>
        </div>
        <div class="m-row">
          <span class="m-row-label">{$t('settings_sync_peers')}</span>
          <span class="m-row-value">{status.peers}</span>
        </div>
        <div class="m-row">
          <span class="m-row-label">{$t('settings_sync_last_run')}</span>
          <span class="m-row-value">{running ? (progressLine || $t('settings_sync_running')) : formatTime(status.last_run)}</span>
        </div>
        {#if running && progress}
          <div class="m-row prog">
            <div class="bar"><div class="fill" style="width: {progress.percent}%"></div></div>
            <span class="prog-label">
              {progressLine}
              {#if progress.detail}<span class="prog-detail">{progress.detail}</span>{/if}
            </span>
          </div>
        {/if}
        {#if status.last_error && !running}
          <div class="m-row msg danger">{status.last_error}</div>
        {:else if status.last_warning}
          <div class="m-row msg warn">{status.last_warning}</div>
        {/if}
        <button class="m-row" onclick={toggleEnabled} disabled={busy}>
          <span class="m-row-label">{$t('settings_sync_enabled')}</span>
          <span class="toggle" class:on={cfg.enabled}></span>
        </button>
      </div>

      <div class="actions group">
        <button class="btn btn-primary" onclick={doSyncNow} disabled={busy || running}>
          <Icon name="refresh-cw" size={16} />
          {running ? (progress ? `${progress.percent}%` : $t('settings_sync_running')) : $t('settings_sync_now')}
        </button>
        <button class="btn btn-ghost danger" onclick={doLeave} disabled={busy}>
          <Icon name="shield-off" size={16} />
          {$t('settings_sync_leave')}
        </button>
      </div>
    {/if}

    <div class="m-section">{$t('settings_sync_backend')}</div>
    <div class="seg backend group">
      <button class="seg-btn" class:active={cfg.backend === 'webdav'} onclick={() => (cfg!.backend = 'webdav')} disabled={status.joined}>
        {$t('settings_sync_backend_webdav')}
      </button>
      <button class="seg-btn" class:active={cfg.backend === 's3'} onclick={() => (cfg!.backend = 's3')} disabled={status.joined}>
        {$t('settings_sync_backend_s3')}
      </button>
    </div>

    {#if cfg.backend === 'webdav'}
      <div class="m-field">
        <label for="dav-url">{$t('settings_sync_webdav_url')}</label>
        <input id="dav-url" type="url" bind:value={cfg.webdav.url} placeholder="https://cloud.example.com/remote.php/dav/files/user/veydan" autocomplete="off" autocapitalize="off" />
      </div>
      <div class="m-field">
        <label for="dav-user">{$t('settings_sync_webdav_username')}</label>
        <input id="dav-user" bind:value={cfg.webdav.username} autocomplete="off" autocapitalize="off" />
      </div>
      <div class="m-field">
        <label for="dav-pass">{$t('settings_sync_webdav_password')}</label>
        <input id="dav-pass" type="password" bind:value={cfg.webdav.password} autocomplete="off" />
      </div>
    {:else}
      <div class="m-field">
        <label for="s3-endpoint">{$t('settings_sync_s3_endpoint')}</label>
        <input id="s3-endpoint" type="url" bind:value={cfg.s3.endpoint} placeholder="https://s3.example.com" autocomplete="off" autocapitalize="off" />
      </div>
      <div class="two">
        <div class="m-field">
          <label for="s3-bucket">{$t('settings_sync_s3_bucket')}</label>
          <input id="s3-bucket" bind:value={cfg.s3.bucket} autocomplete="off" autocapitalize="off" />
        </div>
        <div class="m-field">
          <label for="s3-region">{$t('settings_sync_s3_region')}</label>
          <input id="s3-region" bind:value={cfg.s3.region} placeholder="us-east-1" autocomplete="off" autocapitalize="off" />
        </div>
      </div>
      <div class="m-field">
        <label for="s3-prefix">{$t('settings_sync_s3_prefix')}</label>
        <input id="s3-prefix" bind:value={cfg.s3.prefix} autocomplete="off" autocapitalize="off" />
      </div>
      <div class="m-field">
        <label for="s3-ak">{$t('settings_sync_s3_access_key')}</label>
        <input id="s3-ak" bind:value={cfg.s3.access_key} autocomplete="off" autocapitalize="off" />
      </div>
      <div class="m-field">
        <label for="s3-sk">{$t('settings_sync_s3_secret_key')}</label>
        <input id="s3-sk" type="password" bind:value={cfg.s3.secret_key} autocomplete="off" />
      </div>
      <div class="m-list group">
        <button class="m-row" onclick={() => (cfg!.s3.path_style = !cfg!.s3.path_style)}>
          <span class="m-row-label">{$t('settings_sync_s3_path_style')}</span>
          <span class="toggle" class:on={cfg.s3.path_style}></span>
        </button>
      </div>
    {/if}

    <div class="m-field">
      <label for="interval">{$t('settings_sync_interval')}</label>
      <select id="interval" bind:value={cfg.interval_sec}>
        {#each intervals as sec (sec)}
          <option value={sec}>{intervalLabel(sec)}</option>
        {/each}
      </select>
    </div>
    <div class="m-field">
      <label for="device">{$t('settings_sync_device_name')}</label>
      <input id="device" bind:value={cfg.device_name} autocomplete="off" />
    </div>

    {#if !status.joined}
      <button class="btn btn-ghost wide" onclick={doProbe} disabled={busy}>
        <Icon name="search" size={16} />
        {$t('settings_sync_probe')}
      </button>

      {#if probe}
        <p class="probe" class:danger={probe === 'foreign'}>{$t(`settings_sync_probe_${probe}`)}</p>
        {#if probe !== 'foreign'}
          <div class="m-field">
            <label for="pass">{$t('settings_sync_passphrase')}</label>
            <input id="pass" type="password" bind:value={passphrase} placeholder={$t('settings_sync_passphrase_placeholder')} autocomplete="off" />
          </div>
          {#if probe === 'empty'}
            <button class="btn btn-primary wide" onclick={doCreate} disabled={busy || passphrase.length < 8}>
              <Icon name="plus" size={16} />
              {$t('settings_sync_create')}
            </button>
          {:else}
            <button class="btn btn-primary wide" onclick={doJoin} disabled={busy || !passphrase}>
              <Icon name="lock" size={16} />
              {$t('settings_sync_join')}
            </button>
          {/if}
        {/if}
      {/if}
    {:else}
      <button class="btn btn-ghost wide" onclick={() => run(async () => showToast($t('common_saved')))} disabled={busy}>
        <Icon name="check" size={16} />
        {$t('common_save')}
      </button>
    {/if}
  {/if}

  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
  </div>
</div>

<style>
  .group { margin-bottom: var(--sp-4); }
  .mono { font-family: var(--font-mono); }
  .backend { display: flex; width: 100%; }
  .backend .seg-btn { flex: 1; justify-content: center; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-3); }
  .actions { display: flex; gap: var(--sp-2); }
  .actions .btn { flex: 1; }
  .wide { width: 100%; margin-top: var(--sp-2); }
  .msg { font-size: var(--fs-sm); word-break: break-word; }
  .danger { color: var(--danger-text); }
  .warn { color: var(--warn-text); }
  .probe { font-size: var(--fs-sm); color: var(--text-2); margin: var(--sp-3) 0; }
  .prog { flex-direction: column; align-items: stretch; gap: 6px; padding-top: 10px; padding-bottom: 12px; }
  .bar { height: 6px; background: var(--surface-3); border-radius: 999px; overflow: hidden; }
  .fill { height: 100%; background: var(--accent); border-radius: 999px; transition: width 0.2s ease; }
  .prog-label { font-size: var(--fs-sm); color: var(--text-body); }
  .prog-detail { display: block; font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--text-3); word-break: break-all; }
</style>
