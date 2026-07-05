<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api } from '$lib/api';
  import type { NoteCreateInput, NoteFilter } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import NotesList from '$lib/components/notes/NotesList.svelte';
  import NoteEditor from '$lib/components/notes/NoteEditor.svelte';
  import NoteFilters from '$lib/components/notes/NoteFilters.svelte';
  import { t } from '$lib/i18n';

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
  <!-- Header -->
  <div class="page-header">
    <h1>{$t('notes_title')}</h1>
    <div class="header-actions">
      <button class="icon-btn" title={$t('notes_btn_sync')} onclick={() => api.notes.sync()}>
        <Icon name="refresh-cw" size={13} />
      </button>
      <button class="icon-btn" title={$t('notes_btn_open_folder')} onclick={() => api.notes.openFolder()}>
        <Icon name="folder-open" size={13} />
      </button>
      <button class="icon-btn" onclick={() => { sidebarVisible = !sidebarVisible; }} title={$t('notes_btn_toggle_sidebar')}>
        <Icon name="sidebar" size={13} />
      </button>
    </div>
  </div>

  <!-- Search -->
  <div class="toolbar">
    <div class="search-wrap">
      <Icon name="search" size={13} />
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
  </div>

  <!-- Body -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="page-body" bind:this={pageBodyEl} class:is-dragging={dragging !== null}>
    {#if sidebarVisible}
      <div class="sidebar" style="width: {colWidths.sidebar}%">
        <NoteFilters
          notes={notesStore.list}
          allTags={notesStore.allTags}
          folders={notesStore.folders}
          {activeFilter}
          onfilter={handleFilterChange}
        />
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
        <button class="btn-new" onclick={() => (showCreate = true)}>
          <Icon name="plus" size={12} /> {$t('notes_btn_new')}
        </button>
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
  /* width / centering / height come from global .page + .page--fill */
  .notes-page {
    overflow: hidden;
    gap: 0.875rem;
  }

  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }

  h1 { font-size: var(--fs-xl); font-weight: 700; letter-spacing: -0.02em; }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.2rem;
  }

  .icon-btn {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-2);
    display: flex;
    align-items: center;
    padding: var(--sp-1);
    border-radius: var(--radius-sm);
    transition: all 0.15s;
  }

  .icon-btn:hover { color: var(--text); background: var(--surface); }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-shrink: 0;
  }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
    width: 260px;
    flex-shrink: 0;
  }

  .search-wrap :global(svg) {
    position: absolute;
    left: 0.55rem;
    color: var(--text-3);
    pointer-events: none;
  }

  .search-input {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    width: 100%;
    padding: 0 1.75rem 0 var(--sp-8);
    font-size: var(--fs-sm);
    height: 32px;
    outline: none;
    transition: border-color 0.15s;
  }

  .search-input:focus { border-color: var(--accent); }

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
    border-radius: var(--radius);
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
    height: 40px;
    padding: 0 var(--sp-2);
    display: flex;
    align-items: center;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }

  .btn-new {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    margin-left: auto;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-1) 0.6rem;
    font-size: var(--fs-sm);
    color: var(--text-2);
    cursor: pointer;
    transition: all 0.15s;
  }

  .btn-new:hover { border-color: var(--accent); color: var(--accent); }

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
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .create-card {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
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
