<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import FilePanel from '$lib/components/files/FilePanel.svelte';
  import TransferStrip from '$lib/components/files/TransferStrip.svelte';
  import ConflictDialog from '$lib/components/files/ConflictDialog.svelte';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import { filesStore } from '$lib/store/sftp.svelte';
  import { sshStore } from '$lib/store/ssh.svelte';
  import { t } from '$lib/i18n';

  let promptAnswer = $state('');

  onMount(() => {
    sshStore.ensureLoaded();
    filesStore.ensureLoaded();
  });

  function onKeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement | null;
    // Don't hijack keys inside inputs (path editing, dialogs)
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) return;

    if (e.key === 'Tab') {
      e.preventDefault();
      filesStore.activePanel = filesStore.activePanel === 0 ? 1 : 0;
    } else if (e.key === 'F5') {
      e.preventDefault();
      filesStore.copyToOtherPanel(filesStore.activePanel);
    } else if (e.key === 'a' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      filesStore.selectAll(filesStore.activePanel);
    }
  }

  function submitPrompt() {
    filesStore.respondPrompt(promptAnswer);
    promptAnswer = '';
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="page page--fill files-page">
  <div class="page-header">
    <div class="page-title-group">
      <h1>{$t('files_title')}</h1>
      <p class="page-sub">{$t('files_sub')}</p>
    </div>
  </div>

  <div class="panels">
    <FilePanel index={0} />
    <FilePanel index={1} />
  </div>

  <TransferStrip />
</div>

<ConflictDialog />

<!-- Keyboard-interactive (2FA) prompt during SFTP connect -->
<Dialog
  open={filesStore.prompt !== null}
  title={filesStore.prompt?.name || $t('files_prompt_title')}
  onclose={() => filesStore.cancelPrompt()}
>
  {#if filesStore.prompt}
    {#if filesStore.prompt.instructions}
      <p class="prompt-instructions">{filesStore.prompt.instructions}</p>
    {/if}
    <div class="form-group">
      <label for="sftp-prompt-input">{filesStore.prompt.prompts[0]?.prompt ?? ''}</label>
      <input
        id="sftp-prompt-input"
        type={filesStore.prompt.prompts[0]?.echo ? 'text' : 'password'}
        bind:value={promptAnswer}
        onkeydown={(e) => e.key === 'Enter' && submitPrompt()}
      />
    </div>
  {/if}
  {#snippet footer()}
    <button class="btn btn-ghost" onclick={() => filesStore.cancelPrompt()}>{$t('cancel')}</button>
    <button class="btn btn-primary" onclick={submitPrompt}>{$t('files_prompt_send')}</button>
  {/snippet}
</Dialog>

<style>
  .files-page {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .panels {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-3);
  }

  .files-page :global(.transfer-strip) {
    margin-top: var(--sp-3);
  }

  .prompt-instructions {
    margin-bottom: var(--sp-3);
    color: var(--text-2);
    font-size: var(--fs-sm);
    white-space: pre-wrap;
  }
</style>
