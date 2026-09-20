<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import type { SyncConfig, SyncStatus } from '$lib/api';
  import { formatError } from '$lib/utils';
  import CustomSelect from '$lib/components/CustomSelect.svelte';
  import Icon from '$lib/Icon.svelte';

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  let cfg = $state<SyncConfig>({
    enabled: false,
    backend: 'folder',
    folder_path: '',
    s3: { endpoint: '', region: '', bucket: '', prefix: '', access_key: '', secret_key: '', path_style: true },
    webdav: { url: '', username: '', password: '' },
    interval_sec: 60,
  });
  let status = $state<SyncStatus | null>(null);
  let passphrase = $state('');
  let oldPassphrase = $state('');
  let newPassphrase = $state('');
  let busy = $state(false);
  let saved = $state(false);
  let error = $state('');
  let info = $state('');
  let confirmLeave = $state(false);
  let unlisten: (() => void) | null = null;

  const backendOptions = $derived([
    { value: 'folder', label: $t('settings_sync_backend_folder') },
    { value: 's3', label: $t('settings_sync_backend_s3') },
    { value: 'webdav', label: $t('settings_sync_backend_webdav') },
  ]);

  const intervalMin = $derived(Math.max(1, Math.round(cfg.interval_sec / 60)));

  async function refreshStatus() {
    try {
      status = await api.sync.status();
    } catch {}
  }

  async function run(task: () => Promise<void>) {
    busy = true;
    error = '';
    info = '';
    try {
      await task();
    } catch (e) {
      error = formatError(e);
    } finally {
      busy = false;
      await refreshStatus();
    }
  }

  async function saveConfig() {
    await api.sync.setConfig($state.snapshot(cfg));
    cfg = await api.sync.getConfig();
    saved = true;
    setTimeout(() => (saved = false), 2000);
  }

  const save = () => run(saveConfig);

  const probe = () =>
    run(async () => {
      await saveConfig();
      const result = await api.sync.probe();
      info = $t(`settings_sync_probe_${result}` as any);
    });

  const createVault = () =>
    run(async () => {
      await saveConfig();
      status = await api.sync.createVault(passphrase);
      passphrase = '';
      info = $t('settings_sync_created');
    });

  const joinVault = () =>
    run(async () => {
      await saveConfig();
      status = await api.sync.joinVault(passphrase);
      passphrase = '';
      info = $t('settings_sync_joined');
    });

  const syncNow = () =>
    run(async () => {
      await saveConfig();
      status = await api.sync.runNow();
      info = $t('settings_sync_done');
    });

  const leave = () =>
    run(async () => {
      confirmLeave = false;
      status = await api.sync.leave();
      info = $t('settings_sync_left');
    });

  const changePassphrase = () =>
    run(async () => {
      await api.sync.changePassphrase(oldPassphrase, newPassphrase);
      oldPassphrase = '';
      newPassphrase = '';
      info = $t('settings_sync_passphrase_changed');
    });

  async function browseFolder() {
    if (!isTauri) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({ directory: true, multiple: false, title: $t('settings_sync_folder') });
      if (selected && typeof selected === 'string') cfg.folder_path = selected;
    } catch {}
  }

  function setIntervalMin(v: number) {
    cfg.interval_sec = Math.max(1, Math.floor(v || 1)) * 60;
  }

  onMount(async () => {
    try {
      cfg = await api.sync.getConfig();
    } catch {}
    await refreshStatus();
    if (isTauri) {
      const { listen } = await import('@tauri-apps/api/event');
      unlisten = await listen('sync://status', () => void refreshStatus());
    }
  });

  onDestroy(() => unlisten?.());
</script>

<div class="sync">
  <div class="field">
    <span class="field-label">{$t('settings_sync_backend')}</span>
    <CustomSelect options={backendOptions} value={cfg.backend} onchange={(v) => (cfg.backend = v ?? 'folder')} />
  </div>

  {#if cfg.backend === 'folder'}
    <div class="field">
      <span class="field-label">{$t('settings_sync_folder')}</span>
      <div class="row">
        <input class="input mono" type="text" bind:value={cfg.folder_path} placeholder={$t('settings_sync_folder_placeholder')} readonly={!isTauri} />
        {#if isTauri}
          <button class="btn btn-ghost btn-sm icon" onclick={browseFolder} title={$t('settings_backup_browse')}>
            <Icon name="folder-open" size={14} />
          </button>
        {/if}
      </div>
      <p class="hint">{$t('settings_sync_folder_hint')}</p>
    </div>
  {:else if cfg.backend === 's3'}
    <div class="grid">
      <label class="field"><span class="field-label">{$t('settings_sync_s3_endpoint')}</span><input class="input" type="text" bind:value={cfg.s3.endpoint} placeholder="https://s3.example.com" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_s3_region')}</span><input class="input" type="text" bind:value={cfg.s3.region} placeholder="us-east-1" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_s3_bucket')}</span><input class="input" type="text" bind:value={cfg.s3.bucket} /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_s3_prefix')}</span><input class="input" type="text" bind:value={cfg.s3.prefix} placeholder="veydan" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_s3_access_key')}</span><input class="input" type="text" bind:value={cfg.s3.access_key} autocomplete="off" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_s3_secret_key')}</span><input class="input" type="password" bind:value={cfg.s3.secret_key} autocomplete="off" /></label>
    </div>
    <div class="toggle-row">
      <div class="toggle-info">
        <span>{$t('settings_sync_s3_path_style')}</span>
        <span class="hint">{$t('settings_sync_s3_path_style_hint')}</span>
      </div>
      <button class="toggle" class:on={cfg.s3.path_style} onclick={() => (cfg.s3.path_style = !cfg.s3.path_style)} aria-pressed={cfg.s3.path_style} aria-label={$t('settings_sync_s3_path_style')}></button>
    </div>
  {:else}
    <div class="grid">
      <label class="field wide"><span class="field-label">{$t('settings_sync_webdav_url')}</span><input class="input" type="text" bind:value={cfg.webdav.url} placeholder="https://webdav.example.com/veydan" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_webdav_username')}</span><input class="input" type="text" bind:value={cfg.webdav.username} autocomplete="off" /></label>
      <label class="field"><span class="field-label">{$t('settings_sync_webdav_password')}</span><input class="input" type="password" bind:value={cfg.webdav.password} autocomplete="off" /></label>
    </div>
  {/if}

  <div class="grid">
    <label class="field">
      <span class="field-label">{$t('settings_sync_interval')}</span>
      <input class="input" type="number" min="1" value={intervalMin} oninput={(e) => setIntervalMin(Number((e.currentTarget as HTMLInputElement).value))} />
    </label>
  </div>

  {#if status?.joined}
    <div class="toggle-row">
      <div class="toggle-info">
        <span>{$t('settings_sync_enabled')}</span>
        <span class="hint">{$t('settings_sync_enabled_hint')}</span>
      </div>
      <button class="toggle" class:on={cfg.enabled} onclick={() => (cfg.enabled = !cfg.enabled)} aria-pressed={cfg.enabled} aria-label={$t('settings_sync_enabled')}></button>
    </div>
  {/if}

  <div class="row">
    <button class="btn btn-primary btn-sm" disabled={busy} onclick={save}>{$t('settings_backup_save')}</button>
    {#if status?.joined}
      <button class="btn btn-ghost btn-sm" disabled={busy || !isTauri || status.running} onclick={syncNow}>
        {status.running ? $t('settings_sync_running') : $t('settings_sync_now')}
      </button>
      <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => (confirmLeave = true)}>{$t('settings_sync_leave')}</button>
    {:else}
      <button class="btn btn-ghost btn-sm" disabled={busy || !isTauri} onclick={probe}>{$t('settings_sync_probe')}</button>
    {/if}
    {#if saved}<span class="ok">✓</span>{/if}
  </div>

  {#if !status?.joined}
    <div class="field">
      <span class="field-label">{$t('settings_sync_passphrase')}</span>
      <div class="row">
        <input class="input" type="password" bind:value={passphrase} placeholder={$t('settings_sync_passphrase_placeholder')} autocomplete="new-password" />
        <button class="btn btn-primary btn-sm" disabled={busy || !isTauri || passphrase.length < 8} onclick={createVault}>{$t('settings_sync_create')}</button>
        <button class="btn btn-ghost btn-sm" disabled={busy || !isTauri || !passphrase} onclick={joinVault}>{$t('settings_sync_join')}</button>
      </div>
      <p class="hint">{$t('settings_sync_passphrase_hint')}</p>
    </div>
  {/if}

  {#if confirmLeave}
    <div class="confirm">
      <span>{$t('settings_sync_leave_confirm')}</span>
      <div class="row">
        <button class="btn btn-danger btn-sm" disabled={busy} onclick={leave}>{$t('settings_sync_leave')}</button>
        <button class="btn btn-ghost btn-sm" onclick={() => (confirmLeave = false)}>{$t('settings_backup_cancel')}</button>
      </div>
    </div>
  {/if}

  {#if info}<p class="ok">{info}</p>{/if}
  {#if error}<p class="error">{error}</p>{/if}

  {#if status}
    <div class="table">
      <div class="trow"><span class="tlabel">{$t('settings_sync_status')}</span><span class="tvalue">{status.joined ? $t('settings_sync_status_joined') : $t('settings_sync_status_not_joined')}</span></div>
      {#if status.joined}
        <div class="trow"><span class="tlabel">{$t('settings_sync_vault_id')}</span><span class="tvalue">{status.vault_id?.slice(0, 12)}…</span></div>
        <div class="trow"><span class="tlabel">{$t('settings_sync_device_id')}</span><span class="tvalue">{status.device_id}</span></div>
        <div class="trow"><span class="tlabel">{$t('settings_sync_peers')}</span><span class="tvalue">{status.peers}</span></div>
        <div class="trow"><span class="tlabel">{$t('settings_sync_last_run')}</span><span class="tvalue">{status.last_run ? new Date(status.last_run).toLocaleString() : $t('settings_backup_never')}</span></div>
        {#if status.last_error}
          <div class="trow"><span class="tlabel">{$t('settings_sync_last_error')}</span><span class="tvalue error">{status.last_error}</span></div>
        {/if}
      {/if}
    </div>

    {#if status.conflicts.length > 0}
      <div class="field">
        <span class="field-label">{$t('settings_sync_conflicts')}</span>
        <p class="hint">{$t('settings_sync_conflicts_hint')}</p>
        <ul class="conflicts">
          {#each status.conflicts as c (c.note_id)}
            <li><a href="/notes?open={c.note_id}">{c.title}</a></li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if status.joined}
      <details class="passphrase">
        <summary>{$t('settings_sync_change_passphrase')}</summary>
        <div class="row">
          <input class="input" type="password" bind:value={oldPassphrase} placeholder={$t('settings_sync_passphrase_old')} autocomplete="off" />
          <input class="input" type="password" bind:value={newPassphrase} placeholder={$t('settings_sync_passphrase_new')} autocomplete="new-password" />
          <button class="btn btn-ghost btn-sm" disabled={busy || !oldPassphrase || newPassphrase.length < 8} onclick={changePassphrase}>{$t('settings_lock_change')}</button>
        </div>
      </details>
    {/if}
  {/if}
</div>

<style>
  .sync { display: flex; flex-direction: column; gap: var(--sp-4); }
  .row { display: flex; gap: var(--sp-2); align-items: center; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: var(--sp-3); }
  .field { display: flex; flex-direction: column; gap: var(--sp-2); min-width: 0; }
  .field.wide { grid-column: 1 / -1; }
  .field-label { font-size: var(--fs-sm); color: var(--text); font-weight: var(--fw-semibold); }
  .input {
    flex: 1; width: 100%; min-width: 0; height: var(--control-h-lg);
    background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius-field); padding: 0 var(--sp-3);
    font-size: 0.82rem; color: var(--text-body);
  }
  .input.mono { font-family: var(--font-mono); }
  .input:focus { border-color: var(--accent-border); box-shadow: 0 0 0 3px var(--accent-bg); outline: none; }
  .icon { width: var(--control-h-lg); height: var(--control-h-lg); justify-content: center; padding: 0; flex-shrink: 0; }
  .hint { font-size: var(--fs-sm); color: var(--text-dim); margin: 0; }
  .ok { font-size: var(--fs-sm); color: var(--success-text); margin: 0; }
  .error { font-size: var(--fs-sm); color: var(--danger-text); margin: 0; }
  .toggle-row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-4); }
  .toggle-info { display: flex; flex-direction: column; gap: 0.2rem; font-size: var(--fs-base); }
  .confirm { display: flex; flex-direction: column; gap: var(--sp-2); padding: var(--sp-3); border: 1px solid var(--danger-border); border-radius: var(--radius); background: var(--danger-bg); font-size: var(--fs-sm); }
  .table { display: flex; flex-direction: column; gap: 0.35rem; }
  .trow { display: flex; align-items: baseline; gap: var(--sp-2); }
  .tlabel { font-size: 0.82rem; color: var(--text-faint); min-width: 120px; flex-shrink: 0; }
  .tvalue { font-size: var(--fs-sm); color: var(--text-body); font-family: var(--font-mono); word-break: break-all; }
  .conflicts { margin: 0; padding-left: 1.2rem; font-size: var(--fs-sm); }
  .conflicts a { color: var(--warn-text); }
  .passphrase summary { font-size: var(--fs-sm); color: var(--text-dim); cursor: pointer; }
  .passphrase .row { margin-top: var(--sp-2); }
</style>
