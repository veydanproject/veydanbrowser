<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api } from '$lib/api';
  import type { NoteTag, NoteFolder, Note } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import NoteTagsInput from './NoteTagsInput.svelte';
  import NoteHistoryPanel from './NoteHistoryPanel.svelte';
  import NoteHistoryMerge from './NoteHistoryMerge.svelte';
  import { t, locale } from '$lib/i18n';
  import { relTime } from '$lib/utils';
  import { tick } from 'svelte';

  interface Props {
    allTags: NoteTag[];
    folders: NoteFolder[];
  }

  let { allTags, folders }: Props = $props();

  const note = $derived(notesStore.activeNote);
  const saveStatus = $derived(notesStore.saveStatus);
  const externalChange = $derived(notesStore.externalChange);

  let titleValue = $state('');
  let contentValue = $state('');
  let titleInputEl: HTMLInputElement | null = $state(null);

  let _lastNoteId: string | null = null;
  $effect(() => {
    if (note) {
      if (note.id !== _lastNoteId) {
        _lastNoteId = note.id;
      }
      titleValue = note.title;
      contentValue = note.content ?? '';
    }
  });

  let textareaEl: HTMLTextAreaElement | null = $state(null);
  let titleHovered = $state(false);
  let showDeleteConfirm = $state(false);
  let showHistory = $state(false);
  let mergeHistoryId = $state<string | null>(null);

  // Resizable history panel
  function loadHistWidth(): number {
    try {
      const v = localStorage.getItem('notes-history-width');
      if (v) return parseFloat(v);
    } catch {}
    return 31;
  }

  let editorWrapEl: HTMLElement | null = $state(null);
  let histWidthPct = $state(loadHistWidth());
  let histDragging = $state(false);
  let histHovered = $state(false);

  function startHistDrag(e: MouseEvent) {
    e.preventDefault();
    histDragging = true;

    function onMove(ev: MouseEvent) {
      if (!editorWrapEl) return;
      const rect = editorWrapEl.getBoundingClientRect();
      const histPct = ((rect.right - ev.clientX) / rect.width) * 100;
      histWidthPct = Math.max(20, Math.min(50, histPct));
      try { localStorage.setItem('notes-history-width', String(histWidthPct)); } catch {}
    }

    function onUp() {
      histDragging = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  $effect(() => {
    const query = notesStore.searchQuery;
    const content = contentValue;
    if (!textareaEl || !query || !content) return;
    const idx = content.toLowerCase().indexOf(query.toLowerCase());
    if (idx < 0) return;
    tick().then(() => {
      if (!textareaEl) return;
      textareaEl.setSelectionRange(idx, idx + query.length);
      const linesBefore = content.substring(0, idx).split('\n').length - 1;
      const lineHeight = parseFloat(getComputedStyle(textareaEl).lineHeight) || 22;
      textareaEl.scrollTop = Math.max(0, linesBefore * lineHeight - textareaEl.clientHeight / 3);
      textareaEl.focus();
    });
  });

  const contextChips = $derived.by(() => {
    if (!note) return [];
    const chips: { label: string; color: string; onremove?: () => void }[] = [];

    // Folders first — they represent the original context
    const folderIds = notesStore.list.find(n => n.id === note.id)?.folder_ids ?? [];
    for (const fid of folderIds) {
      const folder = folders.find(f => f.id === fid);
      if (folder) {
        chips.push({
          label: folder.name,
          color: folder.color,
          onremove: () => notesStore.removeNoteFolder(note!.id, fid),
        });
      }
    }

    for (const b of note.bindings) {
      if (b.startsWith('workspace:')) {
        const ws = workspacesStore.list.find(w => w.id === b.slice('workspace:'.length));
        if (ws) chips.push({ label: ws.name, color: ws.color, onremove: () => notesStore.removeNoteBinding(note!.id, b) });
      } else if (b.startsWith('profile:')) {
        const pr = profilesStore.list.find(p => p.id === b.slice('profile:'.length));
        if (pr) chips.push({ label: pr.name, color: 'var(--accent)', onremove: () => notesStore.removeNoteBinding(note!.id, b) });
      }
    }

    return chips;
  });

  const formatUpdatedAt = (iso: string): string => relTime(iso, $locale);

  async function confirmDelete() {
    if (!note) return;
    await notesStore.deleteNote(note.id);
    showDeleteConfirm = false;
  }

  function onHistoryRestore(restoredNote: Note) {
    if (note && restoredNote.id === note.id) {
      titleValue = restoredNote.title;
      contentValue = restoredNote.content ?? '';
      notesStore.activeNote = restoredNote;
    }
    showHistory = false;
  }

  function onHistoryMerge(historyId: string) {
    mergeHistoryId = historyId;
  }

  function onMergeResolved(mergedContent: string) {
    if (!note) return;
    contentValue = mergedContent;
    notesStore.onContentChange(mergedContent);
    mergeHistoryId = null;
    showHistory = false;
  }

  function onTitleChange() {
    notesStore.onTitleChange(titleValue);
  }

  function onBodyChange() {
    notesStore.onContentChange(contentValue);
  }

  const saveLabel: Record<string, string> = {
    saved: $t('note_status_saved'),
    saving: $t('note_status_saving'),
    unsaved: $t('note_status_unsaved'),
    failed: $t('note_status_error'),
    external: $t('note_status_external'),
  };

  const saveIcon: Record<string, string> = {
    saved: 'check-circle',
    saving: 'loader',
    unsaved: 'circle',
    failed: 'alert-circle',
    external: 'alert-triangle',
  };
</script>

{#if !note}
  <div class="empty-state editor-empty">
    <Icon name="file-text" size={28} />
    <p>{$t('note_empty_hint')}</p>
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="editor-wrap" bind:this={editorWrapEl} class:is-dragging={histDragging}>
  <div class="editor">
    <!-- Recovery banner -->
    {#if note.has_draft}
      <div class="banner banner-warn">
        <span>{$t('note_draft_found')}</span>
        <div class="banner-actions">
          <button onclick={() => notesStore.recoverDraft(note!.id)}>{$t('note_draft_recover')}</button>
          <button onclick={() => notesStore.discardDraft(note!.id)}>{$t('note_draft_discard')}</button>
        </div>
      </div>
    {/if}

    <!-- External change banner -->
    {#if externalChange}
      <div class="banner banner-info">
        <span>{$t('note_external_change')}</span>
        <div class="banner-actions">
          <button onclick={() => notesStore.acceptExternalChange()}>{$t('note_external_accept')}</button>
          <button onclick={() => notesStore.discardExternalChange()}>{$t('note_external_keep')}</button>
        </div>
      </div>
    {/if}

    <div class="editor-header">
      <div class="title-wrap"
        onmouseenter={() => titleHovered = true}
        onmouseleave={() => titleHovered = false}
      >
        <input
          bind:this={titleInputEl}
          bind:value={titleValue}
          type="text"
          class="title-input"
          placeholder={$t('notes_title_placeholder')}
          maxlength="100"
          oninput={onTitleChange}
        />
        {#if titleHovered}
          <span class="title-hint"><Icon name="pencil" size={12} /></span>
        {/if}
      </div>
      <NoteTagsInput
        selectedTags={note.tags}
        {allTags}
        onchange={(tagNames) => notesStore.setTags(note!.id, tagNames)}
        {contextChips}
        {folders}
        activeFolderIds={notesStore.list.find(n => n.id === note.id)?.folder_ids ?? []}
        onaddFolder={(folderId) => notesStore.addNoteFolder(note!.id, folderId)}
        workspaces={workspacesStore.list}
        profiles={profilesStore.list}
        activeBindings={note.bindings}
        onaddBinding={(binding) => notesStore.addNoteBinding(note!.id, binding)}
      />
      <div class="header-right">
        <span class="updated-at">{formatUpdatedAt(note.updated_at)}</span>
        <span
          class="save-status-icon status-{saveStatus}"
          title={saveLabel[saveStatus] ?? $t('note_status_saved')}
          class:spinning={saveStatus === 'saving'}
        >
          <Icon name={saveIcon[saveStatus] ?? 'check-circle'} size={14} />
        </span>
        <button
          class="icon-action"
          class:active={showHistory}
          onclick={() => { showHistory = !showHistory; mergeHistoryId = null; }}
          title="История версий"
        >
          <Icon name="clock" size={13} />
        </button>
      </div>
    </div>

    <div class="note-content">
      <textarea
        bind:this={textareaEl}
        bind:value={contentValue}
        class="editor-body"
        placeholder={$t('note_content_placeholder')}
        oninput={onBodyChange}
        spellcheck="false"
      ></textarea>
    </div>

    <div class="editor-footer">
      <span class="footer-meta">
        {note.format.toUpperCase()}
        {#if note.bindings.length > 0}
          · {note.bindings.some(b => b.startsWith('profile:')) ? 'profile' : 'workspace'}
        {/if}
      </span>
      <div class="footer-actions">
        <button
          class="icon-action"
          onclick={() => note && notesStore.togglePin(note.id)}
          title={note.pinned ? $t('note_btn_unpin') : $t('note_btn_pin')}
          class:active={note.pinned}
        >
          <Icon name="pin" size={13} />
        </button>
        <button
          class="icon-action"
          onclick={() => note && notesStore.archiveNote(note.id)}
          title={$t('note_btn_archive')}
        >
          <Icon name="archive" size={13} />
        </button>
        <button
          class="icon-action"
          onclick={() => note && api.notes.openExternal(note.id)}
          title={$t('note_btn_open_external')}
        >
          <Icon name="external-link" size={13} />
        </button>
        <button
          class="icon-action icon-danger"
          onclick={() => (showDeleteConfirm = true)}
          title={$t('note_btn_delete')}
        >
          <Icon name="trash-2" size={13} />
        </button>
      </div>
    </div>

    {#if showDeleteConfirm}
      <div class="delete-overlay">
        <div class="delete-modal">
          <p class="delete-title">{$t('note_delete_title')}</p>
          <p class="delete-warn">
            {$t('note_delete_body1')}
            {$t('note_delete_body2')}
          </p>
          <div class="delete-actions">
            <button class="btn btn-ghost btn-sm" onclick={() => (showDeleteConfirm = false)}>{$t('notes_btn_cancel')}</button>
            <button class="btn btn-danger btn-sm" onclick={confirmDelete}>{$t('note_delete_confirm')}</button>
          </div>
        </div>
      </div>
    {/if}

    {#if mergeHistoryId}
      <NoteHistoryMerge
        noteId={note.id}
        historyId={mergeHistoryId}
        onresolved={onMergeResolved}
        oncancel={() => (mergeHistoryId = null)}
      />
    {/if}
  </div>

  {#if showHistory}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="col-resizer"
      class:active={histHovered || histDragging}
      onmousedown={startHistDrag}
      onmouseenter={() => (histHovered = true)}
      onmouseleave={() => (histHovered = false)}
    ></div>
    <div class="history-wrap" style="width: {histWidthPct}%">
      <NoteHistoryPanel
        noteId={note.id}
        onrestore={onHistoryRestore}
        onmerge={onHistoryMerge}
        onclose={() => (showHistory = false)}
      />
    </div>
  {/if}
  </div>
{/if}

<style>
  /* .empty-state covers the centering; keep only the delta needed to fill the editor pane */
  .editor-empty {
    flex: 1;
    font-size: var(--fs-sm);
  }

  .editor-wrap {
    flex: 1;
    display: flex;
    overflow: hidden;
    position: relative;
  }

  .editor-wrap.is-dragging {
    user-select: none;
    cursor: col-resize;
  }

  .col-resizer {
    width: 8px;
    flex-shrink: 0;
    cursor: col-resize;
    position: relative;
    background: transparent;
  }

  .col-resizer::before {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    transform: translateX(-50%);
    background: var(--border);
    transition: background 0.15s, width 0.1s;
  }

  .col-resizer.active::before {
    background: var(--accent);
    width: 2px;
  }

  .history-wrap {
    flex-shrink: 0;
    display: flex;
    overflow: hidden;
    min-width: 180px;
  }

  .editor {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
    min-width: 0;
  }

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    font-size: var(--fs-sm);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .banner-warn { background: var(--warn-bg); color: var(--warn-text); }
  .banner-info { background: var(--accent-bg); color: var(--accent); }

  .banner-actions {
    display: flex;
    gap: 0.4rem;
    flex-shrink: 0;
  }

  .banner-actions button {
    background: none;
    border: 1px solid currentColor;
    border-radius: var(--radius-sm);
    padding: 0.15rem var(--sp-2);
    font-size: var(--fs-xs);
    color: inherit;
    cursor: pointer;
    transition: background 0.15s;
  }

  .banner-actions button:hover { background: rgba(255,255,255,0.1); }

  .editor-header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: 40px;
    padding: 0 var(--sp-4);
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }

  .title-wrap {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    width: 180px;
    flex-shrink: 0;
  }

  .title-hint {
    color: var(--accent);
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .title-input {
    background: none;
    border: none;
    outline: none;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text);
    padding: 0;
    min-width: 0;
    flex: 1;
  }

  .title-input::placeholder { color: var(--text-3); }

  .header-right {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
    margin-left: auto;
  }

  .updated-at {
    font-size: var(--fs-2xs);
    color: var(--text-3);
    white-space: nowrap;
  }

  .save-status-icon {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    line-height: 1;
  }

  .save-status-icon.status-saved    { color: var(--success-text); }
  .save-status-icon.status-saving   { color: var(--accent); }
  .save-status-icon.status-unsaved  { color: var(--warn-text); }
  .save-status-icon.status-failed   { color: var(--danger-text); }
  .save-status-icon.status-external { color: var(--warn-text); }

  .spinning :global(svg) {
    animation: spin 1s linear infinite;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.1rem;
    padding: 0.3rem var(--sp-3);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
    flex-shrink: 0;
    flex-wrap: wrap;
  }
  .tb-btn {
    background: none;
    border: none;
    border-radius: 4px;
    padding: 0.2rem 0.45rem;
    cursor: pointer;
    color: var(--text-2);
    font-size: var(--fs-sm);
    line-height: 1;
    transition: background 0.12s, color 0.12s;
    min-width: 1.8rem;
    text-align: center;
  }
  .tb-btn:hover { background: var(--bg-3); color: var(--text-1); }
  .tb-sep {
    width: 1px;
    height: 1.1rem;
    background: var(--border);
    margin: 0 0.2rem;
    flex-shrink: 0;
  }
  .tb-spacer { flex: 1; }
  .tb-mode-toggle {
    font-size: var(--fs-2xs);
    padding: 0.2rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-2);
  }
  .tb-mode-toggle.active {
    border-color: var(--accent);
    color: var(--accent);
  }
  .tb-mode-toggle:hover { background: var(--bg-3); color: var(--text-1); }

  .note-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    margin: var(--sp-2) var(--sp-3) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .editor-body {
    flex: 1;
    padding: var(--sp-2) var(--sp-4);
    background: none;
    border: none;
    outline: none;
    resize: none;
    font-size: var(--fs-base);
    line-height: 1.6;
    color: var(--text);
    font-family: 'Menlo', 'Consolas', 'SF Mono', monospace;
    overflow-y: auto;
  }

  .editor-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem var(--sp-3);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .footer-meta {
    font-size: var(--fs-2xs);
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-family: monospace;
  }

  .footer-actions {
    display: flex;
    gap: 0.1rem;
  }

  .icon-action {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-2);
    padding: var(--sp-1);
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    transition: color 0.15s, background 0.15s;
  }

  .icon-action:hover { color: var(--text); background: var(--surface); }
  .icon-action.active { color: var(--accent); }
  .icon-danger:hover { color: #ef4444 !important; }

  .delete-overlay {
    position: absolute;
    inset: 0;
    background: rgba(0,0,0,0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
    border-radius: inherit;
  }
  .delete-modal {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: var(--sp-5) var(--sp-6);
    max-width: 320px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .delete-title {
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--text-1);
  }
  .delete-warn {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-2);
    line-height: 1.5;
  }
  .delete-actions {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
    margin-top: var(--sp-1);
  }

</style>
