<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import { passwordErrorKey } from '$lib/password-error';

  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  const needsLock = $derived(notesLock.ready && !notesLock.status.enabled && notesLock.status.vault !== 'ok');
  const mismatch = $derived(
    notesLock.status.enabled && !notesLock.status.locked && notesLock.status.vault === 'mismatch',
  );

  async function reset() {
    if (!password || busy) return;
    if (!confirm($t('pw_reset_confirm'))) return;
    busy = true;
    error = '';
    try {
      await api.passwords.vaultReset(password);
      await notesLock.refresh();
      password = '';
    } catch (e) {
      const key = passwordErrorKey(e);
      error = key ? $t(key) : formatError(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if needsLock}
  <p class="hint">{$t('pw_need_lock')}</p>
{:else if mismatch}
  <form class="hint" onsubmit={(e) => { e.preventDefault(); void reset(); }}>
    <p>{$t('pw_mismatch')}</p>
    <input type="password" bind:value={password} autocomplete="off" placeholder={$t('notes_lock_password')} />
    {#if error}<span class="err">{error}</span>{/if}
    <button class="btn btn-ghost btn-sm" type="submit" disabled={!password || busy}>{$t('pw_reset')}</button>
  </form>
{/if}

<style>
  .hint {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-3);
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text-2);
    font-size: 0.85rem;
  }
  input {
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
  }
  .err { color: var(--danger-text); }
</style>
