<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { portal } from '$lib/portal';
  import { notesStore } from '$lib/store/notes.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { api } from '$lib/api';
  import type { NoteCreateInput, NoteFilter } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import NotesList from './NotesList.svelte';
  import NoteEditor from './NoteEditor.svelte';
  import NoteFilters from './NoteFilters.svelte';
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
  let panelBodyEl: HTMLElement | null = $state(null);
  let dragging: 'sidebar' | 'list' | null = $state(null);
  let hoveredResizer: 'sidebar' | 'list' | null = $state(null);

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
    workspacesStore.ensureLoaded();
    profilesStore.ensureLoaded();
    notesStore.startWatcher();

    return () => { /* watcher stays alive across panel opens */ };
  });

  $effect(() => {
    if (open) {
      untrack(() => notesStore.ensureLoaded());
      // Pre-set filter based on context
      if (context === 'workspace' && contextId) {
        activeFilter = { type: 'workspace', id: contextId };
      } else if (context === 'profile' && contextId) {
        activeFilter = { type: 'profile', id: contextId };
      } else {
        activeFilter = { type: 'all' };
      }
      // Open specific note if requested
      if (openNoteId && untrack(() => notesStore.activeNoteId) !== openNoteId) {
        void notesStore.openNote(openNoteId);
      }
    }
  });

  function hasBinding(bindings: string[], b: string): boolean {
    return bindings.includes(b);
  }

  function isGlobal(bindings: string[]): boolean {
    return !bindings.some(b => b.startsWith('workspace:') || b.startsWith('profile:'));
  }

  // Build NoteFilter from activeFilter state (used for search)
  const noteFilter = $derived.by((): NoteFilter => {
    switch (activeFilter.type) {
      case 'workspace': return { binding: `workspace:${activeFilter.id}` };
      case 'profile': return { binding: `profile:${activeFilter.id}` };
      case 'tag': return { tag_name: activeFilter.id };
      case 'pinned': return { pinned: true };
      case 'archived': return { archived: true, include_deleted: false };
      default: return {};
    }
  });

  // Filtered + searched list
  const displayList = $derived.by(() => {
    let list = notesStore.list;

    // Apply activeFilter
    if (activeFilter.type !== 'all') {
      if (activeFilter.type === 'global') list = list.filter((n) => isGlobal(n.bindings) && !n.archived);
      else if (activeFilter.type === 'workspace') list = list.filter((n) => hasBinding(n.bindings, `workspace:${activeFilter.id}`) && !n.archived);
      else if (activeFilter.type === 'profile') list = list.filter((n) => hasBinding(n.bindings, `profile:${activeFilter.id}`) && !n.archived);
      else if (activeFilter.type === 'tag') list = list.filter((n) => n.tags.some((t) => t.name === activeFilter.id) && !n.archived);
      else if (activeFilter.type === 'tag-group') list = list.filter((n) => n.tags.some((t) => t.name === activeFilter.id || t.name.startsWith(activeFilter.id + '/')) && !n.archived);
      else if (activeFilter.type === 'folder') list = list.filter((n) => n.folder_ids.includes(activeFilter.id!) && !n.archived);
      else if (activeFilter.type === 'pinned') list = list.filter((n) => n.pinned && !n.archived);
      else if (activeFilter.type === 'archived') list = list.filter((n) => n.archived);
    } else {
      list = list.filter((n) => !n.archived);
    }

    // Локальный фильтр только для 1 символа (< 2 не вызывает API)
    if (searchQuery.trim().length === 1) {
      const q = searchQuery.toLowerCase();
      list = list.filter(
        (n) =>
          n.title.toLowerCase().includes(q) ||
          n.tags.some((t) => t.name.toLowerCase().includes(q))
      );
    }

    return list;
  });

  async function handleSearch() {
    if (!searchQuery.trim()) {
      await notesStore.refresh();
      return;
    }
    searching = true;
    try {
      const results = await api.notes.search(searchQuery, noteFilter);
      // Merge results into display (search replaces local filter)
      notesStore.list = results;
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

  async function handleFilterChange(f: { type: string; id?: string }) {
    activeFilter = f;
    searchQuery = '';
    await notesStore.refresh();
  }

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
      format: createFormat,
      bindings,
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
</script>

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
    <div class="panel" role="dialog" aria-label={$t('notes_title')}>
      <!-- Header -->
      <div class="panel-header">
        <div class="header-title">
          <Icon name="file-text" size={16} />
          <h3>{$t('notes_title')}</h3>
          <span class="badge badge-accent">{displayList.length}</span>
        </div>
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
          <button class="close-btn" onclick={() => (open = false)}>
            <Icon name="x" size={15} />
          </button>
        </div>
      </div>

      <!-- Search -->
      <div class="panel-search">
        <div class="search-wrap">
          <span class="search-icon"><Icon name="search" size={13} /></span>
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

      <!-- Body: sidebar + list + editor -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="panel-body" bind:this={panelBodyEl} class:is-dragging={dragging !== null}>
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
            <div class="create-actions">
              <button class="btn btn-ghost" onclick={() => (showCreate = false)}>{$t('notes_btn_cancel')}</button>
              <button class="btn btn-primary" onclick={createNote} disabled={!createTitle.trim()}>
                {$t('notes_btn_create')}
              </button>
            </div>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.3);
    z-index: 900;
    display: flex;
    justify-content: flex-end;
  }

  .panel {
    background: var(--bg-2);
    border-left: 1px solid var(--border);
    width: min(900px, 96vw);
    height: 100%;
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow-lg);
    animation: slide-in 0.2s ease;
    position: relative;
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
    font-size: var(--fs-md);
    font-weight: 600;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 0.2rem;
  }

  .icon-btn, .close-btn {
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

  .icon-btn:hover, .close-btn:hover { color: var(--text); background: var(--surface); }

  .panel-search {
    padding: 0.6rem var(--sp-4) 0;
    flex-shrink: 0;
  }

  .search-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .search-icon {
    position: absolute;
    left: 0.6rem;
    color: var(--text-2);
    pointer-events: none;
    display: flex;
  }

  .search-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.42rem 0.6rem 0.42rem var(--sp-8);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--fs-sm);
  }

  .search-input:focus { outline: none; border-color: var(--accent); }

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
    margin-top: var(--sp-2);
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
    padding: 0.4rem var(--sp-2);
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }

  .btn-new {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
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
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
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

  .create-title {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 0.45rem var(--sp-3);
    color: var(--text);
    font-size: var(--fs-base);
    width: 100%;
    box-sizing: border-box;
  }

  .create-title:focus { outline: none; border-color: var(--accent); }

  .create-actions {
    display: flex;
    gap: var(--sp-2);
    justify-content: flex-end;
  }
</style>
