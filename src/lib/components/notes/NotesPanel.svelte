<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { portal } from '$lib/portal';
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api } from '$lib/api';
  import type { NoteCreateInput } from '$lib/types';
  import { toNoteFilter, templateNotes, type ActiveFilter } from '$lib/notes-filter';
  import TemplateSelect from './TemplateSelect.svelte';
  import NoteLockGate from './NoteLockGate.svelte';
  import Icon from '$lib/Icon.svelte';
  import NotesList from './NotesList.svelte';
  import NoteEditor from './NoteEditor.svelte';
  import NoteFilters from './NoteFilters.svelte';
  import NoteTransferDialog, { type TransferMode } from './NoteTransferDialog.svelte';
  import NoteSyncButton from './NoteSyncButton.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    open?: boolean;
    context?: 'global' | 'workspace' | 'profile';
    contextId?: string;
    workspaceId?: string;
    openNoteId?: string | null;
  }

  let { open = $bindable(false), context = 'global', contextId, workspaceId, openNoteId = null }: Props = $props();

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
  let panelBodyEl: HTMLElement | null = $state(null);
  let dragging: 'sidebar' | 'list' | null = $state(null);
  let hoveredResizer: 'sidebar' | 'list' | null = $state(null);

  // ── Panel width: resizable, persisted globally (shared across profiles/workspaces) ──
  const PANEL_W_KEY = 'notes-panel-width';
  function loadPanelWidth(): number {
    try {
      const v = parseInt(localStorage.getItem(PANEL_W_KEY) ?? '', 10);
      if (Number.isFinite(v)) return v;
    } catch {}
    return 900;
  }
  let panelWidth = $state(loadPanelWidth());
  let panelDragging = $state(false);

  function clampPanelWidth(w: number): number {
    return Math.max(640, Math.min(window.innerWidth * 0.96, w));
  }

  function startPanelDrag(e: MouseEvent) {
    e.preventDefault();
    panelDragging = true;

    function onMove(ev: MouseEvent) {
      panelWidth = clampPanelWidth(window.innerWidth - ev.clientX);
    }
    function onUp() {
      panelDragging = false;
      try { localStorage.setItem(PANEL_W_KEY, String(Math.round(panelWidth))); } catch {}
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function saveColWidths() {
    try { localStorage.setItem('notes-col-widths', JSON.stringify(colWidths)); } catch {}
  }

  function startColDrag(e: MouseEvent, col: 'sidebar' | 'list') {
    e.preventDefault();
    dragging = col;

    function onMove(ev: MouseEvent) {
      if (!panelBodyEl) return;
      const rect = panelBodyEl.getBoundingClientRect();
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

    return () => { /* watcher stays alive across panel opens */ };
  });

  $effect(() => {
    if (open) {
      panelWidth = loadPanelWidth();
      untrack(() => notesStore.ensureLoaded());
      // Pre-set filter based on context
      const initial: ActiveFilter =
        context === 'workspace' && contextId ? { type: 'workspace', id: contextId }
        : context === 'profile' && contextId ? { type: 'profile', id: contextId }
        : { type: 'all' };
      untrack(() => void handleFilterChange(initial));
      // Open specific note if requested
      if (openNoteId && untrack(() => notesStore.activeNoteId) !== openNoteId) {
        void notesStore.openNote(openNoteId);
      }
    }
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
        n.tags.some((t) => t.name.toLowerCase().includes(q))
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

    // Build bindings from active filter first, fallback to context prop
    const bindings: string[] = [];

    if (activeFilter.type === 'workspace' && activeFilter.id) {
      bindings.push(`workspace:${activeFilter.id}`);
    } else if (activeFilter.type === 'profile' && activeFilter.id) {
      bindings.push(`profile:${activeFilter.id}`);
      if (workspaceId) bindings.push(`workspace:${workspaceId}`);
    } else if (context === 'workspace' && contextId) {
      bindings.push(`workspace:${contextId}`);
    } else if (context === 'profile' && contextId) {
      bindings.push(`profile:${contextId}`);
      if (workspaceId) bindings.push(`workspace:${workspaceId}`);
    }
    // 'global', 'all', 'pinned', 'archived' → no bindings (global note)

    const input: NoteCreateInput = {
      title: createTitle.trim(),
      bindings,
      template_id: createTemplate || undefined,
    };

    try {
      const note = await notesStore.createNote(input);
      showCreate = false;
      createTitle = '';
      await notesStore.openNote(note.id);
    } catch {}
  }

  async function handleSelectNote(id: string) {
    await notesStore.openNote(id);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (showCreate) showCreate = false;
      else open = false;
    }
  }

  function workspaceName(id: string): string {
    return workspacesStore.list.find((w) => w.id === id)?.name ?? id;
  }

  function profileName(id: string): string {
    return profilesStore.list.find((p) => p.id === id)?.name ?? id;
  }

  // ── Export / import ──────────────────────────────────────────────────────────
  let transferMode = $state<TransferMode | null>(null);
  let exportIds = $state<string[]>([]);

  function openExport(ids: string[]) {
    exportIds = ids;
    transferMode = 'export';
  }

  function exportFolder(folderId: string) {
    openExport(notesStore.list.filter((n) => n.folder_ids.includes(folderId)).map((n) => n.id));
  }

  const importBindings = $derived(
    activeFilter.type === 'workspace' || activeFilter.type === 'profile'
      ? [`${activeFilter.type}:${activeFilter.id}`]
      : []
  );
</script>

<NoteTransferDialog mode={transferMode} {exportIds} {importBindings} onclose={() => (transferMode = null)} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div
    class="overlay"
    use:portal
    onclick={(e) => { if (e.target === e.currentTarget) open = false; }}
    onkeydown={onKeydown}
    role="presentation"
    tabindex="-1"
  >
    <div
      class="panel"
      class:panel-dragging={panelDragging}
      style="width: min({panelWidth}px, 96vw)"
      role="dialog"
      aria-label={$t('notes_title')}
    >
      <!-- Left-edge resizer: drag to set panel width (persisted globally) -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="panel-edge-resizer"
        class:active={panelDragging}
        onmousedown={startPanelDrag}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize panel"
      ></div>
      <!-- Header -->
      <div class="panel-header">
        <div class="header-title">
          <Icon name="file-text" size={16} />
          <h3>{$t('notes_title')}</h3>
          <span class="badge badge-accent">{displayList.length}</span>
        </div>
        <div class="header-actions">
          <button class="close-btn" onclick={() => (open = false)}>
            <Icon name="x" size={15} />
          </button>
        </div>
      </div>

      <NoteLockGate>
      <!-- Body: sidebar + list + editor (same layout as the Notes screen) -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="panel-body" bind:this={panelBodyEl} class:is-dragging={dragging !== null}>
        {#if sidebarVisible}
          <div class="sidebar" style="width: {colWidths.sidebar}%">
            <div class="search-wrap">
              <span class="search-icon"><Icon name="search" size={15} /></span>
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
              <button class="btn btn-primary btn-new" onclick={() => (showCreate = true)}>
                <Icon name="plus" size={14} /> {$t('notes_btn_new')}
              </button>
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
              folderName={(id) => notesStore.folders.find(f => f.id === id)?.name ?? id}
              folderColor={(id) => notesStore.folders.find(f => f.id === id)?.color ?? 'var(--text-2)'}
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

      <!-- Create modal -->
      {#if showCreate}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
        <div class="create-overlay" onclick={(e) => { if (e.target === e.currentTarget) showCreate = false; }} role="presentation">
          <div class="create-card">
            <h4>{$t('notes_create_title')}</h4>
            <input
              type="text"
              bind:value={createTitle}
              placeholder={$t('notes_title_placeholder')}
              class="create-title"
              onkeydown={(e) => e.key === 'Enter' && createNote()}
            />
            {#if hasTemplates}
              <TemplateSelect class="create-title" bind:value={createTemplate} />
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
      </NoteLockGate>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    z-index: var(--z-drawer);
    display: flex;
    justify-content: flex-end;
  }

  .panel {
    background: var(--surface-drawer);
    border-left: 1px solid var(--border);
    min-width: 640px;
    height: 100%;
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow-drawer);
    animation: slide-in var(--dur-drawer) var(--ease-drawer);
    position: relative;
  }
  .panel.panel-dragging { user-select: none; }

  /* Left-edge drag handle for panel width */
  .panel-edge-resizer {
    position: absolute;
    left: -3px;
    top: 0;
    bottom: 0;
    width: 8px;
    cursor: col-resize;
    z-index: 2;
  }
  .panel-edge-resizer::before {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 1px;
    background: transparent;
    transition: background 0.15s, width 0.1s;
  }
  .panel-edge-resizer:hover::before,
  .panel-edge-resizer.active::before {
    background: var(--accent);
    width: 2px;
  }

  @keyframes slide-in {
    from { transform: translateX(100%); }
    to { transform: translateX(0); }
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.85rem var(--sp-5);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }

  .header-title h3 {
    margin: 0;
    font-size: 1.15rem;
    font-weight: var(--fw-extrabold);
    letter-spacing: -0.3px;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.2rem;
  }

  /* .icon-btn is the global 32px primitive (base.css) */
  .close-btn {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-soft);
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 9px;
    transition: all var(--dur-fast);
  }

  .close-btn:hover { color: var(--text); background: var(--surface-hover); }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
    margin: var(--sp-3) var(--sp-3) var(--sp-2);
    flex-shrink: 0;
  }

  .search-icon {
    position: absolute;
    left: 0.75rem;
    color: var(--text-3);
    pointer-events: none;
    display: flex;
  }

  .search-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.42rem 0.6rem 0.42rem var(--sp-8);
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--fs-sm);
  }

  .search-input:focus {
    outline: none;
    border-color: var(--accent-border);
    box-shadow: 0 0 0 3px var(--accent-bg);
  }

  .search-spinner {
    position: absolute;
    right: 0.6rem;
    color: var(--accent);
    display: flex;
    animation: spin 1s linear infinite;
  }

  .panel-body {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .panel-body.is-dragging {
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

  .sidebar-footer {
    display: flex;
    gap: 6px;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    margin-top: auto;
    flex-shrink: 0;
  }
  .sidebar-footer .icon-btn { width: 32px; height: 32px; }

  .list-col {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-width: 140px;
  }

  .list-header {
    padding: var(--sp-4) var(--sp-4) var(--sp-3);
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .list-title-group { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .list-title {
    margin: 0;
    font-size: 1.25rem;
    font-weight: var(--fw-extrabold);
    letter-spacing: -0.3px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .list-sub { margin: 0; font-size: 0.78rem; color: var(--text-faint); }

  .list-actions { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .list-actions .icon-btn { width: 32px; height: 32px; }

  .btn-new {
    height: 34px;
    padding: 0 14px;
    font-size: 0.82rem;
    border-radius: 9px;
  }

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
    position: absolute;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: var(--z-sticky);
  }

  .create-card {
    background: var(--surface);
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

  .create-title {
    background: var(--surface-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.45rem var(--sp-3);
    color: var(--text);
    font-size: var(--fs-base);
    width: 100%;
    box-sizing: border-box;
  }

  .create-title:focus {
    outline: none;
    border-color: var(--accent-border);
    box-shadow: 0 0 0 3px var(--accent-bg);
  }

  .create-actions {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
  }
</style>
