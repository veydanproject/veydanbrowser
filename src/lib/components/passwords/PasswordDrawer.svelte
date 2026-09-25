<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { userLabels } from '$lib/entity-tags';
  import Drawer from '$lib/components/ui/Drawer.svelte';
  import Icon from '$lib/Icon.svelte';
  import PasswordList from './PasswordList.svelte';
  import PasswordCard from './PasswordCard.svelte';
  import PasswordEditModal from './PasswordEditModal.svelte';
  import PasswordLockBadge from './PasswordLockBadge.svelte';

  interface Props {
    open?: boolean;
    context?: 'global' | 'workspace';
    contextId?: string;
  }

  let { open = $bindable(false), context = 'global', contextId }: Props = $props();

  let search = $state('');
  let selectedId = $state<string | null>(null);
  let editingId = $state<string | null>(null);
  let creating = $state(false);
  /** null shows every workspace */
  let activeWorkspace = $state<string | null>(null);

  const canWrite = $derived(!notesLock.locked && notesLock.status.vault !== 'mismatch');
  const selected = $derived(passwordStore.list.find((entry) => entry.id === selectedId) ?? null);
  const editingEntry = $derived(passwordStore.list.find((entry) => entry.id === editingId) ?? null);

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
    void passwordStore.ensureLoaded();
    void profilesStore.ensureLoaded();
    void workspacesStore.ensureLoaded();
    void notesStore.ensureLoaded();
    void totpStore.ensureLoaded();
  });

  $effect(() => {
    if (!notesLock.locked && passwordStore.loaded) {
      void passwordStore.refresh();
    }
  });

  $effect(() => {
    if (open) activeWorkspace = context === 'workspace' && contextId ? contextId : null;
  });

  $effect(() => {
    if (!open) {
      selectedId = null;
      editingId = null;
      creating = false;
      passwordStore.createRequest = false;
    }
  });

  $effect(() => {
    if (!passwordStore.createRequest || !open || !notesLock.ready) return;
    passwordStore.createRequest = false;
    if (canWrite) creating = true;
  });

  $effect(() => {
    const id = passwordStore.openId;
    if (!id || !open) return;
    selectedId = id;
    passwordStore.openId = null;
  });

  const filtered = $derived.by(() => {
    let list = passwordStore.list;
    if (activeWorkspace) {
      const profileIds = profilesStore.byWorkspace(activeWorkspace).map((profile) => profile.id);
      list = passwordStore.byWorkspace(activeWorkspace, profileIds);
    }
    const q = search.trim().toLowerCase();
    if (!q) return list;
    return list.filter((entry) => {
      const labels = userLabels(entry.tags).join(' ');
      const profiles = entry.tags
        .filter((tag) => tag.startsWith('profile:'))
        .map((tag) => profilesStore.list.find((p) => p.id === tag.slice('profile:'.length))?.name ?? '')
        .join(' ');
      const notes = entry.tags
        .filter((tag) => tag.startsWith('note:'))
        .map((tag) => notesStore.list.find((n) => n.id === tag.slice('note:'.length))?.title ?? '')
        .join(' ');
      const totp = entry.totp_ids
        .map((id) => totpStore.list.find((t) => t.id === id))
        .map((t) => (t ? `${t.issuer ?? ''} ${t.name}` : ''))
        .join(' ');
      return `${entry.title} ${entry.username ?? ''} ${entry.url ?? ''} ${labels} ${profiles} ${notes} ${totp}`
        .toLowerCase()
        .includes(q);
    });
  });

  function toggleWorkspace(id: string) {
    activeWorkspace = activeWorkspace === id ? null : id;
  }

  function initialTags(): string[] {
    return context === 'workspace' && contextId ? [`workspace:${contextId}`] : [];
  }
</script>

<Drawer bind:open title={$t('pw_title')} width="var(--drawer-w-lg)">
  {#snippet titleBadge()}
    <PasswordLockBadge />
  {/snippet}

  {#snippet actions()}
    <span class="count">{filtered.length}</span>
    {#if canWrite && !selected}
      <button class="icon-btn" title={$t('pw_btn_add')} onclick={() => (creating = true)}>
        <Icon name="plus" size={14} />
      </button>
    {/if}
  {/snippet}

  {#snippet subheader()}
    {#if !selected}
      <div class="panel-search">
        <div class="search-wrap">
          <span class="search-icon"><Icon name="search" size={13} /></span>
          <input class="search-input" bind:value={search} placeholder={$t('pw_search')} />
        </div>
      </div>
      {#if workspacesStore.list.length}
        <div class="ws-chips">
          {#if context === 'workspace'}
            <button type="button" class="chip-btn" class:active={activeWorkspace === null} onclick={() => (activeWorkspace = null)}>
              {$t('totp_show_all')}
            </button>
          {/if}
          {#each workspacesStore.list as ws (ws.id)}
            <button type="button" class="chip-btn" class:active={activeWorkspace === ws.id} onclick={() => toggleWorkspace(ws.id)}>
              <span class="ws-dot" style:background={ws.color}></span>
              {ws.name}
            </button>
          {/each}
        </div>
      {/if}
    {/if}
  {/snippet}

  {#if selected}
    <button type="button" class="btn btn-ghost btn-sm back" onclick={() => (selectedId = null)}>
      <Icon name="chevron-left" size={14} /> {$t('pw_back')}
    </button>
    <PasswordCard entry={selected} onedit={() => (editingId = selected.id)} ondeleted={() => (selectedId = null)} />
  {:else if passwordStore.loading && !passwordStore.loaded}
    <p class="muted">{$t('loading')}</p>
  {:else if filtered.length === 0}
    <p class="muted">{$t('pw_empty')}</p>
  {:else}
    <PasswordList entries={filtered} onopen={(id) => (selectedId = id)} onedit={(entry) => (editingId = entry.id)} />
  {/if}
</Drawer>

{#if creating}
  <PasswordEditModal initialTags={initialTags()} onclose={() => (creating = false)} />
{/if}
{#if editingEntry}
  <PasswordEditModal entry={editingEntry} onclose={() => (editingId = null)} />
{/if}

<style>
  .count {
    background: var(--accent-tint);
    color: var(--accent-text-2);
    border: 1px solid var(--accent-tint-border);
    border-radius: var(--radius-sm);
    font-size: var(--fs-2xs);
    font-family: var(--font-mono);
    padding: 0.05rem 0.4rem;
    font-weight: var(--fw-semibold);
  }
  .panel-search { padding: var(--sp-3) var(--sp-4) 0; }
  .search-wrap { position: relative; }
  .search-icon {
    position: absolute;
    left: 0.6rem;
    top: 50%;
    transform: translateY(-50%);
    color: var(--text-2);
    display: flex;
    pointer-events: none;
  }
  .search-input {
    width: 100%;
    box-sizing: border-box;
    height: 34px;
    padding: 0 10px 0 2rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-3);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
  }
  .search-input:focus { outline: none; border-color: var(--accent-border); }
  .ws-chips {
    display: flex;
    gap: 0.35rem;
    padding: 0.6rem var(--sp-4);
    flex-wrap: wrap;
    border-bottom: 1px solid var(--border);
  }
  .chip-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: var(--sp-1) 0.6rem;
    font-size: var(--fs-xs);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--text-2);
    cursor: pointer;
  }
  .chip-btn.active {
    background: var(--accent-tint);
    border-color: var(--accent-tint-border);
    color: var(--accent-text);
  }
  .ws-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
  .muted { color: var(--text-2); font-size: 0.85rem; }
  .back { margin-bottom: var(--sp-3); }
</style>
