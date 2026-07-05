<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import type { SshConnection, Proxy } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Modal from '$lib/Modal.svelte';
  import Drawer from '$lib/components/ui/Drawer.svelte';
  import SSHConnectionForm from '$lib/components/ssh/SSHConnectionForm.svelte';
  import ProxyPanel from '$lib/components/ProxyPanel.svelte';
  import { sshStore } from '$lib/store/ssh.svelte';
  import { proxiesStore } from '$lib/store/proxies.svelte';
  import { api } from '$lib/api';
  import { formatError } from '$lib/utils';

  const PAGE_SIZE = 20;

  let loading = $state(false);
  let error = $state('');

  let search = $state('');
  let filterAuth = $state('all');
  let page = $state(0);

  let panelConn = $state<SshConnection | null | undefined>(undefined);
  let editProxy = $state<Proxy | undefined>(undefined);
  let deleteModal = $state({ open: false, id: '', name: '' });
  let connectingId = $state<string | null>(null);

  onMount(async () => {
    loading = true;
    try { await Promise.all([sshStore.ensureLoaded(), proxiesStore.ensureLoaded()]); }
    catch (e) { error = formatError(e); }
    finally { loading = false; }
  });

  let filtered = $derived(sshStore.connections.filter((c) => {
    if (filterAuth !== 'all' && c.auth_type !== filterAuth) return false;
    if (search.trim()) {
      const q = search.toLowerCase();
      return (
        c.name.toLowerCase().includes(q) ||
        c.host.toLowerCase().includes(q) ||
        c.username.toLowerCase().includes(q)
      );
    }
    return true;
  }));

  let totalPages = $derived(Math.max(1, Math.ceil(filtered.length / PAGE_SIZE)));
  let currentPage = $derived(Math.min(page, totalPages - 1));
  let pageItems = $derived(filtered.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE));

  $effect(() => {
    search; filterAuth;
    page = 0;
  });

  function connStatus(id: string): 'connected' | 'connecting' | 'idle' {
    const sessions = sshStore.sessionsForConnection(id);
    if (sessions.some((s) => s.status === 'connected')) return 'connected';
    if (sessions.some((s) => s.status === 'connecting')) return 'connecting';
    return 'idle';
  }

  async function handleConnect(conn: SshConnection) {
    connectingId = conn.id;
    try {
      await sshStore.connect(conn.id);
    } catch (e) {
      error = formatError(e);
    } finally {
      connectingId = null;
    }
  }

  async function handleDisconnect(conn: SshConnection) {
    const sessions = sshStore.sessionsForConnection(conn.id).filter(
      (s) => s.status === 'connected' || s.status === 'connecting'
    );
    for (const s of sessions) {
      try { await sshStore.disconnect(s.session_id); } catch {}
    }
  }

  async function confirmDelete() {
    try {
      await api.ssh.connectionDelete(deleteModal.id);
      sshStore.connections = sshStore.connections.filter((c) => c.id !== deleteModal.id);
    } catch (e) {
      error = formatError(e);
    } finally {
      deleteModal = { open: false, id: '', name: '' };
    }
  }

  function onFormSaved(conn: SshConnection) {
    const exists = sshStore.connections.find((c) => c.id === conn.id);
    sshStore.connections = exists
      ? sshStore.connections.map((c) => c.id === conn.id ? conn : c)
      : [conn, ...sshStore.connections];
    panelConn = undefined;
  }

  function authLabel(auth: string) {
    if (auth === 'password') return 'password';
    if (auth === 'key') return 'key';
    if (auth === 'key_password') return 'key+pass';
    return auth;
  }

  function formatDateParts(iso: string | null): { date: string; time: string } {
    if (!iso) return { date: '—', time: '' };
    const d = new Date(iso);
    return {
      date: d.toLocaleDateString(),
      time: d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    };
  }

  function proxyName(id: string | null): string {
    if (!id) return '—';
    return proxiesStore.list.find((p) => p.id === id)?.name ?? id.slice(0, 8);
  }

  function proxyForConn(id: string | null): Proxy | undefined {
    if (!id) return undefined;
    return proxiesStore.list.find((p) => p.id === id);
  }

  function onProxySaved(proxy: Proxy) {
    proxiesStore.list = proxiesStore.list.map((p) => p.id === proxy.id ? proxy : p);
    editProxy = undefined;
  }
</script>

<div class="page page--fill">
  <div class="page-header">
    <h1>{$t('terminal_title')}</h1>
    <button class="btn btn-primary spacer" onclick={() => (panelConn = null)}>
      <Icon name="plus" size={14} />{$t('terminal_add')}
    </button>
  </div>

  {#if error}<div class="error-msg" style="margin-bottom:1rem">{error}</div>{/if}

  <!-- Toolbar -->
  <div class="filter-bar">
    <div class="search-field">
      <Icon name="search" size={13} />
      <input type="text" bind:value={search} placeholder={$t('terminal_search_placeholder')} />
      {#if search}
        <button class="search-clear" onclick={() => (search = '')}><Icon name="x" size={11} /></button>
      {/if}
    </div>

    <select bind:value={filterAuth} class="filter-select">
      <option value="all">{$t('terminal_filter_all_auth')}</option>
      <option value="password">{$t('ssh_auth_password')}</option>
      <option value="key">{$t('ssh_auth_key')}</option>
      <option value="key_password">{$t('ssh_auth_key_password')}</option>
    </select>

    <span class="count-badge">{$t('terminal_count', { n: String(filtered.length) })}</span>
  </div>

  {#if loading}
    <div class="empty-state">{$t('loading')}</div>
  {:else if sshStore.connections.length === 0}
    <div class="empty-state">
      <div class="empty-icon"><Icon name="terminal" size={40} strokeWidth={1.5} /></div>
      <p>{$t('terminal_empty')}</p>
      <button class="btn btn-primary" onclick={() => (panelConn = null)}>
        <Icon name="plus" size={14} />{$t('terminal_empty_add')}
      </button>
    </div>
  {:else if filtered.length === 0}
    <div class="empty-state">
      <Icon name="search" size={32} strokeWidth={1.5} />
      <p>{$t('terminal_not_found')}</p>
    </div>
  {:else}
    <div class="table-wrap">
      <table class="ssh-table">
        <thead>
          <tr>
            <th class="col-num">#</th>
            <th class="col-status"></th>
            <th>{$t('terminal_col_name')}</th>
            <th class="col-host">{$t('terminal_col_host')}</th>
            <th class="col-user">{$t('terminal_col_user')}</th>
            <th class="col-auth">{$t('terminal_col_auth')}</th>
            <th class="col-proxy">{$t('terminal_col_proxy')}</th>
            <th class="col-last">{$t('terminal_col_last')}</th>
            <th class="col-actions">{$t('terminal_col_actions')}</th>
          </tr>
        </thead>
        <tbody>
          {#each pageItems as conn, i (conn.id)}
            {@const status = connStatus(conn.id)}
            {@const dp = formatDateParts(conn.last_connected_at)}
            <tr class="ssh-row" onclick={() => (panelConn = conn)}>
              <td class="col-num text-muted">{currentPage * PAGE_SIZE + i + 1}</td>
              <td class="col-status">
                <span class="status-dot status-dot-{status}" title={$t(`terminal_status_${status}`)}></span>
              </td>
              <td class="col-name">
                <span class="conn-name">{conn.name}</span>
              </td>
              <td class="col-host">
                <code>{conn.host}:{conn.port}</code>
              </td>
              <td class="col-user">
                <span class="text-muted">{conn.username}</span>
              </td>
              <td class="col-auth">
                <div class="auth-tags">
                  <span class="auth-badge auth-{conn.auth_type}">{authLabel(conn.auth_type)}</span>
                  {#if conn.password && conn.auth_type !== 'password'}
                    <span class="auth-badge">pass</span>
                  {/if}
                  {#if conn.requires_2fa}
                    <span class="tag-2fa">2FA</span>
                  {/if}
                </div>
              </td>
              <td class="col-proxy">
                {#if conn.proxy_id}
                  <button
                    class="proxy-chip"
                    title="Edit proxy"
                    onclick={(e) => { e.stopPropagation(); editProxy = proxyForConn(conn.proxy_id); }}
                  >{proxyName(conn.proxy_id)}</button>
                {:else}
                  <span class="text-muted">—</span>
                {/if}
              </td>
              <td class="col-last">
                <span class="date-cell">
                  <span>{dp.date}</span>
                  {#if dp.time}<span class="time-part">{dp.time}</span>{/if}
                </span>
              </td>
              <td class="col-actions">
                <div class="row-actions">
                  {#if status === 'connected' || status === 'connecting'}
                    <button
                      class="icon-btn danger-soft"
                      title={$t('terminal_btn_disconnect')}
                      onclick={(e) => { e.stopPropagation(); handleDisconnect(conn); }}
                    >
                      <Icon name="square" size={13} />
                    </button>
                  {:else}
                    <button
                      class="icon-btn success"
                      title={$t('terminal_btn_connect')}
                      disabled={connectingId === conn.id}
                      onclick={(e) => { e.stopPropagation(); handleConnect(conn); }}
                    >
                      <Icon name="terminal" size={13} />
                    </button>
                  {/if}
                  <button
                    class="icon-btn"
                    title={$t('terminal_btn_edit')}
                    onclick={(e) => { e.stopPropagation(); panelConn = conn; }}
                  >
                    <Icon name="pencil" size={13} />
                  </button>
                  <button
                    class="icon-btn danger-soft"
                    title={$t('terminal_btn_delete')}
                    onclick={(e) => { e.stopPropagation(); deleteModal = { open: true, id: conn.id, name: conn.name }; }}
                  >
                    <Icon name="trash-2" size={13} />
                  </button>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if totalPages > 1}
      <div class="pagination">
        <button class="page-btn" disabled={currentPage === 0} onclick={() => (page = currentPage - 1)}>
          <Icon name="chevron-left" size={14} />
        </button>
        {#each Array.from({ length: totalPages }, (_, i) => i) as p_}
          <button
            class="page-btn"
            class:active={p_ === currentPage}
            onclick={() => (page = p_)}
          >{p_ + 1}</button>
        {/each}
        <button class="page-btn" disabled={currentPage === totalPages - 1} onclick={() => (page = currentPage + 1)}>
          <Icon name="chevron-right" size={14} />
        </button>
      </div>
    {/if}
  {/if}
</div>

<Drawer
  open={panelConn !== undefined}
  width="440px"
  title={panelConn ? $t('ssh_form_edit') : $t('ssh_form_new')}
  onclose={() => (panelConn = undefined)}
>
  <SSHConnectionForm
    connection={panelConn}
    onSave={onFormSaved}
    onCancel={() => (panelConn = undefined)}
  />
</Drawer>

{#if editProxy !== undefined}
  <ProxyPanel
    proxy={editProxy}
    onclose={() => (editProxy = undefined)}
    onsaved={onProxySaved}
  />
{/if}

<Modal
  open={deleteModal.open}
  title={$t('terminal_btn_delete')}
  message={$t('terminal_confirm_delete', { name: deleteModal.name })}
  confirmLabel={$t('terminal_btn_delete')}
  cancelLabel={$t('ssh_btn_cancel')}
  variant="danger"
  onconfirm={confirmDelete}
  oncancel={() => (deleteModal = { open: false, id: '', name: '' })}
/>

<style>
  /* панель фильтров — единые примитивы .filter-bar/.search-field/.filter-select/.count-badge из base.css */

  .table-wrap {
    flex: 1; min-height: 0; overflow-y: auto;
    border: 1px solid var(--border); border-radius: var(--radius);
    background: var(--bg-2);
  }

  .ssh-table {
    width: 100%; border-collapse: collapse; font-size: var(--fs-sm);
    table-layout: fixed;
  }

  .ssh-table thead {
    position: sticky; top: 0; z-index: 1;
    background: var(--surface-2); border-bottom: 1px solid var(--border);
  }

  .ssh-table th {
    padding: var(--sp-2) var(--sp-3); text-align: left;
    font-size: var(--fs-2xs); font-weight: 700; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.05em;
    white-space: nowrap;
  }

  .ssh-table td { padding: var(--sp-2) var(--sp-3); border-bottom: 1px solid var(--border); vertical-align: middle; }
  .ssh-row:last-child td { border-bottom: none; }
  .ssh-row:hover td { background: var(--surface-2); }
  .ssh-row { cursor: pointer; }

  .col-num { width: 40px; color: var(--text-3); font-size: var(--fs-xs); }
  .col-name { width: 130px; }
  .col-host { width: 155px; }
  .col-user { width: 115px; }
  .col-auth { width: 130px; }
  .col-proxy { width: 110px; }
  .col-status { width: 52px; text-align: center; }
  /* col-last: no fixed width — takes remaining space */
  .col-actions { width: 124px; }

  .conn-name { font-weight: 600; color: var(--text); display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .auth-tags { display: flex; flex-wrap: wrap; gap: var(--sp-1); align-items: center; }

  code { font-family: monospace; font-size: var(--fs-sm); color: var(--text-2); background: var(--surface-2); padding: 0.1rem 0.35rem; border-radius: 4px; }

  .auth-badge {
    font-size: var(--fs-2xs); font-weight: 700; letter-spacing: 0.04em;
    padding: 0.15rem var(--sp-2); border-radius: 999px;
    border: 1px solid var(--border); background: var(--surface-2); color: var(--text-2);
  }
  .auth-key { background: var(--accent-bg); border-color: color-mix(in srgb, var(--accent) 30%, var(--border)); color: var(--accent); }
  .auth-key_password { background: var(--warn-bg); border-color: color-mix(in srgb, var(--warn-text) 30%, var(--border)); color: var(--warn-text); }

  .tag-2fa { font-size: var(--fs-2xs); font-weight: 700; letter-spacing: 0.04em; padding: 0.15rem var(--sp-2); border-radius: 999px; background: var(--success-bg); border: 1px solid color-mix(in srgb, var(--success) 30%, var(--border)); color: var(--success-text); }
  .tag-proxy { font-size: var(--fs-2xs); font-weight: 700; letter-spacing: 0.04em; padding: 0.15rem var(--sp-2); border-radius: 999px; background: var(--surface-2); border: 1px solid var(--border); color: var(--text-3); }

  .status-dot {
    display: inline-block;
    width: 8px; height: 8px; border-radius: 50%;
  }
  .status-dot-connected { background: var(--success-text); box-shadow: 0 0 5px var(--success-text); }
  .status-dot-connecting {
    background: var(--warn-text);
    animation: dot-pulse 1s ease-in-out infinite;
  }
  .status-dot-idle { background: var(--border-2); }

  @keyframes dot-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .date-cell { display: flex; flex-direction: column; gap: 0.05rem; }
  .date-cell span { color: var(--text-3); font-size: var(--fs-xs); }
  .time-part { color: var(--text-4, var(--text-3)); font-size: var(--fs-2xs); opacity: 0.75; }

  .proxy-chip {
    display: inline-flex; align-items: center;
    background: var(--accent-bg); border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--border));
    color: var(--accent); border-radius: 999px;
    font-size: var(--fs-2xs); font-weight: 700; letter-spacing: 0.03em;
    padding: 0.15rem var(--sp-2); cursor: pointer;
    transition: all 0.15s;
  }
  .proxy-chip:hover { background: color-mix(in srgb, var(--accent) 15%, var(--accent-bg)); }

  .text-muted { color: var(--text-3); font-size: var(--fs-xs); }
  .row-actions { display: flex; gap: var(--sp-1); flex-shrink: 0; }

  .pagination {
    display: flex; align-items: center; gap: var(--sp-1); justify-content: center;
    padding-top: var(--sp-1); flex-shrink: 0;
  }

  .page-btn {
    min-width: 30px; height: 30px; padding: 0 0.4rem;
    display: flex; align-items: center; justify-content: center;
    background: var(--surface-2); border: 1px solid var(--border);
    border-radius: var(--radius-sm); color: var(--text-2);
    font-size: var(--fs-sm); cursor: pointer; transition: all 0.15s;
  }
  .page-btn:hover:not(:disabled) { background: var(--surface); border-color: var(--border-2); color: var(--text); }
  .page-btn.active { background: var(--accent); border-color: var(--accent); color: #fff; }
  .page-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  /* uses global .empty-state */
  .empty-icon { opacity: 0.4; }

</style>
