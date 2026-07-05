<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import type { Workspace, WorkspaceStats, CreateWorkspaceRequest } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Modal from '$lib/Modal.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { proxiesStore } from '$lib/store/proxies.svelte';
  import { formatError } from '$lib/utils';

  let workspaces = $state<Workspace[]>([]);
  let stats = $state<Record<string, WorkspaceStats>>({});
  let loading = $state(false);
  let error = $state('');

  let createModal = $state(false);
  let editModal = $state<{ open: boolean; id: string; name: string; description: string; color: string }>({
    open: false, id: '', name: '', description: '', color: '#6366f1',
  });
  let deleteModal = $state<{ open: boolean; id: string; name: string }>({
    open: false, id: '', name: '',
  });
  let createForm = $state<CreateWorkspaceRequest>({ name: '', description: '', color: '#6366f1' });
  let saving = $state(false);

  let dragSrcIndex = $state(-1);
  let insertAt = $state(-1);
  let ghostActive = $state(false);
  let ghostX = $state(0);
  let ghostY = $state(0);

  const ORDER_KEY = 'ws_order';

  const COLORS = [
    '#6366f1', '#8b5cf6', '#ec4899', '#f43f5e',
    '#f97316', '#eab308', '#22c55e', '#14b8a6',
    '#06b6d4', '#3b82f6',
  ];

  onMount(async () => {
    // Use cached data if available, then refresh stats
    if (workspacesStore.loaded) {
      workspaces = applyOrder(workspacesStore.list);
      await loadStats();
    } else {
      await loadData();
    }
  });

  function applyOrder(ws: Workspace[]): Workspace[] {
    try {
      const saved: string[] = JSON.parse(localStorage.getItem(ORDER_KEY) ?? '[]');
      if (saved.length === 0) return ws;
      const map = new Map(ws.map((w) => [w.id, w]));
      const ordered = saved.map((id) => map.get(id)).filter(Boolean) as Workspace[];
      const rest = ws.filter((w) => !saved.includes(w.id));
      return [...ordered, ...rest];
    } catch {
      return ws;
    }
  }

  function saveOrder() {
    localStorage.setItem(ORDER_KEY, JSON.stringify(workspaces.map((w) => w.id)));
  }

  function findInsertIndex(clientX: number, clientY: number): number {
    const cards = [...document.querySelectorAll('.workspace-card')] as HTMLElement[];
    if (cards.length === 0) return 0;
    let minDist = Infinity;
    let result = cards.length;
    for (let idx = 0; idx < cards.length; idx++) {
      const rect = cards[idx].getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const dist = Math.hypot(clientX - cx, clientY - cy);
      if (dist < minDist) {
        minDist = dist;
        result = clientX < cx ? idx : idx + 1;
      }
    }
    return result;
  }

  function onHandlePointerDown(e: PointerEvent, i: number) {
    e.preventDefault();
    dragSrcIndex = i;
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;

    function onMove(ev: PointerEvent) {
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) > 6) {
        moved = true;
        ghostActive = true;
        document.body.style.cursor = 'grabbing';
      }
      if (moved) {
        ghostX = ev.clientX;
        ghostY = ev.clientY;
        insertAt = findInsertIndex(ev.clientX, ev.clientY);
      }
    }

    function onUp() {
      document.removeEventListener('pointermove', onMove);
      document.removeEventListener('pointerup', onUp);
      document.body.style.cursor = '';

      if (moved && dragSrcIndex !== -1) {
        const from = dragSrcIndex;
        const to = insertAt;
        if (to !== -1 && from !== to && from !== to - 1) {
          const arr = [...workspaces];
          const [item] = arr.splice(from, 1);
          arr.splice(to > from ? to - 1 : to, 0, item);
          workspaces = arr;
          saveOrder();
        }
      }

      dragSrcIndex = -1;
      insertAt = -1;
      ghostActive = false;
    }

    document.addEventListener('pointermove', onMove);
    document.addEventListener('pointerup', onUp);
  }

  async function loadData() {
    loading = true;
    try {
      await workspacesStore.refresh();
      workspaces = applyOrder(workspacesStore.list);
      await loadStats();
    } catch (e) {
      error = formatError(e);
    } finally {
      loading = false;
    }
  }

  async function loadStats() {
    try {
      const statsArr = await Promise.all(workspaces.map((w) => api.workspaces.stats(w.id)));
      const map: Record<string, WorkspaceStats> = {};
      statsArr.forEach((s) => (map[s.id] = s));
      stats = map;
    } catch (e) {
      error = formatError(e);
    }
  }

  async function createWorkspace() {
    if (!createForm.name.trim()) return;
    saving = true;
    try {
      const w = await api.workspaces.create(createForm);
      workspacesStore.list = [...workspacesStore.list, w];
      workspaces = applyOrder(workspacesStore.list);
      stats = { ...stats, [w.id]: { id: w.id, profile_count: 0, proxy_count: 0, active_count: 0 } };
      saveOrder();
      createModal = false;
      createForm = { name: '', description: '', color: '#6366f1' };
    } catch (e) {
      error = formatError(e);
    } finally {
      saving = false;
    }
  }

  function openEdit(w: Workspace) {
    editModal = { open: true, id: w.id, name: w.name, description: w.description ?? '', color: w.color };
  }

  async function saveEdit() {
    saving = true;
    try {
      const updated = await api.workspaces.update(editModal.id, {
        name: editModal.name,
        description: editModal.description || null,
        color: editModal.color,
      });
      workspacesStore.list = workspacesStore.list.map((w) => (w.id === updated.id ? updated : w));
      workspaces = workspaces.map((w) => (w.id === updated.id ? updated : w));
      editModal = { open: false, id: '', name: '', description: '', color: '#6366f1' };
    } catch (e) {
      error = formatError(e);
    } finally {
      saving = false;
    }
  }

  function openDelete(w: Workspace) {
    deleteModal = { open: true, id: w.id, name: w.name };
  }

  async function confirmDelete(mode: 'move_to_default' | 'delete_all') {
    try {
      await api.workspaces.delete(deleteModal.id, mode);
      workspacesStore.list = workspacesStore.list.filter((w) => w.id !== deleteModal.id);
      workspaces = workspaces.filter((w) => w.id !== deleteModal.id);
      const { [deleteModal.id]: _, ...rest } = stats;
      stats = rest;
      deleteModal = { open: false, id: '', name: '' };
      await profilesStore.refresh();
      await proxiesStore.refresh();
    } catch (e) {
      error = formatError(e);
    }
  }

  function getStats(id: string) {
    return stats[id] ?? { profile_count: 0, proxy_count: 0, active_count: 0 };
  }
</script>

<div class="page">
  <div class="page-header">
    <h1>{$t('workspaces_title')}</h1>
    <button class="btn btn-primary spacer" onclick={() => (createModal = true)}>
      <Icon name="plus" size={14} />{$t('workspaces_new')}
    </button>
  </div>

  {#if error}
    <div class="error-msg" style="margin-bottom:1rem">{error}</div>
  {/if}

  {#if loading}
    <div class="empty-state">{$t('loading')}</div>
  {:else if workspaces.length === 0}
    <div class="empty-state">
      <div class="empty-icon"><Icon name="layers" size={48} strokeWidth={1.25} /></div>
      <p>{$t('workspaces_empty')}</p>
      <button class="btn btn-primary" onclick={() => (createModal = true)}>
        {$t('workspaces_empty_create')}
      </button>
    </div>
  {:else}
    <div class="workspace-grid">
      {#each workspaces as ws, i (ws.id)}
        {@const s = getStats(ws.id)}
        <div
          class="workspace-card"
          class:drag-source={dragSrcIndex === i}
          class:insert-before={ghostActive && insertAt === i && dragSrcIndex !== i && dragSrcIndex !== i - 1}
          style="--ws-color: {ws.color}"
          role="button"
          tabindex="0"
          onclick={() => !ghostActive && goto(`/workspace/${ws.id}`)}
          onkeydown={(e) => e.key === 'Enter' && goto(`/workspace/${ws.id}`)}
        >

          <div class="card-accent"></div>
          <div class="card-body">
            <div class="card-header">
              <div class="drag-handle" role="button" tabindex="-1" aria-label="Drag to reorder" title="Drag to reorder" onpointerdown={(e) => onHandlePointerDown(e, i)}>
                <Icon name="grip-vertical" size={14} />
              </div>
              <div class="card-icon" style="background: color-mix(in srgb, {ws.color} 15%, var(--surface-2))">
                <Icon name="layers" size={18} />
              </div>
              <div class="card-title-group">
                <h2 class="card-title">{ws.name}</h2>
                {#if ws.description}
                  <p class="card-desc">{ws.description}</p>
                {/if}
              </div>
              {#if !ws.is_default}
                <div class="card-menu">
                  <button class="menu-btn" onclick={(e) => { e.stopPropagation(); openEdit(ws); }} title={$t('workspaces_btn_edit')}>
                    <Icon name="pencil" size={13} />
                  </button>
                  <button class="menu-btn danger" onclick={(e) => { e.stopPropagation(); openDelete(ws); }} title={$t('workspaces_btn_delete')}>
                    <Icon name="trash-2" size={13} />
                  </button>
                </div>
              {/if}
            </div>

            <div class="card-stats">
              <div class="stat">
                <Icon name="folder-open" size={13} />
                <span>{s.profile_count} {$t('workspaces_profiles')}</span>
              </div>
              <div class="stat">
                <Icon name="globe" size={13} />
                <span>{s.proxy_count} {$t('workspaces_proxies')}</span>
              </div>
              {#if s.active_count > 0}
                <div class="stat active">
                  <span class="active-dot"></span>
                  <span>{s.active_count} {$t('workspaces_active')}</span>
                </div>
              {/if}
            </div>
          </div>

          <div class="card-footer">
            <a href="/workspace/{ws.id}" class="open-btn" onclick={(e) => e.stopPropagation()}>
              {$t('workspaces_open')}
            </a>
          </div>
        </div>
      {/each}

      <button class="add-card" onclick={() => (createModal = true)}>
        <Icon name="plus" size={24} strokeWidth={1.5} />
        <span>{$t('workspaces_new')}</span>
      </button>
    </div>
  {/if}
</div>

{#if ghostActive && dragSrcIndex !== -1}
  <div
    class="drag-ghost"
    style="left:{ghostX}px; top:{ghostY}px; --ws-color:{workspaces[dragSrcIndex]?.color ?? '#6366f1'}"
  >
    <div class="card-accent"></div>
    <div class="ghost-name">{workspaces[dragSrcIndex]?.name}</div>
  </div>
{/if}

<!-- Create Workspace Modal -->
{#if createModal}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div class="overlay" onclick={() => (createModal = false)} onkeydown={(e) => e.key === 'Escape' && (createModal = false)} role="presentation" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-labelledby="create-modal-title">
      <div class="modal-header">
        <h3 id="create-modal-title">New Workspace</h3>
        <button class="icon-btn" onclick={() => (createModal = false)} aria-label="Close"><Icon name="x" size={15} /></button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label for="ws-name">{$t('workspaces_form_name')}</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            id="ws-name"
            type="text"
            bind:value={createForm.name}
            placeholder={$t('workspaces_form_name_placeholder')}
            autofocus
          />
        </div>
        <div class="form-group">
          <label for="ws-desc">{$t('workspaces_form_desc')}</label>
          <input id="ws-desc" type="text" bind:value={createForm.description} placeholder="…" />
        </div>
        <div class="form-group">
          <label for="ws-color">{$t('workspaces_form_color')}</label>
          <div id="ws-color" class="color-picker" role="group" aria-label={$t('workspaces_form_color')}>
            {#each COLORS as c}
              <button
                class="color-swatch"
                class:selected={createForm.color === c}
                style="background: {c}"
                aria-label="Color {c}"
                aria-pressed={createForm.color === c}
                onclick={() => (createForm.color = c)}
              ></button>
            {/each}
            <label
              class="color-swatch color-swatch-custom"
              class:selected={!COLORS.includes(createForm.color ?? '')}
              title="Custom color"
              aria-label="Custom color"
            >
              <input type="color" bind:value={createForm.color} />
              {#if !COLORS.includes(createForm.color ?? '')}
                <span class="custom-dot" style="background:{createForm.color}"></span>
              {/if}
            </label>
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" onclick={() => (createModal = false)}>{$t('workspaces_btn_cancel')}</button>
        <button class="btn btn-primary" disabled={saving || !createForm.name.trim()} onclick={createWorkspace}>
          {saving ? '…' : $t('workspaces_btn_create')}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Edit Workspace Modal -->
{#if editModal.open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div class="overlay" onclick={() => (editModal = { ...editModal, open: false })} onkeydown={(e) => e.key === 'Escape' && (editModal = { ...editModal, open: false })} role="presentation" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-labelledby="edit-modal-title">
      <div class="modal-header">
        <h3 id="edit-modal-title">{$t('workspaces_btn_edit')}</h3>
        <button class="icon-btn" onclick={() => (editModal = { ...editModal, open: false })} aria-label="Close"><Icon name="x" size={15} /></button>
      </div>
      <div class="modal-body">
        <div class="form-group">
          <label for="edit-name">{$t('workspaces_form_name')}</label>
          <input id="edit-name" type="text" bind:value={editModal.name} />
        </div>
        <div class="form-group">
          <label for="edit-desc">{$t('workspaces_form_desc')}</label>
          <input id="edit-desc" type="text" bind:value={editModal.description} />
        </div>
        <div class="form-group">
          <label for="edit-color">{$t('workspaces_form_color')}</label>
          <div id="edit-color" class="color-picker" role="group" aria-label={$t('workspaces_form_color')}>
            {#each COLORS as c}
              <button
                class="color-swatch"
                class:selected={editModal.color === c}
                style="background: {c}"
                aria-label="Color {c}"
                aria-pressed={editModal.color === c}
                onclick={() => (editModal.color = c)}
              ></button>
            {/each}
            <label
              class="color-swatch color-swatch-custom"
              class:selected={!COLORS.includes(editModal.color)}
              title="Custom color"
              aria-label="Custom color"
            >
              <input type="color" bind:value={editModal.color} />
              {#if !COLORS.includes(editModal.color)}
                <span class="custom-dot" style="background:{editModal.color}"></span>
              {/if}
            </label>
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" onclick={() => (editModal = { ...editModal, open: false })}>{$t('workspaces_btn_cancel')}</button>
        <button class="btn btn-primary" disabled={saving} onclick={saveEdit}>
          {saving ? '…' : $t('workspaces_btn_save')}
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Delete Workspace Modal -->
{#if deleteModal.open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div class="overlay" onclick={() => (deleteModal = { ...deleteModal, open: false })} onkeydown={(e) => e.key === 'Escape' && (deleteModal = { ...deleteModal, open: false })} role="presentation" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
    <div class="modal" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-labelledby="delete-modal-title">
      <div class="modal-header">
        <h3 id="delete-modal-title">{$t('workspaces_btn_delete')}</h3>
        <button class="icon-btn" onclick={() => (deleteModal = { ...deleteModal, open: false })} aria-label="Close"><Icon name="x" size={15} /></button>
      </div>
      <div class="modal-body">
        <p class="delete-warning">
          <Icon name="alert-triangle" size={16} />
          Delete <strong>{deleteModal.name}</strong>?
        </p>
        <div class="delete-options">
          <button class="delete-option" onclick={() => confirmDelete('move_to_default')}>
            <Icon name="arrow-left" size={14} />
            <div>
              <div class="option-title">{$t('workspaces_delete_move')}</div>
              <div class="option-desc">Profiles and proxies will be moved to Default workspace</div>
            </div>
          </button>
          <button class="delete-option danger" onclick={() => confirmDelete('delete_all')}>
            <Icon name="trash-2" size={14} />
            <div>
              <div class="option-title">{$t('workspaces_delete_all')}</div>
              <div class="option-desc">All profiles and proxies will be permanently deleted</div>
            </div>
          </button>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn btn-ghost" onclick={() => (deleteModal = { ...deleteModal, open: false })}>{$t('workspaces_btn_cancel')}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* ── Grid ── */
  .workspace-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: var(--sp-4);
  }

  /* ── Card ── */
  .workspace-card {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow);
    transition: border-color 0.15s, transform 0.15s, box-shadow 0.15s;
    cursor: pointer;
  }

  .workspace-card:hover {
    border-color: var(--ws-color, var(--border-2));
    transform: translateY(-2px);
    box-shadow: var(--shadow-lg);
  }
  .workspace-card.drag-source { opacity: 0.25; transition: none; }
  .workspace-card.insert-before { box-shadow: -4px 0 0 0 var(--accent), var(--shadow-lg); }

  .drag-ghost {
    position: fixed;
    width: 220px;
    background: var(--bg-2);
    border: 1.5px solid var(--ws-color, var(--accent));
    border-radius: var(--radius);
    box-shadow: 0 8px 32px rgba(0,0,0,0.25);
    pointer-events: none;
    z-index: 9999;
    opacity: 0.93;
    transform: translate(-50%, -50%) rotate(2deg) scale(1.04);
    overflow: hidden;
    user-select: none;
  }
  .ghost-name {
    padding: 0.625rem 0.875rem;
    font-size: var(--fs-sm);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .drag-handle {
    display: flex; align-items: center; justify-content: center;
    color: var(--text-3); cursor: grab; flex-shrink: 0;
    width: 18px; opacity: 0; transition: opacity 0.15s;
  }
  .workspace-card:hover .drag-handle { opacity: 1; }
  .drag-handle:active { cursor: grabbing; }

  .card-accent {
    height: 3px;
    background: var(--ws-color, var(--accent));
  }

  .card-body {
    padding: var(--sp-4);
    display: flex;
    flex-direction: column;
    gap: 0.875rem;
    flex: 1;
  }

  .card-header {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-3);
  }

  .card-icon {
    width: 36px;
    height: 36px;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ws-color, var(--accent));
    flex-shrink: 0;
  }

  .card-title-group { flex: 1; min-width: 0; }
  .card-title { font-size: var(--fs-md); font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-desc { font-size: var(--fs-xs); color: var(--text-2); margin-top: 0.15rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .card-menu { display: flex; gap: 0.2rem; flex-shrink: 0; }
  .menu-btn {
    display: flex; align-items: center; justify-content: center;
    width: 26px; height: 26px; background: transparent; border-radius: var(--radius-sm);
    border: none; color: var(--text-3); cursor: pointer; transition: all 0.15s;
  }
  .menu-btn:hover { background: var(--surface-2); color: var(--text-2); }
  .menu-btn.danger:hover { background: var(--danger-bg); color: var(--danger-text); }

  .card-stats { display: flex; flex-direction: column; gap: 0.35rem; }
  .stat { display: flex; align-items: center; gap: 0.4rem; font-size: var(--fs-sm); color: var(--text-2); }
  .stat.active { color: var(--success-text); font-weight: 500; }
  .active-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--success); flex-shrink: 0; }

  .card-footer {
    border-top: 1px solid var(--border);
    padding: 0.625rem var(--sp-4);
  }

  .open-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--ws-color, var(--accent));
    text-decoration: none;
    transition: opacity 0.15s;
  }
  .open-btn:hover { opacity: 0.75; }

  /* ── Add card ── */
  .add-card {
    background: transparent;
    border: 2px dashed var(--border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    color: var(--text-3);
    cursor: pointer;
    min-height: 160px;
    transition: all 0.15s;
    font-size: var(--fs-sm);
  }
  .add-card:hover { border-color: var(--accent); color: var(--accent); background: var(--accent-bg); }

  /* ── Modals ── */
  .overlay {
    position: fixed; inset: 0; background: rgba(0,0,0,0.5);
    display: flex; align-items: center; justify-content: center;
    z-index: 100; backdrop-filter: blur(2px);
  }

  .modal {
    background: var(--bg-2); border: 1px solid var(--border);
    border-radius: var(--radius); box-shadow: var(--shadow-lg);
    width: 400px; max-width: 95vw;
    display: flex; flex-direction: column;
    overflow: hidden;
  }

  .modal-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: var(--sp-4) var(--sp-5);
    border-bottom: 1px solid var(--border);
  }
  .modal-header h3 { font-size: var(--fs-md); font-weight: 600; }

  .modal-body { padding: var(--sp-5); display: flex; flex-direction: column; gap: 0.875rem; }
  .modal-footer { padding: 0.875rem var(--sp-5); border-top: 1px solid var(--border); display: flex; gap: var(--sp-2); justify-content: flex-end; }

  .color-picker { display: flex; gap: 0.4rem; flex-wrap: wrap; align-items: center; }
  .color-swatch {
    width: 24px; height: 24px; border-radius: 50%; border: 2px solid transparent;
    cursor: pointer; transition: transform 0.1s, border-color 0.1s;
    padding: 0; flex-shrink: 0;
  }
  .color-swatch:hover { transform: scale(1.15); }
  .color-swatch.selected { border-color: var(--text); transform: scale(1.15); }

  .color-swatch-custom {
    position: relative;
    background: conic-gradient(
      #f43f5e, #f97316, #eab308, #22c55e, #06b6d4, #6366f1, #ec4899, #f43f5e
    );
    display: flex; align-items: center; justify-content: center;
    overflow: hidden;
  }
  .color-swatch-custom input[type="color"] {
    position: absolute; inset: 0; width: 100%; height: 100%;
    opacity: 0; cursor: pointer; border: none; padding: 0; margin: 0;
  }
  .color-swatch-custom .custom-dot {
    position: absolute; inset: 3px; border-radius: 50%;
    border: 1.5px solid rgba(255,255,255,0.6);
    pointer-events: none;
  }

  .delete-warning {
    display: flex; align-items: center; gap: var(--sp-2);
    color: var(--warn-text); font-size: var(--fs-sm);
    background: var(--warn-bg); padding: 0.625rem 0.875rem;
    border-radius: var(--radius-sm);
  }

  .delete-options { display: flex; flex-direction: column; gap: var(--sp-2); }
  .delete-option {
    display: flex; align-items: flex-start; gap: var(--sp-3);
    padding: var(--sp-3); border-radius: var(--radius-sm);
    background: var(--surface-2); border: 1px solid var(--border);
    cursor: pointer; text-align: left; transition: all 0.15s; color: var(--text);
  }
  .delete-option:hover { border-color: var(--border-2); background: var(--surface); }
  .delete-option.danger { color: var(--danger-text); }
  .delete-option.danger:hover { background: var(--danger-bg); border-color: color-mix(in srgb, var(--danger) 30%, var(--border)); }
  .option-title { font-size: var(--fs-sm); font-weight: 600; margin-bottom: 0.15rem; }
  .option-desc { font-size: var(--fs-xs); color: var(--text-2); }
</style>
