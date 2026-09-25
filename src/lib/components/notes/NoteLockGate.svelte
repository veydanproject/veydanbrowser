<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let password = $state('');
  let error = $state(false);
  let busy = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
  });

  $effect(() => {
    if (notesLock.locked) {
      password = '';
      error = false;
      queueMicrotask(() => input?.focus());
    }
  });

  async function submit() {
    if (!password || busy) return;
    busy = true;
    try {
      await notesLock.unlock(password);
      error = false;
    } catch {
      error = true;
      password = '';
      input?.focus();
    } finally {
      busy = false;
    }
  }
</script>

{#if !notesLock.ready}
  <div class="lock-screen"></div>
{:else if notesLock.locked}
  <div class="lock-screen">
    <form class="lock-card" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
      <Icon name="lock" size={28} />
      <h3>{$t('notes_lock_title')}</h3>
      <input
        bind:this={input}
        type="password"
        bind:value={password}
        placeholder={$t('notes_lock_password')}
        autocomplete="off"
        class:invalid={error}
      />
      {#if error}<span class="err">{$t('notes_lock_wrong')}</span>{/if}
      <button class="btn btn-primary" type="submit" disabled={!password || busy}>{$t('notes_lock_unlock')}</button>
    </form>
  </div>
{:else}
  {@render children()}
{/if}

<style>
  .lock-screen {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    height: 100%;
    background: var(--bg);
  }
  .lock-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    width: 280px;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
    color: var(--text-2);
  }
  .lock-card h3 { margin: 0; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .lock-card input { width: 100%; }
  .lock-card input.invalid { border-color: var(--danger); }
  .lock-card .btn { width: 100%; justify-content: center; }
  .err { font-size: var(--fs-xs); color: var(--danger); }
</style>
