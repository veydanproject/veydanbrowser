<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { NoteAttachment } from '$lib/types';
  import { api, downloadNoteAttachment } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    noteId: string;
    attachments: NoteAttachment[];
    readonly?: boolean;
    oninsert: (a: NoteAttachment) => void;
    onchanged: () => void;
    onpick: (files: File[]) => void;
  }

  let { noteId, attachments, readonly = false, oninsert, onchanged, onpick }: Props = $props();

  let fileInput: HTMLInputElement | null = $state(null);
  let gcResult = $state<string | null>(null);
  let gcBusy = $state(false);

  function fmtSize(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }

  async function remove(a: NoteAttachment) {
    await api.notes.attachmentDelete(noteId, a.name);
    onchanged();
  }

  /** Remove attachments no note references (across all notes). */
  async function gc() {
    gcBusy = true;
    try {
      const removed = await api.notes.attachmentsGc(true);
      gcResult = $t('note_att_gc_done', { n: String(removed.length) });
      onchanged();
    } finally {
      gcBusy = false;
    }
  }

  function onFiles(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    if (input.files?.length) onpick(Array.from(input.files));
    input.value = '';
  }
</script>

<div class="att-panel">
  <div class="att-header">
    <span class="att-title"><Icon name="paperclip" size={12} /> {$t('note_att_title')} ({attachments.length})</span>
    <div class="att-actions">
      {#if !readonly}
        <button class="att-btn" title={$t('note_att_add')} onclick={() => fileInput?.click()}>
          <Icon name="plus" size={12} />
        </button>
        <input bind:this={fileInput} type="file" multiple hidden onchange={onFiles} />
      {/if}
      <button class="att-btn" title={$t('note_att_gc')} onclick={gc} disabled={gcBusy}>
        <Icon name="trash-2" size={12} />
      </button>
    </div>
  </div>
  {#if gcResult}
    <p class="att-hint">{gcResult}</p>
  {/if}
  {#if attachments.length === 0}
    <p class="att-hint">{$t('note_att_empty')}</p>
  {:else}
    <ul class="att-list">
      {#each attachments as a (a.name)}
        <li class="att-item">
          <Icon name={a.is_image ? 'image' : 'file-text'} size={13} />
          <button class="att-name" title={a.name} onclick={() => api.notes.attachmentOpen(noteId, a.name)}>{a.name}</button>
          <span class="att-size">{fmtSize(a.size)}</span>
          <button class="att-btn" title={$t('note_att_download')} onclick={() => downloadNoteAttachment(noteId, a.name)}>
            <Icon name="download" size={12} />
          </button>
          {#if !readonly}
            <button class="att-btn" title={$t('note_att_insert')} onclick={() => oninsert(a)}><Icon name="link" size={12} /></button>
            <button class="att-btn att-danger" title={$t('note_att_delete')} onclick={() => remove(a)}><Icon name="x" size={12} /></button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .att-panel {
    border-top: 1px solid var(--border);
    background: var(--surface);
    padding: var(--sp-2) var(--sp-3);
    max-height: 180px;
    overflow-y: auto;
    flex-shrink: 0;
  }
  .att-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-1);
  }
  .att-title {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: var(--fs-2xs);
    font-weight: var(--fw-semibold);
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .att-actions { display: flex; gap: 0.2rem; }
  .att-btn {
    background: none;
    border: none;
    color: var(--text-3);
    cursor: pointer;
    border-radius: 4px;
    padding: 0.15rem 0.3rem;
    display: inline-flex;
    align-items: center;
  }
  .att-btn:hover:not(:disabled) { background: var(--surface-2); color: var(--text); }
  .att-btn.att-danger:hover { color: var(--danger); }
  .att-btn:disabled { opacity: 0.4; }
  .att-hint { margin: 0.2rem 0; font-size: var(--fs-xs); color: var(--text-3); }
  .att-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.15rem; }
  .att-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: var(--fs-xs);
    color: var(--text-2);
    padding: 0.15rem 0.2rem;
    border-radius: 4px;
  }
  .att-item:hover { background: var(--surface-2); }
  .att-name {
    flex: 1;
    min-width: 0;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    font-size: var(--fs-xs);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    padding: 0;
  }
  .att-name:hover { text-decoration: underline; }
  .att-size { font-family: var(--font-mono); font-size: var(--fs-2xs); color: var(--text-3); }
</style>
