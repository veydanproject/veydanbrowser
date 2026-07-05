<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import type { Profile, Proxy, WorkspaceColumn } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import ProfileSidePanel from './ProfileSidePanel.svelte';
  import { totpStore } from '$lib/store/totp.svelte';

  interface Props {
    profiles: Profile[];
    proxies: Proxy[];
    workspaceId: string;
    columns: WorkspaceColumn[];
    runningProfiles: Set<string>;
    onprofilechange: () => void;
    oncolumnschange: () => void;
    onedit: (p: Profile) => void;
    onrawdata?: (p: Profile) => void;
  }

  let {
    profiles = $bindable(),
    proxies,
    workspaceId,
    columns = $bindable(),
    runningProfiles = $bindable(),
    onprofilechange,
    oncolumnschange,
    onedit,
    onrawdata,
  }: Props = $props();

  // ── Filters ──────────────────────────────────────────────────────────────────
  let search = $state('');
  let filterProxy = $state('');
  let filterRuntime = $state('');

  // ── Drag & Drop (pointer events) ─────────────────────────────────────────────
  let dragId = $state<string | null>(null);
  let dragPos = $state<{ x: number; y: number } | null>(null);
  let dragStartPos: { x: number; y: number } | null = null;
  let dragOver = $state<string | null>(null);

  // ── Panel ─────────────────────────────────────────────────────────────────────
  let selectedProfile = $state<Profile | null>(null);

  $effect(() => {
    if (selectedProfile) {
      const fresh = profiles.find(p => p.id === selectedProfile!.id);
      if (fresh && fresh !== selectedProfile) selectedProfile = fresh;
    }
  });

  // ── Add column modal ─────────────────────────────────────────────────────────
  let showAddColumn = $state(false);
  let newColName = $state('');
  let newColColor = $state('#6366f1');
  let addColLoading = $state(false);

  // ── Edit column ───────────────────────────────────────────────────────────────
  let editingCol = $state<WorkspaceColumn | null>(null);
  let editColName = $state('');
  let editColColor = $state('');

  const PALETTE = ['#6366f1', '#f97316', '#22c55e', '#ef4444', '#eab308', '#06b6d4', '#ec4899', '#8b5cf6'];

  const proxyMap = $derived(new Map(proxies.map((p) => [p.id, p])));
  const sortedCols = $derived([...columns].sort((a, b) => a.position - b.position));
  const colTagSet = $derived(new Set(columns.map((c) => c.tag_name)));

  function filterProfile(p: Profile): boolean {
    if (search && !p.name.toLowerCase().includes(search.toLowerCase())) return false;
    if (filterProxy && p.proxy_id !== filterProxy) return false;
    if (filterRuntime === 'running' && !runningProfiles.has(p.id)) return false;
    if (filterRuntime === 'stopped' && runningProfiles.has(p.id)) return false;
    return true;
  }

  function colProfiles(tagName: string): Profile[] {
    return profiles
      .filter((p) => {
        const tags: string[] = p.tags ?? [];
        return tags.includes(tagName) && filterProfile(p);
      })
      .sort((a, b) => a.kanban_order - b.kanban_order);
  }

  function unassignedProfiles(): Profile[] {
    return profiles
      .filter((p) => {
        if (!filterProfile(p)) return false;
        const tags: string[] = p.tags ?? [];
        return !tags.some((t) => colTagSet.has(t));
      })
      .sort((a, b) => a.kanban_order - b.kanban_order);
  }

  // ── Pointer-based Drag & Drop ─────────────────────────────────────────────────

  function onCardPointerDown(e: PointerEvent, profile: Profile) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    dragId = profile.id;
    dragPos = { x: e.clientX, y: e.clientY };
    dragStartPos = { x: e.clientX, y: e.clientY };
    dragOver = null;
  }

  function onCardPointerMove(e: PointerEvent) {
    if (!dragId) return;
    dragPos = { x: e.clientX, y: e.clientY };

    // Find the column element under cursor by checking all elements at this point
    const els = document.elementsFromPoint(e.clientX, e.clientY);
    const colEl = els.find((el) => (el as HTMLElement).dataset.colTag !== undefined);
    dragOver = colEl ? (colEl as HTMLElement).dataset.colTag ?? null : null;
  }

  async function onCardPointerUp(e: PointerEvent, profile: Profile) {
    if (!dragId) return;

    const id = dragId;
    const targetTag = dragOver;
    const moved = dragStartPos
      ? Math.abs(e.clientX - dragStartPos.x) > 5 || Math.abs(e.clientY - dragStartPos.y) > 5
      : false;

    dragId = null;
    dragPos = null;
    dragStartPos = null;
    dragOver = null;

    if (!moved) {
      openPanel(profile);
      return;
    }

    if (!targetTag) return;

    const tags: string[] = profile.tags ?? [];
    if (targetTag !== 'unassigned' && tags.includes(targetTag)) return;

    const targetTag2 = targetTag === 'unassigned' ? '' : targetTag;
    const targetProfiles = targetTag === 'unassigned' ? unassignedProfiles() : colProfiles(targetTag);
    const newOrder = targetProfiles.length > 0
      ? Math.max(...targetProfiles.map((p) => p.kanban_order)) + 1
      : 0;

    const newTags = tags.filter((t) => !colTagSet.has(t));
    if (targetTag2) newTags.push(targetTag2);

    profiles = profiles.map((p) =>
      p.id === id ? { ...p, tags: newTags, kanban_order: newOrder } : p
    );

    try {
      await api.profiles.moveToKanbanColumn(id, targetTag2, newOrder);
      onprofilechange();
    } catch {
      onprofilechange();
    }
  }

  // ── Add Column ────────────────────────────────────────────────────────────────
  async function addColumn() {
    if (!newColName.trim()) return;
    addColLoading = true;
    try {
      const slugged = newColName.trim().toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');
      const tag = slugged || `col-${Date.now()}`;
      const col = await api.workspaces.columns.create(workspaceId, {
        name: newColName.trim(),
        tag_name: tag,
        color: newColColor,
      });
      columns = [...columns, col];
      oncolumnschange();
      newColName = '';
      showAddColumn = false;
    } finally {
      addColLoading = false;
    }
  }

  // ── Edit Column ───────────────────────────────────────────────────────────────
  function startEditCol(col: WorkspaceColumn) {
    editingCol = col;
    editColName = col.name;
    editColColor = col.color;
  }

  async function saveEditCol() {
    if (!editingCol || !editColName.trim()) return;
    const updated = await api.workspaces.columns.update(editingCol.id, {
      name: editColName.trim(),
      color: editColColor,
    });
    columns = columns.map((c) => (c.id === updated.id ? updated : c));
    editingCol = null;
    oncolumnschange();
  }

  async function deleteCol(col: WorkspaceColumn) {
    await api.workspaces.columns.delete(col.id);
    columns = columns.filter((c) => c.id !== col.id);
    oncolumnschange();
  }

  // ── Side Panel ────────────────────────────────────────────────────────────────
  function openPanel(p: Profile) { selectedProfile = p; }
  function onPanelClose() { selectedProfile = null; }
  function onPanelChange() { onprofilechange(); selectedProfile = null; }
  function syncProfile(updated: Profile) {
    profiles = profiles.map((p) => (p.id === updated.id ? updated : p));
    selectedProfile = updated;
    if (updated.status === 'running') {
      runningProfiles = new Set([...runningProfiles, updated.id]);
    } else {
      const next = new Set(runningProfiles);
      next.delete(updated.id);
      runningProfiles = next;
    }
  }

  const isSelectedRunning = $derived(
    selectedProfile ? runningProfiles.has(selectedProfile.id) : false
  );

  function getProxy(proxyId: string | null) {
    return proxyId ? (proxyMap.get(proxyId) ?? null) : null;
  }

  const dragGhostLabel = $derived(
    dragId ? (profiles.find((p) => p.id === dragId)?.name ?? '') : ''
  );
</script>

<div class="kanban-wrap">
  <!-- Filters -->
  <div class="filter-bar">
    <div class="search-field">
      <Icon name="search" size={13} />
      <input type="text" placeholder={$t('kanban_filter_search')} bind:value={search} />
      {#if search}
        <button class="search-clear" onclick={() => (search = '')}><Icon name="x" size={11} /></button>
      {/if}
    </div>
    <select class="filter-select" bind:value={filterProxy}>
      <option value="">{$t('kanban_filter_proxy')}</option>
      {#each proxies as pr}
        <option value={pr.id}>{pr.name}</option>
      {/each}
    </select>
    <select class="filter-select" bind:value={filterRuntime}>
      <option value="">{$t('kanban_filter_runtime')}</option>
      <option value="running">{$t('kanban_filter_running')}</option>
      <option value="stopped">{$t('kanban_filter_stopped')}</option>
    </select>
    {#if search || filterProxy || filterRuntime}
      <button class="filter-clear" onclick={() => { search = ''; filterProxy = ''; filterRuntime = ''; }} title="Clear filters">
        <Icon name="x" size={12} />
      </button>
    {/if}
    <span class="count-badge">{profiles.filter(filterProfile).length} / {profiles.length}</span>
  </div>

  <!-- Board -->
  <div class="board" style="grid-template-columns: repeat({sortedCols.length + 1 + (columns.length < 20 ? 1 : 0)}, minmax(200px, 1fr))">
    <!-- Dynamic columns -->
    {#each sortedCols as col (col.id)}
      {@const cards = colProfiles(col.tag_name)}
      <div
        class="column"
        class:drop-target={dragOver === col.tag_name}
        data-col-tag={col.tag_name}
        role="list"
      >
        <div class="col-header">
          <span class="col-dot" style="background:{col.color}"></span>
          {#if editingCol?.id === col.id}
            <input
              class="col-edit-input"
              bind:value={editColName}
              onblur={saveEditCol}
              onkeydown={(e) => e.key === 'Enter' && saveEditCol()}
            />
            <div class="col-color-row">
              {#each PALETTE as c}
                <button
                  class="swatch"
                  class:active={editColColor === c}
                  style="background:{c}"
                  onclick={() => (editColColor = c)}
                  aria-label={c}
                ></button>
              {/each}
            </div>
          {:else}
            <span class="col-title">{col.name}</span>
            <span class="col-count">{cards.length}</span>
            <button class="icon-btn" onclick={() => startEditCol(col)} title="Edit column">
              <Icon name="edit" size={11} />
            </button>
            <button class="icon-btn danger" onclick={() => deleteCol(col)} title="Delete column">
              <Icon name="trash" size={11} />
            </button>
          {/if}
        </div>

        <div class="col-cards">
          {#each cards as profile (profile.id)}
            {@const proxy = getProxy(profile.proxy_id)}
            {@const totpCount = totpStore.countForProfile(profile.id)}
            <div
              class="card"
              class:card-running={runningProfiles.has(profile.id)}
              class:dragging={dragId === profile.id}
              onpointerdown={(e) => onCardPointerDown(e, profile)}
              onpointermove={onCardPointerMove}
              onpointerup={(e) => onCardPointerUp(e, profile)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === 'Enter' && openPanel(profile)}
            >
              <div class="card-drag"><Icon name="grip-vertical" size={12} /></div>
              <div class="card-content">
                <div class="card-top-row">
                  <span class="card-name">{profile.name}</span>
                  {#if totpCount > 0}
                    <span class="totp-badge" title="TOTP codes">{totpCount}</span>
                  {/if}
                  {#if runningProfiles.has(profile.id)}
                    <span class="running-dot"></span>
                  {/if}
                </div>
                <div class="card-meta">
                  <span class="meta-item"><Icon name="monitor" size={10} />{profile.fingerprint_preset}</span>
                  {#if proxy}
                    <span class="meta-item accent"><Icon name="globe" size={10} />{proxy.country ?? proxy.name}</span>
                  {:else}
                    <span class="meta-item muted"><Icon name="wifi-off" size={10} />no proxy</span>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/each}

    <!-- Unassigned column (always shown) -->
    {#if columns.length > 0}
      {@const cards = unassignedProfiles()}
      <div
        class="column unassigned"
        class:drop-target={dragOver === 'unassigned'}
        data-col-tag="unassigned"
        role="list"
      >
        <div class="col-header">
          <span class="col-dot" style="background:#94a3b8"></span>
          <span class="col-title">Unassigned</span>
          <span class="col-count">{cards.length}</span>
        </div>
        <div class="col-cards">
          {#each cards as profile (profile.id)}
            {@const proxy = getProxy(profile.proxy_id)}
            {@const totpCount = totpStore.countForProfile(profile.id)}
            <div
              class="card"
              class:card-running={runningProfiles.has(profile.id)}
              class:dragging={dragId === profile.id}
              onpointerdown={(e) => onCardPointerDown(e, profile)}
              onpointermove={onCardPointerMove}
              onpointerup={(e) => onCardPointerUp(e, profile)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === 'Enter' && openPanel(profile)}
            >
              <div class="card-drag"><Icon name="grip-vertical" size={12} /></div>
              <div class="card-content">
                <div class="card-top-row">
                  <span class="card-name">{profile.name}</span>
                  {#if totpCount > 0}
                    <span class="totp-badge" title="TOTP codes">{totpCount}</span>
                  {/if}
                  {#if runningProfiles.has(profile.id)}
                    <span class="running-dot"></span>
                  {/if}
                </div>
                <div class="card-meta">
                  <span class="meta-item"><Icon name="monitor" size={10} />{profile.fingerprint_preset}</span>
                  {#if proxy}
                    <span class="meta-item accent"><Icon name="globe" size={10} />{proxy.country ?? proxy.name}</span>
                  {:else}
                    <span class="meta-item muted"><Icon name="wifi-off" size={10} />no proxy</span>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <!-- When no columns exist show all profiles in one column -->
    {#if columns.length === 0}
      <div class="column" role="list">
        <div class="col-header">
          <span class="col-dot" style="background:#94a3b8"></span>
          <span class="col-title">All Profiles</span>
          <span class="col-count">{profiles.filter(filterProfile).length}</span>
        </div>
        <div class="col-cards">
          {#each profiles.filter(filterProfile).sort((a,b)=>a.kanban_order-b.kanban_order) as profile (profile.id)}
            {@const proxy = getProxy(profile.proxy_id)}
            {@const totpCount = totpStore.countForProfile(profile.id)}
            <div
              class="card"
              class:card-running={runningProfiles.has(profile.id)}
              onclick={() => openPanel(profile)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === 'Enter' && openPanel(profile)}
            >
              <div class="card-content">
                <div class="card-top-row">
                  <span class="card-name">{profile.name}</span>
                  {#if totpCount > 0}
                    <span class="totp-badge" title="TOTP codes">{totpCount}</span>
                  {/if}
                  {#if runningProfiles.has(profile.id)}
                    <span class="running-dot"></span>
                  {/if}
                </div>
                <div class="card-meta">
                  <span class="meta-item"><Icon name="monitor" size={10} />{profile.fingerprint_preset}</span>
                  {#if proxy}
                    <span class="meta-item accent"><Icon name="globe" size={10} />{proxy.country ?? proxy.name}</span>
                  {:else}
                    <span class="meta-item muted"><Icon name="wifi-off" size={10} />no proxy</span>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Add Column button -->
    {#if !showAddColumn}
      <button class="add-col-btn" onclick={() => (showAddColumn = true)}>
        <Icon name="plus" size={16} />
        <span>Add column</span>
      </button>
    {:else}
      <div class="add-col-form">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="col-name-input"
          placeholder="Column name…"
          bind:value={newColName}
          onkeydown={(e) => e.key === 'Enter' && addColumn()}
          autofocus
        />
        <div class="palette-row">
          {#each PALETTE as c}
            <button
              class="swatch"
              class:active={newColColor === c}
              style="background:{c}"
              onclick={() => (newColColor = c)}
              aria-label={c}
            ></button>
          {/each}
        </div>
        <div class="add-col-actions">
          <button class="btn-primary" onclick={addColumn} disabled={addColLoading || !newColName.trim()}>
            {addColLoading ? '…' : 'Create'}
          </button>
          <button class="btn-ghost" onclick={() => { showAddColumn = false; newColName = ''; }}>Cancel</button>
        </div>
      </div>
    {/if}
  </div>
</div>

<!-- Drag ghost -->
{#if dragId && dragPos}
  <div class="drag-ghost" style="left:{dragPos.x}px;top:{dragPos.y}px;">
    <Icon name="grip-vertical" size={12} />
    {dragGhostLabel}
  </div>
{/if}

{#if selectedProfile}
  <ProfileSidePanel
    profile={selectedProfile}
    proxy={getProxy(selectedProfile.proxy_id)}
    {workspaceId}
    {columns}
    isRunning={isSelectedRunning}
    onclose={onPanelClose}
    onchange={onPanelChange}
    onsync={syncProfile}
    {onedit}
    onrawdata={(p) => { selectedProfile = null; onrawdata?.(p); }}
  />
{/if}

<style>
  .kanban-wrap { display: flex; flex-direction: column; gap: var(--sp-3); height: 100%; overflow: hidden; }

  /* панель фильтров — единые примитивы .filter-bar/.search-field/.filter-select/.filter-clear/.count-badge из base.css */

  .board {
    display: grid;
    gap: var(--sp-3);
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    padding-bottom: var(--sp-1);
    align-items: start;
  }

  .column {
    background: var(--bg2); border: 1px solid var(--border); border-radius: 8px;
    display: flex; flex-direction: column; min-height: 120px; max-height: 100%;
    transition: border-color 0.15s, background 0.15s;
  }
  .column.drop-target { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, var(--bg2)); }
  .column.unassigned { opacity: 0.7; }

  .col-header {
    display: flex; align-items: center; gap: 0.4rem;
    padding: 0.55rem 0.7rem; border-bottom: 1px solid var(--border);
    min-height: 38px; flex-wrap: wrap;
  }
  .col-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
  .col-title { font-size: var(--fs-2xs); font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-muted); flex: 1; }
  .col-count { font-size: var(--fs-2xs); background: var(--bg3); border: 1px solid var(--border); padding: 0 0.35rem; border-radius: 999px; color: var(--text-muted); }

  .col-edit-input {
    flex: 1; background: var(--bg3); border: 1px solid var(--accent); border-radius: 4px;
    color: var(--text); font-size: var(--fs-sm); padding: 0.2rem 0.4rem; outline: none;
  }
  .col-color-row { display: flex; gap: 3px; flex-wrap: wrap; width: 100%; padding-top: var(--sp-1); }

  .col-cards {
    padding: var(--sp-2); display: flex; flex-direction: column; gap: 0.4rem;
    overflow-y: auto; flex: 1;
  }

  .card {
    background: var(--bg); border: 1px solid var(--border); border-radius: 6px;
    padding: 0.55rem 0.6rem; cursor: grab; transition: border-color 0.13s, box-shadow 0.13s, opacity 0.13s;
    display: flex; gap: 0.35rem; align-items: flex-start;
    user-select: none; touch-action: none;
  }
  .card:hover { border-color: var(--border-strong, var(--accent)); box-shadow: 0 1px 4px rgba(0,0,0,.12); }
  .card.card-running { border-color: color-mix(in srgb, #22c55e 35%, var(--border)); }
  .card.dragging { opacity: 0.35; cursor: grabbing; }

  .card-drag { color: var(--text-muted); padding-top: 2px; flex-shrink: 0; }
  .card-content { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 0.25rem; }
  .card-top-row { display: flex; align-items: center; justify-content: space-between; gap: 0.3rem; }
  .card-name { font-size: var(--fs-sm); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .running-dot { width: 7px; height: 7px; border-radius: 50%; background: #22c55e; flex-shrink: 0; }

  .totp-badge {
    background: var(--accent-bg);
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 999px;
    font-size: var(--fs-2xs);
    padding: 0.05rem 0.3rem;
    font-weight: 600;
    line-height: 1.4;
    flex-shrink: 0;
  }
  .card-meta { display: flex; flex-wrap: wrap; gap: 0.2rem; }
  .meta-item { display: inline-flex; align-items: center; gap: 0.2rem; font-size: var(--fs-2xs); color: var(--text-muted); }
  .meta-item.accent { color: var(--accent); }
  .meta-item.muted { opacity: 0.6; }

  /* Drag ghost */
  .drag-ghost {
    position: fixed;
    pointer-events: none;
    z-index: 9999;
    background: var(--bg);
    border: 1px solid var(--accent);
    border-radius: 6px;
    padding: 0.4rem 0.65rem;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text);
    box-shadow: 0 4px 14px rgba(0,0,0,0.2);
    transform: translate(12px, -50%);
    opacity: 0.92;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Add column */
  .add-col-btn {
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 0.4rem;
    min-height: 80px; border: 2px dashed var(--border); border-radius: 8px;
    background: none; cursor: pointer; color: var(--text-muted); font-size: var(--fs-sm);
    transition: border-color 0.15s, color 0.15s; padding: var(--sp-4);
  }
  .add-col-btn:hover { border-color: var(--accent); color: var(--accent); }

  .add-col-form {
    background: var(--bg2); border: 1px solid var(--border); border-radius: 8px;
    padding: var(--sp-3); display: flex; flex-direction: column; gap: var(--sp-2);
  }
  .col-name-input {
    background: var(--bg3); border: 1px solid var(--border); border-radius: 5px;
    color: var(--text); padding: 0.4rem 0.6rem; font-size: var(--fs-base); outline: none;
    width: 100%;
  }
  .col-name-input:focus { border-color: var(--accent); }
  .palette-row { display: flex; gap: 4px; flex-wrap: wrap; }
  .add-col-actions { display: flex; gap: 0.4rem; }

  .swatch {
    width: 18px; height: 18px; border-radius: 50%; border: 2px solid transparent; cursor: pointer;
    flex-shrink: 0; transition: border-color 0.1s, transform 0.1s;
  }
  .swatch.active { border-color: var(--text); transform: scale(1.15); }
  .swatch:hover { transform: scale(1.1); }

  .icon-btn {
    background: none; border: none; cursor: pointer; color: var(--text-muted);
    padding: 0.2rem; border-radius: 3px; display: inline-flex; align-items: center;
    opacity: 0;
    transition: opacity 0.15s, color 0.15s;
  }
  .col-header:hover .icon-btn { opacity: 1; }
  .icon-btn:hover { color: var(--text); background: var(--bg3); }
  .icon-btn.danger:hover { color: #ef4444; }

  .btn-primary {
    background: var(--accent); color: white; border: none; border-radius: 5px;
    padding: 0.35rem 0.7rem; font-size: var(--fs-sm); cursor: pointer;
  }
  .btn-primary:disabled { opacity: 0.6; cursor: not-allowed; }
  .btn-ghost {
    background: none; border: 1px solid var(--border); border-radius: 5px;
    color: var(--text-muted); padding: 0.35rem 0.7rem; font-size: var(--fs-sm); cursor: pointer;
  }
  .btn-ghost:hover { border-color: var(--text-muted); color: var(--text); }
</style>
