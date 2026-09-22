<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import Icon from '$lib/Icon.svelte';
  import NotesNav from '$lib/components/mobile/NotesNav.svelte';
  import NoteRow from '$lib/components/mobile/NoteRow.svelte';
  import BottomSheet from '$lib/components/mobile/BottomSheet.svelte';
  import MoveFolderSheet from '$lib/components/mobile/MoveFolderSheet.svelte';
  import {
    api,
    formatError,
    type NavChild,
    type NoteListFilter,
    type NoteListItem,
    type NoteNav,
  } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import type { RowAction, RowMode } from '$lib/mobile/notes-editor';

  let notes = $state<NoteListItem[]>([]);
  let nav = $state<NoteNav | null>(null);
  let search = $state('');
  let searchOpen = $state(false);
  let menuOpen = $state(false);
  let error = $state('');
  let searchTimer: ReturnType<typeof setTimeout>;
  let busy = $state(false);

  // Filter lives in the URL so back navigation works.
  const filter = $derived<NoteListFilter>({
    kind: page.url.searchParams.get('kind') || 'all',
    id: page.url.searchParams.get('id') ?? undefined,
  });
  const rowMode = $derived<RowMode>(filter.kind === 'trash' ? 'trash' : filter.kind === 'archived' ? 'archived' : 'normal');
  const isMain = $derived(['all', 'pinned', 'archived'].includes(filter.kind));
  const folder = $derived<NavChild | undefined>(filter.kind === 'folder' ? nav?.folders.find((f) => f.id === filter.id) : undefined);

  // Sheets
  let sheet = $state<'none' | 'more' | 'quick' | 'move'>('none');
  let quickNote = $state<NoteListItem | null>(null);
  let moveTargets = $state<string[]>([]);

  // Multi-select
  let selecting = $state(false);
  let selected = $state<Set<string>>(new Set());

  // Pull to refresh
  let pull = $state(0);
  let refreshing = $state(false);
  let pullStartY = 0;
  let pulling = false;

  function title(): string {
    if (!nav) return $t('app_notes');
    const id = filter.id ?? '';
    switch (filter.kind) {
      case 'archived': return $t('notes_filter_archived');
      case 'trash': return $t('notes_filter_trash');
      case 'global': return $t('notes_filter_global');
      case 'workspace': return nav.workspaces.find((w) => w.id === id)?.name ?? $t('app_notes');
      case 'profile':
        for (const w of nav.workspaces) {
          const p = (w.profiles ?? []).find((x) => x.id === id);
          if (p) return p.name;
        }
        return $t('app_notes');
      case 'folder': return folder?.name ?? $t('app_notes');
      case 'smart': return nav.smart.find((s) => s.id === id)?.name ?? $t('app_notes');
      case 'domain':
      case 'tag': return id || $t('app_notes');
      default: return $t('app_notes');
    }
  }

  function countLabel(n: number): string {
    const key = n % 10 === 1 && n % 100 !== 11 ? 'notes_count_one' : 'notes_count_many';
    return $t(key, { n: String(n) });
  }

  async function load() {
    try {
      [notes, nav] = await Promise.all([api.notes.list(search, filter), api.notes.nav()]);
    } catch (e) {
      error = formatError(e);
    }
  }

  // Reload whenever the URL filter changes.
  let seen = '';
  $effect(() => {
    const key = `${filter.kind}:${filter.id ?? ''}`;
    if (key === seen) return;
    seen = key;
    exitSelect();
    void load();
  });

  function go(next: NoteListFilter) {
    const p = new URLSearchParams();
    if (next.kind !== 'all') p.set('kind', next.kind);
    if (next.id) p.set('id', next.id);
    const qs = p.toString();
    void goto(qs ? `/notes?${qs}` : '/notes');
  }

  function onSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(load, 200);
  }

  function closeSearch() {
    searchOpen = false;
    if (search) {
      search = '';
      void load();
    }
  }

  // ── Actions ──

  async function run(fn: () => Promise<unknown>) {
    busy = true;
    error = '';
    try {
      await fn();
      await load();
    } catch (e) {
      error = formatError(e);
    } finally {
      busy = false;
    }
  }

  function apply(action: RowAction, ids: string[]) {
    const one = async (id: string) => {
      switch (action) {
        case 'pin': {
          const n = notes.find((x) => x.id === id);
          return api.notes.update(id, { pinned: !n?.pinned });
        }
        case 'archive': return api.notes.update(id, { archived: true });
        case 'unarchive': return api.notes.update(id, { archived: false });
        case 'delete': return api.notes.delete(id);
        case 'restore': return api.notes.restore(id);
        case 'purge': return api.notes.purge(id);
      }
    };
    return run(async () => {
      for (const id of ids) await one(id);
    });
  }

  function onAction(action: RowAction, note: NoteListItem) {
    if (action === 'purge' && !confirm($t('notes_purge_confirm', { title: note.title || $t('notes_untitled') }))) return;
    void apply(action, [note.id]);
  }

  function quickAct(action: RowAction) {
    const n = quickNote;
    sheet = 'none';
    if (n) onAction(action, n);
  }

  function openMove(ids: string[]) {
    moveTargets = ids;
    sheet = 'move';
  }

  const moveCurrent = $derived.by((): string | null => {
    if (moveTargets.length !== 1) return null;
    const n = notes.find((x) => x.id === moveTargets[0]);
    return n?.chips.find((c) => c.kind === 'folder')?.id ?? null;
  });

  async function doMove(folderId: string | null) {
    const ids = moveTargets;
    sheet = 'none';
    await run(async () => {
      for (const id of ids) await api.notes.setFolder(id, folderId);
    });
    exitSelect();
  }

  async function emptyTrash() {
    if (!confirm($t('notes_trash_empty_confirm'))) return;
    await run(() => api.notes.emptyTrash());
  }

  // ── Multi-select ──

  function startSelect(note?: NoteListItem) {
    selecting = true;
    selected = new Set(note ? [note.id] : []);
  }
  function toggleSelect(note: NoteListItem) {
    const next = new Set(selected);
    if (next.has(note.id)) next.delete(note.id);
    else next.add(note.id);
    selected = next;
  }
  function exitSelect() {
    selecting = false;
    selected = new Set();
  }
  async function bulk(action: RowAction) {
    const ids = [...selected];
    if (!ids.length) return;
    if (action === 'purge' && !confirm($t('notes_trash_empty_confirm'))) return;
    await apply(action, ids);
    exitSelect();
  }

  // ── Pull to refresh ──

  function scroller(el: EventTarget | null): HTMLElement | null {
    return (el as HTMLElement | null)?.closest('.m-body') ?? null;
  }
  function onPullStart(e: TouchEvent) {
    const s = scroller(e.currentTarget);
    pulling = !!s && s.scrollTop <= 0 && !refreshing;
    pullStartY = e.touches[0].clientY;
  }
  function onPullMove(e: TouchEvent) {
    if (!pulling) return;
    const dy = e.touches[0].clientY - pullStartY;
    pull = dy > 0 ? Math.min(dy * 0.5, 80) : 0;
  }
  async function onPullEnd() {
    if (!pulling) return;
    pulling = false;
    if (pull < 60) {
      pull = 0;
      return;
    }
    pull = 48;
    refreshing = true;
    await load();
    refreshing = false;
    pull = 0;
  }

</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="m-page list" ontouchstart={onPullStart} ontouchmove={onPullMove} ontouchend={onPullEnd} ontouchcancel={onPullEnd}>
  <div class="pull" style:height="{pull}px">
    <span class:spin={refreshing}><Icon name="refresh-cw" size={18} /></span>
  </div>

  {#if selecting}
    <div class="m-header">
      <button class="m-ibtn" onclick={exitSelect} aria-label={$t('common_cancel')}><Icon name="x" size={22} /></button>
      <h1 class="m-title">{$t('notes_selected', { n: String(selected.size) })}</h1>
    </div>
  {:else}
    <div class="m-header">
      {#if isMain}
        <button class="m-ibtn" onclick={() => (menuOpen = true)} aria-label={$t('notes_nav_open')}><Icon name="menu" size={22} /></button>
      {:else}
        <a class="m-ibtn" href="/notes" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
      {/if}
      {#if folder}
        <span class="m-doc" style:--c={folder.color}><Icon name="folder" size={16} /></span>
      {/if}
      <h1 class="m-title">{title()}</h1>
      <button class="m-ibtn" class:on={searchOpen} onclick={() => (searchOpen = !searchOpen)} aria-label={$t('nav_search')}><Icon name="search" size={22} /></button>
      <button class="m-ibtn" onclick={() => (sheet = 'more')} aria-label={$t('nav_more')}><Icon name="more-vertical" size={22} /></button>
    </div>
    {#if searchOpen}
      <div class="m-search">
        <div class="field">
          <Icon name="search" size={16} />
          <input type="search" bind:value={search} oninput={onSearch} placeholder={$t('notes_search_placeholder')} />
          {#if search}<button type="button" class="clear" onclick={() => { search = ''; load(); }} aria-label="clear"><Icon name="x" size={16} /></button>{/if}
        </div>
        <button type="button" class="cancel" onclick={closeSearch}>{$t('common_cancel')}</button>
      </div>
    {/if}
  {/if}

  {#if isMain && !selecting}
    <div class="m-seg">
      <button type="button" class="m-seg-btn" class:active={filter.kind === 'all'} onclick={() => go({ kind: 'all' })}>{$t('notes_all')}</button>
      <button type="button" class="m-seg-btn" class:active={filter.kind === 'pinned'} onclick={() => go({ kind: 'pinned' })}>{$t('notes_filter_pinned')}</button>
      <button type="button" class="m-seg-btn" class:active={filter.kind === 'archived'} onclick={() => go({ kind: 'archived' })}>{$t('notes_filter_archived')}</button>
    </div>
  {/if}

  <div class="m-body">
  {#if filter.kind === 'trash'}
    <p class="m-hint">{$t('notes_trash_hint')}</p>
  {:else if !selecting && !isMain}
    <p class="m-sub count">{countLabel(notes.length)}</p>
  {/if}

  {#if error}
    <div class="m-error">{error}</div>
  {/if}

  {#if notes.length === 0}
    <div class="m-empty">
      <Icon name={filter.kind === 'trash' ? 'trash-2' : 'file-text'} size={40} />
      <p>{search.trim() || filter.kind !== 'all' ? $t('common_nothing_found') : $t('notes_list_empty')}</p>
    </div>
  {:else}
    <div class="m-cards">
      {#each notes as n (n.id)}
        <NoteRow
          note={n}
          mode={rowMode}
          {selecting}
          selected={selected.has(n.id)}
          onopen={(x) => goto(`/notes/${x.id}`)}
          ontoggle={toggleSelect}
          onlongpress={(x) => startSelect(x)}
          onquick={(x) => { quickNote = x; sheet = 'quick'; }}
          onaction={onAction}
        />
      {/each}
    </div>
  {/if}

  {#if filter.kind === 'trash' && notes.length}
    <button type="button" class="empty-trash" disabled={busy} onclick={emptyTrash}>
      {$t('notes_trash_empty_btn')}
    </button>
  {/if}
  </div>

</div>

{#if selecting}
  <div class="bulk">
    {#if rowMode === 'trash'}
      <button type="button" onclick={() => bulk('restore')}><Icon name="rotate-ccw" size={20} /><span>{$t('notes_restore')}</span></button>
      <button type="button" class="danger" onclick={() => bulk('purge')}><Icon name="trash-2" size={20} /><span>{$t('notes_purge')}</span></button>
    {:else}
      <button type="button" onclick={() => openMove([...selected])}><Icon name="folder" size={20} /><span>{$t('notes_move_btn')}</span></button>
      {#if rowMode === 'archived'}
        <button type="button" onclick={() => bulk('unarchive')}><Icon name="archive-restore" size={20} /><span>{$t('notes_unarchive')}</span></button>
      {:else}
        <button type="button" onclick={() => bulk('archive')}><Icon name="archive" size={20} /><span>{$t('notes_archive')}</span></button>
      {/if}
      <button type="button" class="danger" onclick={() => bulk('delete')}><Icon name="trash-2" size={20} /><span>{$t('notes_delete')}</span></button>
    {/if}
  </div>
{/if}

<NotesNav open={menuOpen} {nav} {filter} onclose={() => (menuOpen = false)} onselect={go} onchange={load} />

<BottomSheet open={sheet === 'more'} title={title()} onclose={() => (sheet = 'none')}>
  <div class="m-list">
    <button type="button" class="m-row" disabled={!notes.length} onclick={() => { sheet = 'none'; startSelect(); }}>
      <Icon name="check-square" size={20} /><span class="m-row-label">{$t('notes_select')}</span>
    </button>
  </div>
</BottomSheet>

<BottomSheet open={sheet === 'quick' && !!quickNote} title={quickNote?.title || $t('notes_untitled')} onclose={() => (sheet = 'none')}>
  <div class="m-list">
    {#if rowMode === 'trash'}
      <button type="button" class="m-row" onclick={() => quickAct('restore')}><Icon name="rotate-ccw" size={20} /><span class="m-row-label">{$t('notes_restore')}</span></button>
      <button type="button" class="m-row" onclick={() => quickAct('purge')}><Icon name="trash-2" size={20} /><span class="m-row-label danger-text">{$t('notes_purge')}</span></button>
    {:else}
      <button type="button" class="m-row" onclick={() => quickAct('pin')}><Icon name="pin" size={20} /><span class="m-row-label">{quickNote?.pinned ? $t('notes_unpin') : $t('notes_pin')}</span></button>
      <button type="button" class="m-row" onclick={() => { const id = quickNote!.id; sheet = 'none'; openMove([id]); }}><Icon name="folder" size={20} /><span class="m-row-label">{$t('notes_move')}</span></button>
      {#if rowMode === 'archived'}
        <button type="button" class="m-row" onclick={() => quickAct('unarchive')}><Icon name="archive-restore" size={20} /><span class="m-row-label">{$t('notes_unarchive')}</span></button>
      {:else}
        <button type="button" class="m-row" onclick={() => quickAct('archive')}><Icon name="archive" size={20} /><span class="m-row-label">{$t('notes_archive')}</span></button>
      {/if}
      <button type="button" class="m-row" onclick={() => quickAct('delete')}><Icon name="trash-2" size={20} /><span class="m-row-label danger-text">{$t('notes_delete')}</span></button>
    {/if}
  </div>
</BottomSheet>

<MoveFolderSheet
  open={sheet === 'move'}
  folders={nav?.folders ?? []}
  current={moveCurrent}
  {busy}
  onclose={() => (sheet = 'none')}
  onmove={doMove}
/>

<style>
  .list { min-height: 0; }
  .pull {
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    color: var(--text-3);
    transition: height var(--dur-fast);
  }
  .pull .spin { animation: spin 1s linear infinite; }
  .pull span { display: inline-flex; }
  .count { margin: 0 var(--sp-1) var(--sp-3); font-size: 13px; }
  .empty-trash {
    margin-top: auto;
    min-height: 48px;
    background: var(--m-card);
    border: 1px solid var(--m-card-border);
    border-radius: var(--m-radius);
    box-shadow: var(--m-card-shadow);
    color: var(--danger-text);
    font-size: 15px;
    font-weight: 600;
  }
  .m-cards { padding-bottom: var(--sp-6); }
  .danger-text { color: var(--danger-text); }
  .bulk {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 30;
    display: flex;
    background: var(--m-nav);
    border-top: 1px solid var(--border);
    height: calc(var(--nav-h) + var(--sab));
    padding-bottom: var(--sab);
  }
  .bulk button {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text);
    font-size: 11px;
    font-weight: 600;
  }
  .bulk button.danger { color: var(--danger-text); }
</style>
