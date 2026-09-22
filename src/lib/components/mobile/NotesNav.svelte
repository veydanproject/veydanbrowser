<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { api, formatError, type NoteListFilter, type NoteNav, type NavChild } from '$lib/mobile/api';
  import { NAV_COLORS } from '$lib/mobile/nav-colors';
  import { longpress } from '$lib/mobile/longpress';
  import { t } from '$lib/mobile/i18n';
  import NavEditSheet from './NavEditSheet.svelte';
  import SmartViewSheet from './SmartViewSheet.svelte';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    nav: NoteNav | null;
    filter: NoteListFilter;
    onclose: () => void;
    onselect: (filter: NoteListFilter) => void;
    onchange: () => void;
  }

  let { open, nav, filter, onclose, onselect, onchange }: Props = $props();
  let collapsed = $state<Set<string>>(new Set());
  let error = $state('');

  type Menu = { kind: 'folder' | 'smart'; id: string; name: string; color: string };
  let menu = $state<Menu | null>(null);

  let edit = $state<{ id: string | null; parentId?: string; title: string } | null>(null);
  let editName = $state('');
  let editColor = $state(NAV_COLORS[0]);
  let smartOpen = $state(false);
  let smartId = $state<string | null>(null);
  let saving = $state(false);

  type FolderNode = { item: NavChild; children: FolderNode[]; total: number };

  // Folder tree. Children nest under their parent.
  const spaces = $derived.by((): FolderNode[] => {
    const folders = nav?.folders ?? [];
    const map = new Map<string, FolderNode>();
    for (const f of folders) map.set(f.id, { item: f, children: [], total: f.count });
    const roots: FolderNode[] = [];
    for (const node of map.values()) {
      const pid = node.item.parent_id;
      if (pid && map.has(pid)) map.get(pid)!.children.push(node);
      else roots.push(node);
    }
    const rollup = (n: FolderNode): number => {
      n.total = n.item.count + n.children.reduce((s, c) => s + rollup(c), 0);
      return n.total;
    };
    roots.forEach(rollup);
    return roots;
  });

  const hasBrowser = $derived(!!nav && (nav.workspaces.length > 0 || nav.counts.global > 0));

  function swatch(color: string | undefined): string {
    const c = (color ?? '').trim();
    return c && c.toLowerCase() !== '#ffffff' && c.toLowerCase() !== '#fff' ? c : 'var(--accent)';
  }

  function toggle(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed = next;
  }

  function pick(kind: string, id?: string) {
    onselect({ kind, id });
    onclose();
  }

  function active(kind: string, id?: string) {
    return filter.kind === kind && (id ?? '') === (filter.id ?? '');
  }

  function openNewFolder(parentId?: string) {
    editName = '';
    editColor = NAV_COLORS[0];
    edit = { id: null, parentId, title: parentId ? $t('notes_folder_sub') : $t('notes_folder_new') };
  }
  function openNewSmart() {
    smartId = null;
    smartOpen = true;
  }

  function startEditFromMenu() {
    if (!menu) return;
    if (menu.kind === 'smart') {
      smartId = menu.id;
      smartOpen = true;
      menu = null;
      return;
    }
    editName = menu.name;
    editColor = menu.color || NAV_COLORS[0];
    edit = { id: menu.id, title: $t('notes_item_edit') };
    menu = null;
  }

  async function saveEdit(name: string, color: string) {
    if (!edit || saving) return;
    saving = true;
    error = '';
    try {
      if (edit.id) await api.notes.folderUpdate(edit.id, name, color);
      else {
        const f = await api.notes.folderCreate(name, edit.parentId, color);
        onselect({ kind: 'folder', id: f.id });
      }
      edit = null;
      onchange();
    } catch (e) {
      error = formatError(e);
    } finally {
      saving = false;
    }
  }

  async function removeCurrent() {
    if (!menu) return;
    if (!confirm($t('notes_delete_item_confirm', { name: menu.name }))) return;
    const { kind, id } = menu;
    menu = null;
    error = '';
    try {
      if (kind === 'folder') await api.notes.folderDelete(id);
      else await api.notes.smartViewDelete(id);
      if (filter.kind === kind && filter.id === id) onselect({ kind: 'all' });
      onchange();
    } catch (e) {
      error = formatError(e);
    }
  }
</script>

{#if open}
  <div class="m-dim" onclick={onclose} role="presentation"></div>
  <div class="drawer" role="dialog" aria-modal="true" aria-label={$t('app_notes')}>
    <div class="drawer-head">
      <span class="drawer-title">{$t('app_notes')}</span>
      <button class="m-ibtn" onclick={onclose} aria-label={$t('notes_nav_close')}>
        <Icon name="x" size={22} />
      </button>
    </div>

    {#if error}
      <div class="m-error pad">{error}</div>
    {/if}

    {#if nav}
      <nav class="body">
        <section>
          <button class="item" class:on={active('all')} onclick={() => pick('all')}>
            <Icon name="file-text" size={18} />
            <span class="name">{$t('notes_filter_all')}</span>
            <span class="n">{nav.counts.all}</span>
          </button>
          <button class="item" class:on={active('pinned')} onclick={() => pick('pinned')}>
            <Icon name="pin" size={18} />
            <span class="name">{$t('notes_filter_pinned')}</span>
            <span class="n">{nav.counts.pinned}</span>
          </button>
          <button class="item" class:on={active('archived')} onclick={() => pick('archived')}>
            <Icon name="archive" size={18} />
            <span class="name">{$t('notes_filter_archived')}</span>
            <span class="n">{nav.counts.archived}</span>
          </button>
          <button class="item" class:on={active('trash')} onclick={() => pick('trash')}>
            <Icon name="trash-2" size={18} />
            <span class="name">{$t('notes_filter_trash')}</span>
            <span class="n">{nav.counts.trash}</span>
          </button>
        </section>

        <section>
          <div class="m-label">{$t('notes_filter_folders')}</div>
          {#each spaces as node (node.item.id)}
            {@render space(node)}
          {/each}
          {#if !spaces.length}
            <span class="empty">{$t('notes_filter_folders_empty')}</span>
          {/if}
        </section>

        {#if hasBrowser}
          <section>
            <button type="button" class="label-row" onclick={() => toggle('browser')}>
              <span class="m-label">{$t('notes_browser_section')}</span>
              <Icon name={collapsed.has('browser') ? 'chevron-right' : 'chevron-down'} size={16} />
            </button>
            {#if !collapsed.has('browser')}
              {#if nav.counts.global > 0}
                <button class="item" class:on={active('global')} onclick={() => pick('global')}>
                  <Icon name="globe" size={18} />
                  <span class="name">{$t('notes_filter_global')}</span>
                  <span class="n">{nav.counts.global}</span>
                </button>
              {/if}
              {#each nav.workspaces as ws (ws.id)}
                <button class="item" class:on={active('workspace', ws.id)} onclick={() => pick('workspace', ws.id)}>
                  <span class="m-dot" style:background={swatch(ws.color)}></span>
                  <span class="name">{ws.name}</span>
                  {#if ws.count > 0}<span class="n">{ws.count}</span>{/if}
                </button>
                {#each ws.profiles ?? [] as p (p.id)}
                  <button class="item child" class:on={active('profile', p.id)} onclick={() => pick('profile', p.id)}>
                    <Icon name="browser" size={16} />
                    <span class="name">{p.name}</span>
                    <span class="n">{p.count}</span>
                  </button>
                {/each}
              {/each}
            {/if}
          </section>
        {/if}

        {#if nav.sites.length}
          <section>
            <div class="m-label">{$t('notes_filter_sites')}</div>
            {#each nav.sites as s (s.id)}
              <button class="item" class:on={active('domain', s.id)} onclick={() => pick('domain', s.id)}>
                <Icon name="globe" size={18} />
                <span class="name">{s.name}</span>
                <span class="n">{s.count}</span>
              </button>
            {/each}
          </section>
        {/if}

        <section>
          <div class="label-row">
            <span class="m-label">{$t('notes_smart_title')}</span>
            <button type="button" class="m-ibtn small" onclick={openNewSmart} aria-label={$t('notes_smart_new')}>
              <Icon name="plus" size={16} />
            </button>
          </div>
          {#each nav.smart as view (view.id)}
            <button
              class="item"
              class:on={active('smart', view.id)}
              onclick={() => pick('smart', view.id)}
              {@attach longpress(() => (menu = { kind: 'smart', id: view.id, name: view.name, color: view.color }))}
            >
              <span class="m-doc" style:--c={view.color}><Icon name="list" size={14} /></span>
              <span class="name">{view.name}</span>
            </button>
          {/each}
          {#if !nav.smart.length}
            <span class="empty">{$t('notes_smart_empty')}</span>
          {/if}
        </section>

        <button type="button" class="m-link create" onclick={() => openNewFolder()}>
          <Icon name="plus" size={18} /> {$t('notes_create_folder')}
        </button>
      </nav>
    {/if}
  </div>
{/if}

<BottomSheet open={!!menu} title={menu?.name ?? ''} onclose={() => (menu = null)}>
  <div class="m-list">
    <button type="button" class="m-row" onclick={startEditFromMenu}><Icon name="pencil" size={20} /><span class="m-row-label">{$t('notes_item_edit')}</span></button>
    {#if menu?.kind === 'folder' && !nav?.folders.find((f) => f.id === menu?.id)?.parent_id}
      <button type="button" class="m-row" onclick={() => { const id = menu!.id; menu = null; openNewFolder(id); }}><Icon name="folder-plus" size={20} /><span class="m-row-label">{$t('notes_folder_sub')}</span></button>
    {/if}
    <button type="button" class="m-row" onclick={removeCurrent}><Icon name="trash-2" size={20} /><span class="m-row-label danger">{$t('notes_tag_delete')}</span></button>
  </div>
</BottomSheet>

<NavEditSheet
  open={!!edit}
  title={edit?.title ?? ''}
  bind:name={editName}
  bind:color={editColor}
  placeholder={$t('notes_folder_name')}
  {saving}
  onclose={() => (edit = null)}
  onsave={saveEdit}
/>

{#if nav}
  <SmartViewSheet
    open={smartOpen}
    viewId={smartId}
    {nav}
    onclose={() => (smartOpen = false)}
    onsaved={(id) => { onselect({ kind: 'smart', id }); onchange(); }}
  />
{/if}

{#snippet space(node: FolderNode)}
  {@const f = node.item}
  <div class="space">
    <button
      class="item space-row"
      class:on={active('folder', f.id)}
      onclick={() => pick('folder', f.id)}
      {@attach longpress(() => (menu = { kind: 'folder', id: f.id, name: f.name, color: f.color }))}
    >
      <span class="m-dot" style:background={swatch(f.color)}></span>
      <span class="name">{f.name}</span>
      <span class="n">{node.total}</span>
    </button>
    {#each node.children as child (child.item.id)}
      {@render folderNode(child, 1, f.color)}
    {/each}
  </div>
{/snippet}

{#snippet folderNode(node: FolderNode, depth: number, spaceColor: string)}
  {@const f = node.item}
  <button
    class="item folder-row"
    class:on={active('folder', f.id)}
    style:padding-left="{12 + depth * 20}px"
    onclick={() => pick('folder', f.id)}
    {@attach longpress(() => (menu = { kind: 'folder', id: f.id, name: f.name, color: f.color }))}
  >
    <span class="folder-ico" style:--c={f.color || spaceColor}><Icon name="folder" size={18} /></span>
    <span class="name">{f.name}</span>
    <span class="n">{node.total}</span>
  </button>
  {#each node.children as child (child.item.id)}
    {@render folderNode(child, depth + 1, spaceColor)}
  {/each}
{/snippet}

<style>
  .drawer {
    position: fixed;
    top: 0;
    left: 0;
    bottom: 0;
    width: min(88vw, 340px);
    background: var(--m-sheet);
    z-index: 51;
    display: flex;
    flex-direction: column;
    padding-top: var(--sat);
    animation: slide var(--dur-drawer) var(--ease-drawer);
    border-radius: 0 24px 24px 0;
  }
  @keyframes slide {
    from { transform: translateX(-100%); }
    to { transform: translateX(0); }
  }
  .drawer-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--sp-3) var(--sp-2) var(--sp-2) var(--sp-4);
    flex-shrink: 0;
  }
  .drawer-title { font-weight: 800; font-size: 20px; letter-spacing: -0.02em; }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 var(--sp-3) calc(var(--nav-h) + var(--sab) + var(--sp-6));
  }
  section { display: flex; flex-direction: column; }
  section + section { margin-top: var(--sp-2); }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--touch);
    padding: 0 12px;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    font-weight: 500;
    text-align: left;
    width: 100%;
    border-radius: 12px;
  }
  .item > :global(svg) { color: var(--text-2); }
  .item:active { background: var(--surface-2); }
  .item.on { background: var(--accent-bg); color: var(--accent-text); }
  .item.on > :global(svg) { color: var(--accent); }
  .item.child { padding-left: 40px; }
  .space-row { font-weight: 700; }
  .folder-row { font-weight: 500; }
  .folder-ico {
    display: inline-flex;
    color: color-mix(in srgb, var(--c) 70%, var(--text-2));
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    margin-left: auto;
    color: var(--text-3);
    font-size: 13px;
    font-weight: 600;
    flex-shrink: 0;
  }
  .item.on .n { color: inherit; }
  .space { display: flex; flex-direction: column; }
  .empty {
    padding: var(--sp-2) var(--sp-3);
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    border: 0;
    background: none;
    padding: 0;
    text-align: left;
    cursor: pointer;
  }
  .label-row .m-label { flex: 1; }
  .label-row > :global(svg) { color: var(--text-3); flex-shrink: 0; margin-right: 10px; }
  .m-ibtn.small { width: 36px; height: 36px; color: var(--text-3); }
  .create { margin-top: var(--sp-3); }
  .pad { padding: 0 var(--sp-3); }
  .danger { color: var(--danger-text); }
</style>
