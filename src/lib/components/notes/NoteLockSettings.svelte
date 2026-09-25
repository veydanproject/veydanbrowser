<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import { passwordErrorKey } from '$lib/password-error';

  let current = $state('');
  let next = $state('');
  let confirm = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  const TIMEOUTS = [0, 1, 5, 15, 30, 60];

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
  });

  const canSave = $derived(next.length >= 4 && next === confirm && (!notesLock.status.enabled || current.length > 0));

  async function run(task: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await task();
      current = '';
      next = '';
      confirm = '';
    } catch (e) {
      const key = passwordErrorKey(e);
      error = key ? $t(key) : formatError(e);
    } finally {
      busy = false;
    }
  }

  const save = () => run(() => notesLock.setPassword(next, current || undefined));
  const remove = () => run(() => notesLock.setPassword(null, current));
  const setTimeoutMin = (e: Event) =>
    run(() => notesLock.setTimeout(Number((e.currentTarget as HTMLSelectElement).value)));
</script>

<div class="lock-settings">
  {#if notesLock.status.enabled}
    <input class="field" type="password" bind:value={current} placeholder={$t('settings_lock_current')} autocomplete="off" />
  {/if}
  <div class="row">
    <input class="field" type="password" bind:value={next} placeholder={$t('settings_lock_new')} autocomplete="new-password" />
    <input class="field" type="password" bind:value={confirm} placeholder={$t('settings_lock_confirm')} autocomplete="new-password" />
  </div>
  <div class="row actions">
    <button class="btn btn-primary btn-sm" disabled={busy || !canSave} onclick={save}>
      {notesLock.status.enabled ? $t('settings_lock_change') : $t('settings_lock_enable')}
    </button>
    {#if notesLock.status.enabled}
      <button class="btn btn-ghost btn-sm" disabled={busy || !current} onclick={remove}>{$t('settings_lock_disable')}</button>
    {/if}
  </div>
  {#if notesLock.status.enabled}
    <label class="row timeout">
      <span>{$t('settings_lock_timeout')}</span>
      <select class="field" value={String(notesLock.status.timeout_min)} onchange={setTimeoutMin} disabled={busy}>
        {#each TIMEOUTS as m (m)}
          <option value={String(m)}>{m === 0 ? $t('settings_lock_timeout_never') : $t('settings_lock_timeout_min', { n: String(m) })}</option>
        {/each}
      </select>
    </label>
  {/if}
  {#if error}<div class="error-msg">{error}</div>{/if}
</div>

<style>
  .lock-settings { display: flex; flex-direction: column; gap: 8px; }
  .row { display: flex; gap: 8px; align-items: center; }
  .field {
    flex: 1;
    min-width: 0;
    padding: 6px 8px;
    font-size: 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-2);
    color: var(--text);
  }
  .timeout span { font-size: 12px; color: var(--text-2); white-space: nowrap; }
  .timeout .field { flex: 0 1 180px; }
  .error-msg { font-size: 12px; color: var(--danger); }
</style>
