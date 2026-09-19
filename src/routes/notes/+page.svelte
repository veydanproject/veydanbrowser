<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api, isNotesWindow } from '$lib/api';
  import type { NoteCreateInput, NoteFilter } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import NotesList from '$lib/components/notes/NotesList.svelte';
  import NoteEditor from '$lib/components/notes/NoteEditor.svelte';
  import NoteFilters from '$lib/components/notes/NoteFilters.svelte';
  import { t } from '$lib/i18n';

  const standaloneNotes = isNotesWindow();

  let searchQuery = $state('');
  let searching = $state(false);
  let showCreate = $state(false);
  let createTitle = $state('');
  let createFormat = $state<string>('txt');
  let activeFilter = $state<{ type: string; id?: string }>({ type: 'all' });
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
    workspacesStore.ensureLoaded();
    profilesStore.ensureLoaded();
    notesStore.startWatcher();
  });

  const noteFilter = $derived.by((): NoteFilter => {
    switch (activeFilter.type) {
      case 'workspace': return { binding: `workspace:${activeFilter.id}` };
      case 'profile':   return { binding: `profile:${activeFilter.id}` };
      case 'tag':       return { tag_name: activeFilter.id };
      case 'pinned':    return { pinned: true };
      case 'archived':  return { archived: true, include_deleted: false };
      default:          return {};
    }
  });

  const displayList = $derived.by(() => {
    let list = notesStore.list;

    if (activeFilter.type !== 'all') {
      if (activeFilter.type === 'global')
        list = list.filter((n) => !n.bindings.some((b: string) => b.startsWith('workspace:') || b.startsWith('profile:')) && !n.archived);
      else if (activeFilter.type === 'workspace')
        list = list.filter((n) => n.bindings.includes(`workspace:${activeFilter.id}`) && !n.archived);
      else if (activeFilter.type === 'profile')
        list = list.filter((n) => n.bindings.includes(`profile:${activeFilter.id}`) && !n.archived);
      else if (activeFilter.type === 'tag')
        list = list.filter((n) => n.tags.some((tg: { name: string }) => tg.name === activeFilter.id) && !n.archived);
      else if (activeFilter.type === 'tag-group')
        list = list.filter((n) => n.tags.some((tg: { name: string }) => tg.name === activeFilter.id || tg.name.startsWith(activeFilter.id + '/')) && !n.archived);
      else if (activeFilter.type === 'folder') {
        const ids = folderDescendantIds(activeFilter.id!);
        list = list.filter((n) => n.folder_ids.some(fid => ids.has(fid)) && !n.archived);
      }
      else if (activeFilter.type === 'pinned')
        list = list.filter((n) => n.pinned && !n.archived);
      else if (activeFilter.type === 'archived')
        list = list.filter((n) => n.archived);
    } else {
      list = list.filter((n) => !n.archived);
    }

    // Локальный фильтр только для 1 символа (< 2 не вызывает API)
    if (searchQuery.trim().length === 1) {
      const q = searchQuery.toLowerCase();
      list = list.filter(
        (n) =>
          n.title.toLowerCase().includes(q) ||
          n.tags.some((tg: { name: string }) => tg.name.toLowerCase().includes(q)) ||
          n.folder_ids.some(fid => notesStore.folders.find(f => f.id === fid)?.name.toLowerCase().includes(q))
      );
    }

    return list;
  });

  async function handleSearch() {
    if (!searchQuery.trim()) {
      notesStore.searchQuery = '';
      await notesStore.refresh();
      return;
    }
    searching = true;
    try {
      const results = await api.notes.search(searchQuery, noteFilter);
      notesStore.list = results;
      notesStore.searchQuery = searchQuery.trim();
    } catch {}
    finally { searching = false; }
  }

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearchInput() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => {
      if (searchQuery.trim().length >= 2) void handleSearch();
      else void notesStore.refresh();
    }, 350);
  }

  $effect(() => {
    if (notesStore.activeNoteId && !displayList.some(n => n.id === notesStore.activeNoteId)) {
      notesStore.activeNoteId = null;
      notesStore.activeNote = null;
    }
  });

  async function handleFilterChange(f: { type: string; id?: string }) {
    activeFilter = f;
    searchQuery = '';
    notesStore.searchQuery = '';
    await notesStore.refresh();
  }

  async function createNote() {
    if (!createTitle.trim()) return;

    const bindings: string[] = [];
    if (activeFilter.type === 'workspace' && activeFilter.id)
      bindings.push(`workspace:${activeFilter.id}`);
    else if (activeFilter.type === 'profile' && activeFilter.id)
      bindings.push(`profile:${activeFilter.id}`);

    const input: NoteCreateInput = {
      title: createTitle.trim(),
      format: createFormat,
      bindings,
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
</script>

<div class="page page--fill notes-page">
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
          allTags={notesStore.allTags}
          folders={notesStore.folders}
          {activeFilter}
          onfilter={handleFilterChange}
        />
        <div class="sidebar-footer">
          <button class="icon-btn" title={$t('notes_btn_sync')} onclick={() => api.notes.sync()}>
            <Icon name="refresh-cw" size={14} />
          </button>
          <button class="icon-btn" title={$t('notes_btn_open_folder')} onclick={() => api.notes.openFolder()}>
            <Icon name="folder-open" size={14} />
          </button>
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
        onkeydown={(e) => e.key === 'Enter' && createNote()}
      />
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
    height: 36px;
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
