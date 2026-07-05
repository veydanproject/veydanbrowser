<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import type { Workspace, Profile, Proxy, WorkspaceColumn, ProxyCheckResult } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Modal from '$lib/Modal.svelte';
  import KanbanBoard from '$lib/components/KanbanBoard.svelte';
  import TableView from '$lib/components/TableView.svelte';
  import TopologyGraph from '$lib/components/TopologyGraph.svelte';
  import CreateProfilePanel from '$lib/components/CreateProfilePanel.svelte';
  import EditProfilePanel from '$lib/components/EditProfilePanel.svelte';
  import ProfileSidePanel from '$lib/components/ProfileSidePanel.svelte';
  import RawDataPanel from '$lib/components/RawDataPanel.svelte';
  import ImportProfileModal from '$lib/components/ImportProfileModal.svelte';
  import ProxyPanel from '$lib/components/ProxyPanel.svelte';
  import { listen } from '@tauri-apps/api/event';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { proxiesStore } from '$lib/store/proxies.svelte';
  import { formatError } from '$lib/utils';
  import TotpGenerator from '$lib/components/TotpGenerator.svelte';
  import NotesPanel from '$lib/components/notes/NotesPanel.svelte';
  import SSHPanel from '$lib/components/ssh/SSHPanel.svelte';

  let workspaceId = $derived($page.params.id ?? '');

  type Tab = 'board' | 'proxies' | 'topology' | 'notes';
  type ViewMode = 'kanban' | 'table';

  let tab = $state<Tab>('board');
  let viewMode = $state<ViewMode>('kanban');

  let workspace = $state<Workspace | null>(null);
  let profiles = $state<Profile[]>([]);
  let proxies = $state<Proxy[]>([]);
  let columns = $state<WorkspaceColumn[]>([]);
  let loading = $state(false);
  let error = $state('');
  let saving = $state(false);
  let notesValue = $state('');
  let notesSaving = $state(false);

  // Running profiles set (updated by events + fallback)
  let runningProfiles = $state<Set<string>>(new Set());

  let showCreatePanel = $state(false);
  let showImportModal = $state(false);
  let editingProfile = $state<import('$lib/types').Profile | null>(null);
  let selectedProfile = $state<import('$lib/types').Profile | null>(null);
  let rawDataProfile = $state<import('$lib/types').Profile | null>(null);
  let unlistenRunning: (() => void) | undefined;

  const isSelectedRunning = $derived(
    selectedProfile ? runningProfiles.has(selectedProfile.id) : false
  );

  // Reload data whenever workspaceId changes (covers initial load + navigation between workspaces)
  $effect(() => {
    const id = workspaceId;
    if (!id) return;
    tab = 'board';
    workspace = null;
    profiles = [];
    proxies = [];
    columns = [];
    notesValue = '';
    viewMode = (localStorage.getItem(`rb-view-${id}`) as ViewMode) ?? 'kanban';
    loadData();
    refreshStatuses();
  });

  onMount(async () => {
    // Event listener lives for the full component lifecycle
    unlistenRunning = await listen<{ running_ids: string[] }>(
      'profiles://running-changed',
      (e) => { runningProfiles = new Set(e.payload.running_ids); }
    );
  });

  onDestroy(() => unlistenRunning?.());

  async function loadData() {
    loading = true;
    try {
      const [ws, cols] = await Promise.all([
        api.workspaces.get(workspaceId),
        api.workspaces.columns.list(workspaceId),
        profilesStore.ensureLoaded(),
        proxiesStore.ensureLoaded(),
      ]);
      if (!ws) { goto('/', { replaceState: true }); return; }
      workspace = ws;
      columns = cols;
      notesValue = ws.notes ?? '';
      profiles = profilesStore.byWorkspace(workspaceId);
      proxies = proxiesStore.byWorkspace(workspaceId);
    } catch (e) { error = formatError(e); }
    finally { loading = false; }
  }

  async function refreshStatuses() {
    try {
      const ids = await api.profiles.runningIds();
      runningProfiles = new Set(ids);
    } catch {}
  }

  function setViewMode(mode: ViewMode) {
    viewMode = mode;
    localStorage.setItem(`rb-view-${workspaceId}`, mode);
  }

  async function saveNotes() {
    if (!workspace) return;
    notesSaving = true;
    try {
      const updated = await api.workspaces.update(workspaceId, { notes: notesValue || null });
      workspace = updated;
    } catch (e) { error = formatError(e); }
    finally { notesSaving = false; }
  }

  async function stopAll() {
    saving = true;
    try {
      const ids = [...runningProfiles];
      await Promise.all(ids.map((id) => api.profiles.stop(id).catch(() => {})));
      await refreshStatuses();
    } catch (e) { error = formatError(e); }
    finally { saving = false; }
  }
  async function onProfileChange() {
    await profilesStore.refresh();
    profiles = profilesStore.byWorkspace(workspaceId);
  }
  function onColumnsChange() { /* columns already updated in child via $bindable */ }

  let runningCount = $derived(runningProfiles.size);

  // --- Proxy CRUD ---
  let totpOpen = $state(false);
  let notesOpen = $state(false);
  let sshOpen = $state(false);
  let proxyPanelProxy = $state<Proxy | null | undefined>(undefined);
  let proxyCheckResults = $state<Record<string, ProxyCheckResult & { checking?: boolean; err?: string }>>({});
  let proxyDeleteModal = $state({ open: false, id: '', name: '' });
  let proxySearch = $state('');

  function openCreateProxy() {
    proxyPanelProxy = null;
  }

  let fingerprintPrompt = $state<{ id: string; fingerprint: string; ip: string; country: string | null; city: string | null } | null>(null);

  async function checkProxy(id: string) {
    proxyCheckResults = { ...proxyCheckResults, [id]: { ip: '', country: null, city: null, ok: false, checking: true } };
    try {
      const result = await proxiesStore.check(id);
      proxyCheckResults = { ...proxyCheckResults, [id]: { ...result, checking: false } };

      if (result.ssh_fingerprint_is_new && result.ssh_fingerprint) {
        fingerprintPrompt = { id, fingerprint: result.ssh_fingerprint, ip: result.ip, country: result.country, city: result.city };
        return;
      }

      proxies = proxiesStore.byWorkspace(workspaceId);
    } catch (e) {
      proxyCheckResults = { ...proxyCheckResults, [id]: { ip: '', country: null, city: null, ok: false, checking: false, err: formatError(e) } };
      proxiesStore.markFailed(id);
      proxies = proxiesStore.byWorkspace(workspaceId);
    }
  }

  async function trustFingerprint() {
    if (!fingerprintPrompt) return;
    const { id, fingerprint, ip, country, city } = fingerprintPrompt;
    try {
      await proxiesStore.trustFingerprint(id, fingerprint, ip, country, city);
      proxies = proxiesStore.byWorkspace(workspaceId);
    } catch (e) { console.error('trustFingerprint error:', e); }
    finally { fingerprintPrompt = null; }
  }

  async function confirmDeleteProxy() {
    try {
      await proxiesStore.remove(proxyDeleteModal.id);
      proxies = proxiesStore.byWorkspace(workspaceId);
    } catch {}
    finally { proxyDeleteModal = { open: false, id: '', name: '' }; }
  }

  function onProxyPanelSaved(proxy: Proxy) {
    proxiesStore.upsert(proxy);
    proxies = proxiesStore.byWorkspace(workspaceId);
    proxyPanelProxy = undefined;
  }

  let filteredProxies = $derived(proxies.filter((p) => {
    if (!proxySearch.trim()) return true;
    const q = proxySearch.toLowerCase();
    return (
      p.name.toLowerCase().includes(q) ||
      p.host.toLowerCase().includes(q) ||
      (p.country ?? '').toLowerCase().includes(q) ||
      (p.last_ip ?? '').includes(q)
    );
  }));
</script>

<div class="page page--fill ws-page">
  {#if loading}
    <div class="centered">{$t('loading')}</div>
  {:else if workspace}
    <!-- Header -->
    <div class="ws-header">
      <div class="breadcrumb">
        <a href="/" class="back-link">
          <Icon name="arrow-left" size={13} />
          {$t('back_workspaces')}
        </a>
        <Icon name="chevron-right" size={13} />
        <span class="ws-name" style="color: {workspace.color}">{workspace.name}</span>
      </div>

      <div class="ws-meta">
        <span class="badge">
          <Icon name="folder-open" size={12} />
          {profiles.length} {$t('workspaces_profiles')}
        </span>
        <span class="badge">
          <Icon name="globe" size={12} />
          {proxies.length} {$t('workspaces_proxies')}
        </span>
        {#if runningCount > 0}
          <span class="badge badge-ok">
            <span class="dot"></span>
            {runningCount} {$t('workspaces_active')}
          </span>
        {/if}
      </div>

      <div class="ws-actions">
        {#if runningCount > 0}
          <button class="btn btn-ghost btn-sm" disabled={saving} onclick={stopAll}>
            <Icon name="square" size={12} />{$t('workspace_btn_stop_all')}
          </button>
        {/if}
        <button class="btn btn-ghost btn-sm" onclick={() => (totpOpen = true)} title={$t('totp_title')}>
          <Icon name="shield" size={12} />TOTP
        </button>
        <button class="btn btn-ghost btn-sm" onclick={() => (notesOpen = true)} title="Notes">
          <Icon name="file-text" size={12} />Notes
        </button>
        <button class="btn btn-ghost btn-sm" onclick={() => (sshOpen = true)} title={$t('ssh_title')}>
          <Icon name="terminal" size={12} />{$t('ssh_bar_label')}
        </button>
        <button
          class="btn btn-ghost btn-sm"
          class:action-hidden={tab === 'proxies'}
          onclick={() => (showImportModal = true)}
        >
          <Icon name="download" size={12} />{$t('workspace_btn_import')}
        </button>
        <button
          class="btn btn-primary btn-sm"
          onclick={tab === 'proxies' ? openCreateProxy : () => (showCreatePanel = true)}
        >
          <Icon name="plus" size={12} />
          {tab === 'proxies' ? $t('proxies_add') : $t('workspace_btn_add_profile')}
        </button>
      </div>
    </div>

    {#if error}
      <div class="error-msg" style="margin-bottom:0.75rem">{error}</div>
    {/if}

    {#if fingerprintPrompt}
      <div class="fingerprint-banner">
        <div class="fingerprint-banner-icon">⚠</div>
        <div class="fingerprint-banner-body">
          <div class="fingerprint-banner-title">{$t('ssh_fingerprint_new_title')}</div>
          <div class="fingerprint-banner-fp">{fingerprintPrompt.fingerprint}</div>
          <div class="fingerprint-banner-hint">{$t('ssh_fingerprint_new_hint')}</div>
        </div>
        <div class="fingerprint-banner-actions">
          <button class="btn btn-primary btn-sm" onclick={trustFingerprint}>{$t('ssh_fingerprint_trust')}</button>
          <button class="btn btn-ghost btn-sm" onclick={() => fingerprintPrompt = null}>{$t('cancel')}</button>
        </div>
      </div>
    {/if}

    <!-- Tabs + View Switcher -->
    <div class="tabs-row">
      <div class="tab-bar">
        <button class="tab" class:active={tab === 'board'} onclick={() => (tab = 'board')}>
          <Icon name="kanban" size={14} />{$t('workspace_tab_board')}
        </button>
        <button class="tab" class:active={tab === 'proxies'} onclick={() => (tab = 'proxies')}>
          <Icon name="globe" size={14} />{$t('workspace_tab_proxies')}
          <span class="tab-count">{proxies.length}</span>
        </button>
        <button class="tab" class:active={tab === 'topology'} onclick={() => (tab = 'topology')}>
          <Icon name="git-fork" size={14} />{$t('workspace_tab_topology')}
        </button>
      </div>

      <div class="view-switcher" class:hidden={tab !== 'board'}>
          <button
            class="view-btn"
            class:active={viewMode === 'kanban'}
            onclick={() => setViewMode('kanban')}
            title="Kanban view"
          >
            <Icon name="kanban" size={14} />
          </button>
          <button
            class="view-btn"
            class:active={viewMode === 'table'}
            onclick={() => setViewMode('table')}
            title="Table view"
          >
            <Icon name="list" size={14} />
          </button>
        </div>
    </div>

    <!-- Tab Content -->
    <div class="tab-content">
      {#if tab === 'board'}
        {#if viewMode === 'kanban'}
          <KanbanBoard
            bind:profiles
            bind:columns
            {proxies}
            {workspaceId}
            bind:runningProfiles
            onprofilechange={onProfileChange}
            oncolumnschange={onColumnsChange}
            onedit={(p) => (editingProfile = p)}
            onrawdata={(p) => (rawDataProfile = p)}
          />
        {:else}
          <TableView
            {profiles}
            {proxies}
            {columns}
            {runningProfiles}
            onSelect={(p) => (selectedProfile = p)}
            onEdit={(p) => (editingProfile = p)}
            onRefresh={onProfileChange}
          />
        {/if}

      {:else if tab === 'proxies'}
        <div class="proxies-tab">
          {#if proxies.length === 0}
            <div class="empty-state">
              <Icon name="globe" size={36} strokeWidth={1.5} />
              <p>{$t('proxies_empty')}</p>
              <button class="btn btn-primary btn-sm" onclick={openCreateProxy}>
                <Icon name="plus" size={13} />{$t('proxies_empty_add')}
              </button>
            </div>
          {:else}
            <div class="filter-bar">
              <div class="search-field">
                <Icon name="search" size={13} />
                <input
                  type="text"
                  bind:value={proxySearch}
                  placeholder={$t('proxies_search_placeholder')}
                />
                {#if proxySearch}
                  <button class="search-clear" onclick={() => (proxySearch = '')}><Icon name="x" size={11} /></button>
                {/if}
              </div>
              <span class="count-badge">{filteredProxies.length} / {proxies.length}</span>
            </div>

            {#if filteredProxies.length === 0}
              <div class="empty-state" style="padding:2rem">
                <Icon name="search" size={28} strokeWidth={1.5} />
                <p>{$t('proxies_not_found')}</p>
              </div>
            {:else}
              <div class="proxy-table-wrap">
                <table class="proxy-table">
                  <thead>
                    <tr>
                      <th class="col-num">#</th>
                      <th>{$t('proxy_col_name')}</th>
                      <th class="col-type">{$t('proxy_col_type')}</th>
                      <th>{$t('proxy_col_host')}</th>
                      <th class="col-status">{$t('proxy_col_status')}</th>
                      <th class="col-actions"></th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each filteredProxies as proxy, i (proxy.id)}
                      {@const result = proxyCheckResults[proxy.id]}
                      <tr class="proxy-row" onclick={() => (proxyPanelProxy = proxy)}>
                        <td class="col-num">
                          <span class="row-num">{i + 1}</span>
                        </td>
                        <td class="col-name">
                          <span class="proxy-name">{proxy.name}</span>
                        </td>
                        <td class="col-type">
                          <span class="type-badge type-{proxy.proxy_type}">{proxy.proxy_type}</span>
                        </td>
                        <td><code class="host-code">{proxy.host}:{proxy.port}</code></td>
                        <td class="col-status">
                          <span class="status-badge status-{proxy.status}">{proxy.status}</span>
                        </td>
                        <td class="col-actions">
                          <div class="row-acts">
                            <button class="act-btn" title={$t('proxy_btn_check')} disabled={result?.checking} onclick={(e) => { e.stopPropagation(); checkProxy(proxy.id); }}>
                              <Icon name="refresh-cw" size={12} />
                            </button>
                            <button class="act-btn" title={$t('proxy_btn_edit')} onclick={(e) => { e.stopPropagation(); proxyPanelProxy = proxy; }}>
                              <Icon name="pencil" size={12} />
                            </button>
                            <button class="act-btn act-danger" title={$t('proxy_btn_delete')} onclick={(e) => { e.stopPropagation(); proxyDeleteModal = { open: true, id: proxy.id, name: proxy.name }; }}>
                              <Icon name="trash-2" size={12} />
                            </button>
                          </div>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {/if}
          {/if}
        </div>

      {:else if tab === 'topology'}
        <TopologyGraph {profiles} {proxies} {runningProfiles} />

      {:else if tab === 'notes'}
        <div class="notes-tab">
          <textarea
            class="notes-area"
            bind:value={notesValue}
            placeholder={$t('workspace_notes_placeholder')}
          ></textarea>
          <div class="notes-footer">
            <button class="btn btn-primary btn-sm" disabled={notesSaving} onclick={saveNotes}>
              {notesSaving ? '…' : $t('workspace_btn_save_notes')}
            </button>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

{#if showCreatePanel}
  <CreateProfilePanel
    {workspaceId}
    proxies={proxiesStore.list}
    onclose={() => (showCreatePanel = false)}
    oncreated={() => { showCreatePanel = false; onProfileChange(); }}
  />
{/if}

<ImportProfileModal
  {workspaceId}
  open={showImportModal}
  onclose={() => (showImportModal = false)}
  onimported={() => { showImportModal = false; onProfileChange(); }}
/>

{#if selectedProfile}
  <ProfileSidePanel
    profile={selectedProfile}
    proxy={proxiesStore.list.find((p) => p.id === selectedProfile?.proxy_id) ?? null}
    {workspaceId}
    {columns}
    isRunning={isSelectedRunning}
    onclose={() => (selectedProfile = null)}
    onchange={() => { selectedProfile = null; onProfileChange(); }}
    onsync={(updated) => {
      profilesStore.list = profilesStore.list.map((p) => p.id === updated.id ? updated : p);
      profiles = profilesStore.byWorkspace(workspaceId);
      selectedProfile = updated;
      if (updated.status === 'running') {
        runningProfiles = new Set([...runningProfiles, updated.id]);
      } else {
        const next = new Set(runningProfiles);
        next.delete(updated.id);
        runningProfiles = next;
      }
    }}
    onedit={(p) => { selectedProfile = null; editingProfile = p; }}
    onrawdata={(p) => { selectedProfile = null; rawDataProfile = p; }}
  />
{/if}

{#if rawDataProfile}
  <RawDataPanel
    profile={rawDataProfile}
    onclose={() => (rawDataProfile = null)}
  />
{/if}

{#if editingProfile}
  <EditProfilePanel
    profile={editingProfile}
    proxies={proxiesStore.list}
    onclose={() => (editingProfile = null)}
    onsaved={(updated) => {
      profilesStore.list = profilesStore.list.map((p) => p.id === updated.id ? updated : p);
      profiles = profilesStore.byWorkspace(workspaceId);
      if (selectedProfile?.id === updated.id) selectedProfile = updated;
      editingProfile = null;
    }}
  />
{/if}

{#if proxyPanelProxy !== undefined}
  <ProxyPanel
    proxy={proxyPanelProxy}
    workspaceId={workspaceId}
    onclose={() => (proxyPanelProxy = undefined)}
    onsaved={onProxyPanelSaved}
  />
{/if}

<Modal
  open={proxyDeleteModal.open}
  title={$t('proxy_btn_delete')}
  message={$t('proxy_confirm_delete', { name: proxyDeleteModal.name })}
  confirmLabel={$t('proxy_btn_delete')}
  cancelLabel={$t('proxy_btn_cancel')}
  variant="danger"
  onconfirm={confirmDeleteProxy}
  oncancel={() => (proxyDeleteModal = { open: false, id: '', name: '' })}
/>

<TotpGenerator
  bind:open={totpOpen}
  context="workspace"
  contextId={workspaceId}
/>

<NotesPanel
  bind:open={notesOpen}
  context="workspace"
  contextId={workspaceId}
/>

<SSHPanel
  bind:open={sshOpen}
  context="workspace"
  workspaceId={workspaceId}
/>

<style>
  /* width / centering / height come from global .page + .page--fill */
  .ws-page {
    gap: var(--sp-2);
  }

  .centered { text-align: center; color: var(--text-2); padding: var(--sp-12); }

  .ws-header {
    display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-3);
    padding-bottom: 0.625rem; border-bottom: 1px solid var(--border);
  }

  .breadcrumb {
    display: flex; align-items: center; gap: 0.4rem; font-size: var(--fs-sm); color: var(--text-2);
  }

  .back-link {
    display: flex; align-items: center; gap: 0.3rem;
    color: var(--text-2); text-decoration: none; transition: color 0.15s;
  }
  .back-link:hover { color: var(--text); }

  .ws-name { font-weight: 700; font-size: var(--fs-md); }

  .ws-meta { display: flex; align-items: center; gap: 0.4rem; flex: 1; }

  .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--success); }

  .ws-actions { display: flex; gap: 0.4rem; flex-wrap: wrap; margin-left: auto; }
  .action-hidden { visibility: hidden; pointer-events: none; }

  /* Tabs row with view switcher */
  .tabs-row {
    display: flex; align-items: flex-end; justify-content: space-between;
    border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  /* .tab-bar (global) supplies its own border-bottom; .tabs-row already
     underlines the full row, so drop the redundant one and keep original padding. */
  .tab-bar { border-bottom: none; padding: 0.3rem var(--sp-2) 0; }

  /* View switcher */
  .view-switcher {
    display: flex; gap: 2px;
    background: var(--bg2); border: 1px solid var(--border);
    border-radius: 6px; padding: 2px;
    margin-bottom: 4px;
  }
  .view-switcher.hidden { visibility: hidden; pointer-events: none; }

  .view-btn {
    background: none; border: none; cursor: pointer;
    color: var(--text-muted); padding: 0.3rem var(--sp-2);
    border-radius: 4px; display: flex; align-items: center;
    transition: all 0.15s;
  }
  .view-btn.active { background: var(--accent); color: white; }
  .view-btn:not(.active):hover { background: var(--bg3); color: var(--text); }

  /* Tab Content */
  .tab-content {
    flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden;
  }

  /* Proxies Tab */
  .proxies-tab { display: flex; flex-direction: column; gap: 0.625rem; flex: 1; min-height: 0; }

  /* панель фильтров — единые примитивы .filter-bar/.search-field/.count-badge из base.css */

  .proxy-table-wrap {
    flex: 1; min-height: 0; overflow-y: auto;
    border: 1px solid var(--border); border-radius: var(--radius);
    background: var(--bg-2);
  }

  .proxy-table { width: 100%; border-collapse: collapse; font-size: var(--fs-sm); }
  .proxy-table thead {
    position: sticky; top: 0; z-index: 1;
    background: var(--surface-2); border-bottom: 1px solid var(--border);
  }
  .proxy-table th {
    padding: var(--sp-2) var(--sp-3); text-align: left;
    font-size: var(--fs-2xs); font-weight: 700; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.05em; white-space: nowrap;
  }
  .proxy-table td { padding: var(--sp-2) var(--sp-3); vertical-align: middle; }
  .proxy-row { border-bottom: 1px solid var(--border); cursor: pointer; transition: background 0.1s; }
  .proxy-row:last-child { border-bottom: none; }
  .proxy-row:hover { background: var(--surface-2); }

  .col-num { width: 40px; }
  .col-type { width: 72px; }
  .col-status { width: 90px; }
  .col-actions { width: 96px; text-align: right; }

  .row-num { font-size: var(--fs-2xs); color: var(--text-3); }
  .proxy-name { font-weight: 600; display: block; }
  .host-code { font-family: monospace; font-size: var(--fs-xs); color: var(--text-2); background: var(--surface-2); padding: 0.1rem 0.3rem; border-radius: 4px; }

  .type-badge {
    font-size: var(--fs-2xs); font-weight: 700; letter-spacing: 0.04em;
    padding: 0.1rem 0.4rem; border-radius: 999px;
    border: 1px solid var(--border); background: var(--surface-2); color: var(--text-2);
  }
  .type-socks5 { background: var(--accent-bg); border-color: color-mix(in srgb, var(--accent) 30%, var(--border)); color: var(--accent); }

  .status-badge {
    font-size: var(--fs-2xs); font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em;
    padding: 0.1rem 0.4rem; border-radius: 999px;
  }
  .status-active { background: var(--success-bg); color: var(--success-text); }
  .status-failed { background: var(--danger-bg); color: var(--danger-text); }
  .status-unknown { background: var(--surface-2); color: var(--text-3); border: 1px solid var(--border); }

  .row-acts { display: flex; gap: 0.2rem; justify-content: flex-end; }
  .act-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 28px; height: 28px;
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-sm);
    color: var(--text-2); cursor: pointer; padding: 0; transition: all 0.15s;
  }
  .act-btn:hover { background: var(--surface-2); border-color: var(--border); color: var(--text); }
  .act-btn:disabled { opacity: 0.35; cursor: not-allowed; }
  .act-danger:hover { background: var(--danger-bg) !important; border-color: color-mix(in srgb, var(--danger) 40%, var(--border)) !important; color: var(--danger-text) !important; }

  /* Notes Tab */
  .notes-tab { display: flex; flex-direction: column; gap: var(--sp-3); flex: 1; }
  .notes-area { flex: 1; resize: none; min-height: 200px; font-size: var(--fs-sm); line-height: 1.6; }
  .notes-footer { display: flex; justify-content: flex-end; }

  .fingerprint-banner {
    display: flex; align-items: flex-start; gap: var(--sp-3);
    background: color-mix(in srgb, var(--color-warning) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning) 40%, transparent);
    border-radius: var(--radius); padding: 0.875rem var(--sp-4); margin-bottom: var(--sp-3);
  }
  .fingerprint-banner-icon { font-size: var(--fs-lg); flex-shrink: 0; margin-top: 0.1rem; }
  .fingerprint-banner-body { flex: 1; display: flex; flex-direction: column; gap: var(--sp-1); }
  .fingerprint-banner-title { font-weight: 700; font-size: var(--fs-base); }
  .fingerprint-banner-fp {
    font-family: monospace; font-size: var(--fs-sm); color: var(--text-2);
    word-break: break-all;
  }
  .fingerprint-banner-hint { font-size: var(--fs-sm); color: var(--text-2); }
  .fingerprint-banner-actions { display: flex; gap: var(--sp-2); flex-shrink: 0; align-items: flex-start; }
</style>
