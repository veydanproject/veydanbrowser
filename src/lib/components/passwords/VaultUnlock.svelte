<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import BottomSheet from '$lib/components/mobile/BottomSheet.svelte';
  import { t } from '$lib/i18n';
  import { isMobile } from '$lib/platform';
  import { notesLock } from '$lib/store/notes-lock.svelte';

  interface Props {
    open: boolean;
    /** Runs after the vault password is accepted. */
    onunlocked: () => void;
  }

  let { open = $bindable(false), onunlocked }: Props = $props();

  let password = $state('');
  let wrong = $state(false);
  let busy = $state(false);

  $effect(() => {
    if (!open) return;
    password = '';
    wrong = false;
  });

  function close() {
    open = false;
  }

  async function submit() {
    if (!password || busy) return;
    busy = true;
    wrong = false;
    try {
      await notesLock.unlock(password);
      close();
      onunlocked();
    } catch {
      wrong = true;
      password = '';
    } finally {
      busy = false;
    }
  }
</script>

{#snippet form()}
  <form class="form" onsubmit={(e) => { e.preventDefault(); void submit(); }}>
    <input
      class:pin={notesLock.status.kind === 'pin'}
      type={notesLock.status.kind === 'pin' ? 'text' : 'password'}
      inputmode={notesLock.status.kind === 'pin' ? 'numeric' : 'text'}
      pattern={notesLock.status.kind === 'pin' ? '[0-9]*' : undefined}
      bind:value={password}
      placeholder={$t('notes_lock_password')}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
    />
    {#if wrong}<p class="err">{$t('notes_lock_wrong')}</p>{/if}
    <button class="btn btn-primary" type="submit" disabled={!password || busy}>{$t('notes_lock_unlock')}</button>
  </form>
{/snippet}

{#if isMobile}
  <BottomSheet {open} title={$t('notes_lock_title')} onclose={close}>
    {@render form()}
  </BottomSheet>
{:else}
  <Dialog bind:open title={$t('notes_lock_title')} onclose={close} width="360px">
    {@render form()}
  </Dialog>
{/if}

<style>
  .form { display: flex; flex-direction: column; gap: var(--sp-3); }
  input {
    width: 100%;
    min-height: 40px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .err { margin: 0; color: var(--danger-text); font-size: var(--fs-xs); }
  input.pin { -webkit-text-security: disc; }
</style>
