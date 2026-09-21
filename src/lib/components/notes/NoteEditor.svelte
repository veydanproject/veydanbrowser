<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { notesStore } from '$lib/store/notes.svelte';
  import { syncStore } from '$lib/store/sync.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api } from '$lib/api';
  import type { NoteTag, NoteFolder, Note, NoteAttachment } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import NoteTagsInput from './NoteTagsInput.svelte';
  import NoteHistoryPanel from './NoteHistoryPanel.svelte';
  import NoteHistoryMerge from './NoteHistoryMerge.svelte';
  import NoteToolbar, { type EditorMode } from './NoteToolbar.svelte';
  import NoteRichEditor from './NoteRichEditor.svelte';
  import NoteFindBar from './NoteFindBar.svelte';
  import NoteAttachments from './NoteAttachments.svelte';
  import NoteLinks from './NoteLinks.svelte';
  import WikiLinkPicker from './WikiLinkPicker.svelte';
  import { WIKI_MARK, unclosedWikiAt, wikiMarkup } from '$lib/tiptap-ext';
  import { applyAction, shiftIndent, continueList, type EditAction, type EditResult } from '$lib/markdown-edit';
  import { wordCount } from '$lib/markdown';
  import { pasteHasHiddenFiles } from '$lib/notes-files';
  import { t, locale } from '$lib/i18n';
  import { relTime, formatError } from '$lib/utils';
  import { onMount, tick, untrack } from 'svelte';

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
  let showSyncConflict = $state(false);
  const syncConflict = $derived(!!note && (syncStore.status?.conflicts ?? []).some((c) => c.note_id === note.id));

  // ── Editing modes: `rich` (WYSIWYG, default) and `source` (raw Markdown) ─────
  const MODE_KEY = 'notes-editor-mode';
  function loadMode(): EditorMode {
    try {
      // 'edit' is the pre-WYSIWYG name of the source mode
      const v = localStorage.getItem(MODE_KEY);
      if (v === 'source' || v === 'edit') return 'source';
    } catch {}
    return 'rich';
  }
  let mode = $state<EditorMode>(loadMode());
  let findOpen = $state(false);
  let findBar: NoteFindBar | null = $state(null);
  let richEditor: NoteRichEditor | null = $state(null);

  function setMode(m: EditorMode) {
    mode = m;
    try { localStorage.setItem(MODE_KEY, m); } catch {}
  }

  const readonly = $derived(note?.deleted ?? false);
  const stats = $derived({ words: wordCount(contentValue), chars: contentValue.length });

  function setContent(content: string) {
    contentValue = content;
    notesStore.onContentChange(content);
  }

  /** Apply a text transformation to the source textarea, restore selection, and schedule autosave. */
  function applyEdit(r: EditResult) {
    setContent(r.text);
    tick().then(() => {
      if (!textareaEl) return;
      textareaEl.focus();
      textareaEl.setSelectionRange(r.selStart, r.selEnd);
    });
  }

  function runAction(action: EditAction) {
    if (readonly) return;
    if (mode === 'rich') { richEditor?.runAction(action); return; }
    const start = textareaEl?.selectionStart ?? contentValue.length;
    const end = textareaEl?.selectionEnd ?? contentValue.length;
    applyEdit(applyAction(contentValue, start, end, action));
  }

  /** Find/replace works on the source textarea, so it switches the mode. */
  function openFind() {
    setMode('source');
    findOpen = true;
    tick().then(() => findBar?.focus());
  }

  /** Ctrl+K / Ctrl+F / Ctrl+S in the rich editor (bold/italic are built in). */
  function onRichHotkey(e: KeyboardEvent): boolean {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod || e.shiftKey || e.altKey) return false;
    const key = e.key.toLowerCase();
    if (key === 'k') { runAction('link'); return true; }
    if (key === 'f') { openFind(); return true; }
    if (key === 's') { void notesStore.save(); return true; }
    return false;
  }

  function onEditorKeydown(e: KeyboardEvent) {
    const ta = e.currentTarget as HTMLTextAreaElement;
    const { selectionStart: s, selectionEnd: en } = ta;
    const mod = e.ctrlKey || e.metaKey;

    if (wikiOpen) {
      if (e.key === 'Escape') { wikiOpen = false; return; }
      if (wikiPicker?.handleKeydown(e)) return;
    }
    if (mod && !e.shiftKey && !e.altKey) {
      const key = e.key.toLowerCase();
      const hotkeys: Record<string, EditAction> = { b: 'bold', i: 'italic', k: 'link' };
      if (hotkeys[key]) { e.preventDefault(); runAction(hotkeys[key]); return; }
      if (key === 'f') { e.preventDefault(); openFind(); return; }
      if (key === 's') { e.preventDefault(); void notesStore.save(); return; }
    }
    if (readonly) return;
    // Shift+Tab may arrive with a different `key` on WebKitGTK; `code` is stable
    if (e.key === 'Tab' || e.code === 'Tab') {
      e.preventDefault();
      applyEdit(shiftIndent(contentValue, s, en, e.shiftKey));
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !mod && s === en) {
      const r = continueList(contentValue, s);
      if (r) { e.preventDefault(); applyEdit(r); }
    }
  }

  // ── Wiki links: `@@` autocomplete in source mode, navigation, backlinks panel ─
  let wikiOpen = $state(false);
  let wikiQuery = $state('');
  let wikiIndex = $state(0);
  let wikiStart = 0;
  let wikiPicker: WikiLinkPicker | null = $state(null);
  let linksOpen = $state(false);
  let linksVersion = $state(0);

  /** Track an unclosed `@@` immediately before the caret. */
  function updateWikiState() {
    const pos = textareaEl?.selectionStart ?? contentValue.length;
    const before = contentValue.slice(0, pos);
    const open = unclosedWikiAt(before);
    if (open < 0) {
      wikiOpen = false;
      return;
    }
    wikiStart = open;
    wikiQuery = before.slice(open + WIKI_MARK.length);
    wikiIndex = 0;
    wikiOpen = true;
  }

  /** Replace the partial `@@query` with a completed link. */
  function pickWikiLink(title: string) {
    const pos = textareaEl?.selectionStart ?? contentValue.length;
    const link = wikiMarkup(title);
    const rest = contentValue.slice(pos);
    const skip = rest.startsWith(WIKI_MARK) ? WIKI_MARK.length : 0;
    const text = contentValue.slice(0, wikiStart) + link + rest.slice(skip);
    wikiOpen = false;
    const caret = wikiStart + link.length;
    applyEdit({ text, selStart: caret, selEnd: caret });
  }

  /** Open a linked note by id or title; create it when nothing matches. */
  async function openWikiLink(target: string) {
    const key = target.trim().toLowerCase();
    const find = () =>
      notesStore.list.find((n) => n.id === target) ?? notesStore.list.find((n) => n.title.toLowerCase() === key);
    let hit = find();
    if (!hit) {
      // The list may lag behind a rename; reload before deciding to create
      await notesStore.refresh();
      hit = find();
    }
    if (hit) { await notesStore.openNote(hit.id); return; }
    if (readonly || !target.trim()) return;
    const created = await notesStore.createNote({ title: target.trim(), bindings: note?.bindings ?? [] });
    await notesStore.openNote(created.id);
  }

  $effect(() => {
    if (saveStatus !== 'saved') return;
    // untrack: reading linksVersion here would make the effect depend on itself
    untrack(() => linksVersion++);
  });

  // ── Attachments: paste, OS drag-and-drop, panel ──────────────────────────────
  let attachments = $state<NoteAttachment[]>([]);
  let attachmentsOpen = $state(false);

  async function loadAttachments() {
    attachments = note ? await api.notes.attachmentList(note.id) : [];
  }

  $effect(() => {
    if (note?.id) void loadAttachments();
  });

  /** Insert a Markdown link to the attachment at the cursor. */
  function insertAttachmentLink(a: NoteAttachment) {
    const rel = a.rel_path.split('/').map(encodeURIComponent).join('/');
    const link = `${a.is_image ? '!' : ''}[${a.name}](${rel})`;
    if (mode === 'rich') { richEditor?.insertMarkdown(link); return; }
    const pos = textareaEl?.selectionStart ?? contentValue.length;
    const before = contentValue.slice(0, pos);
    const pad = before.length && !before.endsWith('\n') ? '\n' : '';
    const snippet = `${pad}${link}\n`;
    applyEdit({ text: before + snippet + contentValue.slice(pos), selStart: pos + snippet.length, selEnd: pos + snippet.length });
  }

  async function uploadFiles(files: File[]) {
    if (!note || readonly) return;
    for (const f of files) {
      const bytes = new Uint8Array(await f.arrayBuffer());
      const name = f.name || `pasted-${Date.now()}.${f.type.split('/')[1] ?? 'bin'}`;
      insertAttachmentLink(await api.notes.attachmentAdd(note.id, name, bytes));
    }
    await loadAttachments();
  }

  /** Copy local files (paste/drop of file references) into the note's attachments. */
  async function uploadPaths(paths: string[]) {
    if (!note || readonly) return;
    for (const p of paths) {
      try {
        insertAttachmentLink(await api.notes.attachmentAddFromPath(note.id, p));
      } catch (e) {
        console.error('attachment copy failed', p, e);
      }
    }
    await loadAttachments();
  }

  /** Files copied in the OS file manager: the webview hides their paths, so read them via Rust. */
  async function pasteClipboardFiles() {
    const paths = await api.notes.clipboardFilePaths();
    if (paths.length) await uploadPaths(paths);
  }

  // Source-mode paste: image bytes upload; hidden file references copied via the OS clipboard
  function onPaste(e: ClipboardEvent) {
    const files = Array.from(e.clipboardData?.files ?? []);
    if (files.length) { e.preventDefault(); void uploadFiles(files); return; }
    if (pasteHasHiddenFiles(e.clipboardData)) { e.preventDefault(); void pasteClipboardFiles(); }
  }

  // OS file drops arrive via Tauri's native drag-drop (dragDropEnabled), not the webview.
  let panesEl: HTMLElement | null = $state(null);

  function pointInEl(el: HTMLElement | null, x: number, y: number): boolean {
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  // Tauri reports the drop position in physical pixels, except on Linux (GTK logical pixels).
  const dropScale = () => (navigator.userAgent.includes('Linux') ? 1 : window.devicePixelRatio || 1);

  async function onOsDrop(paths: string[], posX: number, posY: number) {
    if (!note || readonly || paths.length === 0) return;
    const x = posX / dropScale();
    const y = posY / dropScale();
    if (!pointInEl(panesEl, x, y)) return;
    if (mode === 'rich') richEditor?.caretAtCoords(x, y);
    await uploadPaths(paths);
  }

  onMount(() => {
    if (!('__TAURI_INTERNALS__' in window)) return;
    let unlisten: (() => void) | null = null;
    let unlistenSync: (() => void) | null = null;
    let unlistenStatus: (() => void) | null = null;
    let disposed = false;
    // Sync status drives the conflict banner
    void syncStore.listen().then((un) => { if (disposed) un(); else unlistenStatus = un; });
    void import('@tauri-apps/api/webview').then(({ getCurrentWebview }) =>
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type !== 'drop') return;
        void onOsDrop(event.payload.paths, event.payload.position.x, event.payload.position.y);
      }),
    ).then((un) => { if (disposed) un(); else unlisten = un; });
    // Attachments pulled by vault sync for the open note
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<string>('notes://attachments-changed', (event) => {
        if (event.payload === note?.id) void loadAttachments();
      }),
    ).then((un) => { if (disposed) un(); else unlistenSync = un; });
    return () => { disposed = true; unlisten?.(); unlistenSync?.(); unlistenStatus?.(); };
  });

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

  // Highlight the list search query inside the opened note
  $effect(() => {
    const query = notesStore.searchQuery;
    const content = contentValue;
    if (!query || !content) return;
    if (mode === 'rich') {
      tick().then(() => richEditor?.selectText(query));
      return;
    }
    if (!textareaEl) return;
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
      } else if (b.startsWith('domain:')) {
        chips.push({ label: b.slice('domain:'.length), color: 'var(--text-2)', onremove: () => notesStore.removeNoteBinding(note!.id, b) });
      }
    }

    return chips;
  });

  const formatUpdatedAt = (iso: string): string => relTime(iso, $locale);

  async function confirmDeleteForever() {
    if (!note) return;
    await notesStore.deleteForever(note.id);
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
    setContent(mergedContent);
    mergeHistoryId = null;
    showHistory = false;
  }

  async function onSyncConflictResolved(mergedContent: string) {
    if (!note) return;
    const id = note.id;
    try {
      await api.sync.conflictResolve(id, mergedContent);
      const fresh = await api.notes.get(id);
      onHistoryRestore(fresh);
      await syncStore.refresh();
    } catch (e) {
      syncStore.error = formatError(e);
    }
    showSyncConflict = false;
  }

  function onTitleChange() {
    notesStore.onTitleChange(titleValue);
  }

  function onBodyChange() {
    notesStore.onContentChange(contentValue);
    updateWikiState();
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

    <!-- Sync conflict banner: the file stays untouched until resolved -->
    {#if syncConflict}
      <div class="banner banner-warn">
        <span>{$t('note_sync_conflict')}</span>
        <div class="banner-actions">
          <button onclick={() => (showSyncConflict = true)}>{$t('note_sync_conflict_resolve')}</button>
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
          class:active={attachmentsOpen}
          onclick={() => (attachmentsOpen = !attachmentsOpen)}
          title={$t('note_att_title')}
        >
          <Icon name="paperclip" size={13} />
          {#if attachments.length > 0}<span class="att-count">{attachments.length}</span>{/if}
        </button>
        <button
          class="icon-action"
          class:active={linksOpen}
          onclick={() => (linksOpen = !linksOpen)}
          title={$t('note_links_title')}
        >
          <Icon name="link" size={13} />
        </button>
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
      <NoteToolbar
        {mode}
        {findOpen}
        disabled={readonly}
        onaction={runAction}
        onmode={setMode}
        ontogglefind={() => (findOpen ? (findOpen = false) : openFind())}
      />
      {#if findOpen}
        <NoteFindBar
          bind:this={findBar}
          textarea={textareaEl}
          content={contentValue}
          {readonly}
          onreplace={setContent}
          onclose={() => { findOpen = false; textareaEl?.focus(); }}
        />
      {/if}
      <div class="panes" bind:this={panesEl}>
        {#if mode === 'rich'}
          {#key note.id}
            <NoteRichEditor
              bind:this={richEditor}
              content={contentValue}
              baseDir={note.base_dir}
              noteId={note.id}
              {readonly}
              placeholder={$t('note_content_placeholder')}
              notes={notesStore.list}
              excludeId={note.id}
              onchange={setContent}
              onwikilink={openWikiLink}
              onfiles={uploadFiles}
              onclipboardfiles={pasteClipboardFiles}
              onhotkey={onRichHotkey}
            />
          {/key}
        {:else}
          <textarea
            bind:this={textareaEl}
            bind:value={contentValue}
            class="editor-body"
            placeholder={$t('note_content_placeholder')}
            oninput={onBodyChange}
            onkeydown={onEditorKeydown}
            onpaste={onPaste}
            onblur={() => (wikiOpen = false)}
            {readonly}
            spellcheck="false"
          ></textarea>
          {#if wikiOpen}
            <WikiLinkPicker
              bind:this={wikiPicker}
              bind:index={wikiIndex}
              query={wikiQuery}
              notes={notesStore.list}
              excludeId={note.id}
              onpick={pickWikiLink}
            />
          {/if}
        {/if}
      </div>
      {#if linksOpen}
        <NoteLinks noteId={note.id} version={linksVersion} onopen={(id) => notesStore.openNote(id)} />
      {/if}
      {#if attachmentsOpen}
        <NoteAttachments
          noteId={note.id}
          {attachments}
          {readonly}
          oninsert={insertAttachmentLink}
          onchanged={loadAttachments}
          onpick={uploadFiles}
        />
      {/if}
    </div>

    <div class="editor-footer">
      <span class="footer-meta">
        {note.format.toUpperCase()}
        {#if note.bindings.length > 0}
          · {note.bindings.some(b => b.startsWith('profile:')) ? 'profile' : 'workspace'}
        {/if}
        · {$t('note_stats', { w: String(stats.words), c: String(stats.chars) })}
      </span>
      <div class="footer-actions">
        {#if note.deleted}
          <button
            class="icon-action"
            onclick={() => note && notesStore.restoreNote(note.id)}
            title={$t('note_btn_restore')}
          >
            <Icon name="rotate-ccw" size={13} />
          </button>
          <button
            class="icon-action icon-danger"
            onclick={() => (showDeleteConfirm = true)}
            title={$t('note_btn_delete_forever')}
          >
            <Icon name="trash-2" size={13} />
          </button>
        {:else}
          <button
            class="icon-action"
            onclick={() => note && notesStore.togglePin(note.id)}
            title={note.pinned ? $t('note_btn_unpin') : $t('note_btn_pin')}
            class:active={note.pinned}
          >
            <Icon name="pin" size={13} />
          </button>
          {#if note.archived}
            <button
              class="icon-action active"
              onclick={() => note && notesStore.restoreNote(note.id)}
              title={$t('note_btn_unarchive')}
            >
              <Icon name="archive" size={13} />
            </button>
          {:else}
            <button
              class="icon-action"
              onclick={() => note && notesStore.archiveNote(note.id)}
              title={$t('note_btn_archive')}
            >
              <Icon name="archive" size={13} />
            </button>
          {/if}
          <button
            class="icon-action"
            onclick={() => note && api.notes.openExternal(note.id)}
            title={$t('note_btn_open_external')}
          >
            <Icon name="external-link" size={13} />
          </button>
          <button
            class="icon-action icon-danger"
            onclick={() => note && notesStore.deleteNote(note.id)}
            title={$t('note_btn_delete')}
          >
            <Icon name="trash-2" size={13} />
          </button>
        {/if}
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
            <button class="btn btn-danger btn-sm" onclick={confirmDeleteForever}>{$t('note_delete_confirm')}</button>
          </div>
        </div>
      </div>
    {/if}

    {#if mergeHistoryId}
      {@const noteId = note.id}
      {@const historyId = mergeHistoryId}
      <NoteHistoryMerge
        load={() => api.notes.historyMerge(noteId, historyId)}
        onresolved={onMergeResolved}
        oncancel={() => (mergeHistoryId = null)}
      />
    {:else if showSyncConflict}
      {@const noteId = note.id}
      <NoteHistoryMerge
        load={() => api.sync.conflictGet(noteId)}
        theirsLabel={$t('note_sync_conflict_remote')}
        theirsShort={$t('note_sync_conflict_remote_short')}
        onresolved={onSyncConflictResolved}
        oncancel={() => (showSyncConflict = false)}
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

  .banner-actions button:hover { background: color-mix(in srgb, currentColor 12%, transparent); }

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
    font-size: var(--fs-lg);
    font-weight: var(--fw-extrabold);
    letter-spacing: -0.3px;
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

  .panes {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .panes > :global(*) { flex: 1; min-width: 0; }

  .editor-body {
    flex: 1;
    padding: var(--sp-3) var(--sp-5);
    background: none;
    border: none;
    outline: none;
    resize: none;
    font-size: 0.95rem;
    line-height: 1.75;
    color: var(--text-body);
    font-family: var(--font-mono);
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
    font-family: var(--font-mono);
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
  .att-count {
    font-size: var(--fs-2xs);
    font-family: var(--font-mono);
    margin-left: 2px;
  }
  .icon-action.active { color: var(--accent); }
  .icon-danger:hover { color: var(--danger-text) !important; }

  .delete-overlay {
    position: absolute;
    inset: 0;
    background: var(--backdrop);
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
    color: var(--text);
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
