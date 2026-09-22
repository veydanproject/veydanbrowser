<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api, isNotesWindow } from '$lib/api';
  import type { NoteCreateInput } from '$lib/types';
  import { toNoteFilter, contextBindings, templateNotes, type ActiveFilter } from '$lib/notes-filter';
  import TemplateSelect from '$lib/components/notes/TemplateSelect.svelte';
  import NoteLockGate from '$lib/components/notes/NoteLockGate.svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import Icon from '$lib/Icon.svelte';
  import NotesList from '$lib/components/notes/NotesList.svelte';
  import ListBulkActions from '$lib/components/notes/ListBulkActions.svelte';
  import NoteEditor from '$lib/components/notes/NoteEditor.svelte';
  import NoteFilters from '$lib/components/notes/NoteFilters.svelte';
  import NoteTransferDialog, { type TransferMode } from '$lib/components/notes/NoteTransferDialog.svelte';
  import NoteSyncButton from '$lib/components/notes/NoteSyncButton.svelte';
  import { t } from '$lib/i18n';

  const standaloneNotes = isNotesWindow();

  let searchQuery = $state('');
  let searching = $state(false);
  let showCreate = $state(false);
  let createTitle = $state('');
  let createTemplate = $state('');
  const hasTemplates = $derived(templateNotes(notesStore.list, notesStore.folders).length > 0);
  let activeFilter = $state<ActiveFilter>({ type: 'all' });
  let sidebarVisible = $state(true);

  // Resizable columns
  function loadColWidths(): { sidebar: number; list: number } {
    try {
      const v = localStorage.getItem('notes-col-widths');
      if (v) return { sidebar: 20, list: 27, ...JSON.parse(v) };
    } catch {}
    return { sidebar: 20, list: 27 };
  }

  let colWidths = $state(loadColWidths());
  let pageBodyEl: HTMLElement | null = $state(null);
  let dragging: 'sidebar' | 'list' | null = $state(null);
  let hoveredResizer: 'sidebar' | 'list' | null = $state(null);

  function saveColWidths() {
    try { localStorage.setItem('notes-col-widths', JSON.stringify(colWidths)); } catch {}
  }

  function startColDrag(e: MouseEvent, col: 'sidebar' | 'list') {
    e.preventDefault();
    dragging = col;

    function onMove(ev: MouseEvent) {
      if (!pageBodyEl) return;
      const rect = pageBodyEl.getBoundingClientRect();
      const pct = ((ev.clientX - rect.left) / rect.width) * 100;
      if (col === 'sidebar') {
        colWidths.sidebar = Math.max(10, Math.min(30, pct));
      } else {
        colWidths.list = Math.max(15, Math.min(45, pct - (sidebarVisible ? colWidths.sidebar : 0)));
      }
      saveColWidths();
    }

    function onUp() {
      dragging = null;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  onMount(() => {
    notesStore.ensureLoaded();
    notesStore.refreshTrash();
    workspacesStore.ensureLoaded();
    profilesStore.ensureLoaded();
    notesStore.startWatcher();
    // Note id passed by the browser extension when this window was just created
    const requested = new URLSearchParams(window.location.search).get('open');
    if (requested) notesStore.openRequestId = requested;
  });

  const isTrash = $derived(activeFilter.type === 'trash');

  // Filtering happens on the backend (notesStore.view); only the 1-char
  // search (too short for FTS) and trash search are applied locally.
  const displayList = $derived.by(() => {
    const q = searchQuery.trim().toLowerCase();
    const list = notesStore.view;
    if (!q || (q.length >= 2 && !isTrash)) return list;
    return list.filter(
      (n) =>
        n.title.toLowerCase().includes(q) ||
        n.tags.some((tg: { name: string }) => tg.name.toLowerCase().includes(q)) ||
        n.folder_ids.some(fid => notesStore.folders.find(f => f.id === fid)?.name.toLowerCase().includes(q))
    );
  });

  async function handleSearch() {
    searching = true;
    try {
      await notesStore.search(searchQuery);
    } catch {}
    finally { searching = false; }
  }

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearchInput() {
    if (isTrash) return;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      if (searchQuery.trim().length >= 2 || notesStore.searchQuery) void handleSearch();
    }, 350);
  }

  $effect(() => {
    const id = notesStore.openRequestId;
    if (!id) return;
    notesStore.openRequestId = null;
    void handleFilterChange({ type: 'all' }).then(() => notesStore.openNote(id));
  });

  $effect(() => {
    if (notesStore.activeNoteId && !displayList.some(n => n.id === notesStore.activeNoteId)) {
      notesStore.activeNoteId = null;
      notesStore.activeNote = null;
    }
  });

  async function handleFilterChange(f: ActiveFilter) {
    activeFilter = f;
    searchQuery = '';
    // Smart views are applied by the effect below (also re-applied when edited)
    if (f.type !== 'smart') await notesStore.setFilter(toNoteFilter(f));
    if (f.type === 'trash') await notesStore.refreshTrash();
  }

  $effect(() => {
    if (activeFilter.type !== 'smart') return;
    const view = notesStore.smartViews.find((v) => v.id === activeFilter.id);
    if (!view) {
      void handleFilterChange({ type: 'all' });
      return;
    }
    void notesStore.setFilter(toNoteFilter(activeFilter, notesStore.smartViews));
  });

  async function createNote() {
    if (!createTitle.trim()) return;

    const input: NoteCreateInput = {
      title: createTitle.trim(),
      bindings: contextBindings(activeFilter),
      template_id: createTemplate || undefined,
    };
    try {
      const note = await notesStore.createNote(input);

      if (activeFilter.type === 'folder' && activeFilter.id) {
        await api.notes.noteAddFolder(note.id, activeFilter.id);
        await notesStore.refresh();
      }

      showCreate = false;
      createTitle = '';
      await notesStore.openNote(note.id);
    } catch {}
  }

  async function handleSelectNote(id: string) {
    await notesStore.openNote(id);
  }

  function workspaceName(id: string): string {
    return workspacesStore.list.find((w) => w.id === id)?.name ?? id;
  }

  function profileName(id: string): string {
    return profilesStore.list.find((p) => p.id === id)?.name ?? id;
  }

  function folderName(id: string): string {
    return notesStore.folders.find((f) => f.id === id)?.name ?? id;
  }

  function folderDescendantIds(folderId: string): Set<string> {
    const result = new Set<string>([folderId]);
    for (const f of notesStore.folders) {
      if (f.parent_id === folderId) {
        for (const id of folderDescendantIds(f.id)) result.add(id);
      }
    }
    return result;
  }

  function folderColor(id: string): string {
    return notesStore.folders.find((f) => f.id === id)?.color ?? 'var(--text-2)';
  }

  // ── Export / import ──────────────────────────────────────────────────────────
  let transferMode = $state<TransferMode | null>(null);
  let exportIds = $state<string[]>([]);

  function openExport(ids: string[]) {
    exportIds = ids;
    transferMode = 'export';
  }

  function exportFolder(folderId: string) {
    const ids = folderDescendantIds(folderId);
    openExport(notesStore.list.filter((n) => n.folder_ids.some((f) => ids.has(f))).map((n) => n.id));
  }

  // Imported notes inherit the active workspace/profile context
  const importBindings = $derived(contextBindings(activeFilter));
</script>

<NoteTransferDialog mode={transferMode} {exportIds} {importBindings} onclose={() => (transferMode = null)} />

<div class="page page--fill notes-page">
  <NoteLockGate>
  <!-- Body: 3 columns full height per redesign (search lives in the sidebar) -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="page-body" bind:this={pageBodyEl} class:is-dragging={dragging !== null}>
    {#if sidebarVisible}
      <div class="sidebar" style="width: {colWidths.sidebar}%">
        <div class="search-wrap">
          <Icon name="search" size={15} />
          <input
            type="text"
            bind:value={searchQuery}
            oninput={onSearchInput}
            placeholder={$t('notes_search_placeholder')}
            class="search-input"
          />
          {#if searching}
            <span class="search-spinner"><Icon name="loader" size={12} /></span>
          {/if}
        </div>
        <NoteFilters
          notes={notesStore.list}
          trashCount={notesStore.trash.length}
          allTags={notesStore.allTags}
          folders={notesStore.folders}
          {activeFilter}
          onfilter={handleFilterChange}
          onexportfolder={exportFolder}
        />
        <div class="sidebar-footer">
          <NoteSyncButton />
          <button class="icon-btn" title={$t('notes_btn_open_folder')} onclick={() => api.notes.openFolder()}>
            <Icon name="folder-open" size={14} />
          </button>
          <button class="icon-btn" title={$t('notes_btn_import')} onclick={() => { exportIds = []; transferMode = 'import'; }}>
            <Icon name="upload" size={14} />
          </button>
          <button class="icon-btn" title={$t('notes_btn_export')} onclick={() => openExport(displayList.map((n) => n.id))}>
            <Icon name="download" size={14} />
          </button>
          {#if notesLock.status.enabled}
            <button class="icon-btn" title={$t('notes_lock_now')} onclick={() => notesLock.lock()}>
              <Icon name="lock" size={14} />
            </button>
          {/if}
          {#if !standaloneNotes}
            <button
              class="icon-btn"
              onclick={() => api.notes.openWindow($t('nav_notes'))}
              title={$t('notes_btn_open_window')}
            >
              <Icon name="file-text" size={14} />
            </button>
          {/if}
          <button class="icon-btn" onclick={() => { sidebarVisible = false; }} title={$t('notes_btn_toggle_sidebar')}>
            <Icon name="sidebar" size={14} />
          </button>
        </div>
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="col-resizer"
        class:active={hoveredResizer === 'sidebar' || dragging === 'sidebar'}
        onmousedown={(e) => startColDrag(e, 'sidebar')}
        onmouseenter={() => (hoveredResizer = 'sidebar')}
        onmouseleave={() => (hoveredResizer = null)}
      ></div>
    {/if}

    <div class="list-col" style="width: {colWidths.list}%">
      <div class="list-header">
        <div class="list-title-group">
          <h2 class="list-title">{$t('notes_filter_all')}</h2>
          <p class="list-sub">
            {displayList.length === 1
              ? $t('panel_notes_count_one', { n: String(displayList.length) })
              : $t('panel_notes_count_many', { n: String(displayList.length) })}
          </p>
        </div>
        <div class="list-actions">
          {#if !sidebarVisible}
            <button class="icon-btn" onclick={() => { sidebarVisible = true; }} title={$t('notes_btn_toggle_sidebar')}>
              <Icon name="sidebar" size={14} />
            </button>
          {/if}
          {#if isTrash}
            <ListBulkActions {isTrash} notes={displayList} />
          {:else}
            <button class="btn-new" onclick={() => (showCreate = true)} title={$t('notes_btn_new')} aria-label={$t('notes_btn_new')}>
              <Icon name="plus" size={16} />
            </button>
          {/if}
        </div>
      </div>
      <div class="list-scroll">
        <NotesList
          notes={displayList}
          activeId={notesStore.activeNoteId}
          onselect={handleSelectNote}
          oncreate={() => (showCreate = true)}
          {workspaceName}
          {profileName}
          {folderName}
          {folderColor}
        />
      </div>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="col-resizer"
      class:active={hoveredResizer === 'list' || dragging === 'list'}
      onmousedown={(e) => startColDrag(e, 'list')}
      onmouseenter={() => (hoveredResizer = 'list')}
      onmouseleave={() => (hoveredResizer = null)}
    ></div>

    <div class="editor-col">
      <NoteEditor allTags={notesStore.allTags} folders={notesStore.folders} />
    </div>
  </div>
  </NoteLockGate>
</div>

<!-- Create modal -->
{#if showCreate && !notesLock.locked}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div class="create-overlay" onclick={(e) => { if (e.target === e.currentTarget) showCreate = false; }} role="presentation">
    <div class="create-card">
      <h4>{$t('notes_create_title')}</h4>
      <input
        type="text"
        bind:value={createTitle}
        placeholder={$t('notes_title_placeholder')}
        onkeydown={(e) => e.key === 'Enter' && createNote()}
      />
      {#if hasTemplates}
        <TemplateSelect bind:value={createTemplate} />
      {/if}
      <div class="create-actions">
        <button class="btn btn-ghost" onclick={() => (showCreate = false)}>{$t('notes_btn_cancel')}</button>
        <button class="btn btn-primary" onclick={createNote} disabled={!createTitle.trim()}>
          {$t('notes_btn_create')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .notes-page {
    overflow: hidden;
    gap: 0.875rem;
    max-width: none;
  }

  /* Search now lives at the top of the sidebar (per redesign) */
  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
    margin: var(--sp-4) var(--sp-3) var(--sp-2);
    flex-shrink: 0;
  }

  .search-wrap :global(svg) {
    position: absolute;
    left: 0.75rem;
    color: var(--text-3);
    pointer-events: none;
  }

  .search-input {
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    width: 100%;
    padding: 0 1.75rem 0 var(--sp-8);
    font-size: var(--fs-sm);
    height: 38px;
    outline: none;
    transition: border-color 0.15s;
  }

  .search-input:focus { border-color: var(--accent-border); }

  .search-spinner {
    position: absolute;
    right: 0.5rem;
    color: var(--accent);
    display: flex;
    animation: spin 1s linear infinite;
  }

  .page-body {
    flex: 1;
    display: flex;
    overflow: hidden;
    border: 1px solid var(--border);
    min-height: 0;
    border-radius: var(--radius-lg);
    background: var(--surface);
  }

  .page-body.is-dragging {
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

  .sidebar {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    min-width: 100px;
  }

  .list-col {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 140px;
  }

  .list-header {
    padding: var(--sp-5) var(--sp-4) var(--sp-3);
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .list-title-group { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .list-title {
    font-size: 1.25rem;
    font-weight: var(--fw-extrabold);
    letter-spacing: -0.3px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .list-sub { font-size: 0.78rem; color: var(--text-faint); }

  .list-actions { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .list-actions .icon-btn { width: 32px; height: 32px; }

  .sidebar-footer {
    display: flex;
    gap: 6px;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    margin-top: auto;
    flex-shrink: 0;
  }
  .sidebar-footer .icon-btn { width: 32px; height: 32px; }
  .btn-new {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--accent-grad);
    color: #fff;
    box-shadow: var(--shadow-accent);
    cursor: pointer;
    flex-shrink: 0;
  }
  .btn-new:hover { filter: brightness(1.08); }

  .list-scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-2);
  }

  .editor-col {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 0;
  }

  /* Create modal */
  .create-overlay {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: var(--z-modal);
  }

  .create-card {
    background: var(--surface-drawer);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--sp-5);
    width: 320px;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    box-shadow: var(--shadow-lg);
  }

  .create-card h4 {
    margin: 0;
    font-size: var(--fs-md);
    font-weight: 600;
  }

  .create-actions {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
  }
</style>
