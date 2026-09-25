<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Hides the whole UI behind the PIN/password while the lock is engaged. -->
<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';
  import { lockInputAttrs } from '$lib/lock';
  import LockRecoverFlow from '$lib/components/lock/LockRecoverFlow.svelte';

  interface Props {
    children: Snippet;
  }

  let { children }: Props = $props();

  let password = $state('');
  let error = $state(false);
  let busy = $state(false);
  let recovering = $state(false);
  let input = $state<HTMLInputElement | null>(null);

  const kind = $derived(notesLock.status.kind);
  const attrs = $derived(lockInputAttrs(kind));
  const hint = $derived(notesLock.status.hint);

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
  });

  $effect(() => {
    if (notesLock.locked) {
      password = '';
      error = false;
      recovering = false;
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
    {#if recovering}
      <div class="lock-card wide">
        <LockRecoverFlow onback={() => (recovering = false)} ondone={() => (recovering = false)} />
      </div>
    {:else}
      <form class="lock-card" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
        <Icon name="lock" size={28} />
        <h3>{kind === 'pin' ? $t('lock_title_pin') : $t('lock_title_password')}</h3>
        <input
          bind:this={input}
          class:pin={kind === 'pin'}
          class:invalid={error}
          type={kind === 'pin' ? 'text' : 'password'}
          inputmode={attrs.inputmode}
          pattern={attrs.pattern}
          bind:value={password}
          placeholder={kind === 'pin' ? $t('lock_kind_pin') : $t('lock_kind_password')}
          autocomplete="off"
          autocapitalize="off"
          spellcheck="false"
        />
        {#if error}
          <span class="err">{$t('notes_lock_wrong')}</span>
          {#if hint}<span class="hint">{$t('lock_hint_prefix', { hint })}</span>{/if}
        {/if}
        <button class="btn btn-primary" type="submit" disabled={!password || busy}>{$t('notes_lock_unlock')}</button>
        <button class="forgot" type="button" onclick={() => (recovering = true)}>{$t('lock_forgot')}</button>
      </form>
    {/if}
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
    min-height: 0;
    padding: var(--sp-4);
    padding-top: max(var(--sp-4), var(--sat, 0px));
    overflow-y: auto;
    background: var(--bg);
  }
  .lock-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    width: 300px;
    max-width: 100%;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-1);
    color: var(--text-2);
  }
  .lock-card.wide { width: 380px; align-items: stretch; }
  .lock-card h3 { margin: 0; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .lock-card input {
    width: 100%;
    min-height: 40px;
    padding: 0 10px;
    font: inherit;
    text-align: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
  }
  .lock-card input.invalid { border-color: var(--danger); }
  .lock-card input.pin { -webkit-text-security: disc; }
  .lock-card .btn { width: 100%; justify-content: center; }
  .err { font-size: var(--fs-xs); color: var(--danger); }
  .hint { font-size: var(--fs-sm); color: var(--text); text-align: center; }
  .forgot {
    border: none;
    background: transparent;
    color: var(--text-3);
    font: inherit;
    font-size: var(--fs-xs);
    cursor: pointer;
    text-decoration: underline;
  }
  .forgot:hover { color: var(--text); }
</style>
