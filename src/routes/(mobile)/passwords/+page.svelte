<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import Icon from '$lib/Icon.svelte';
  import PasswordList from '$lib/components/passwords/PasswordList.svelte';
  import PasswordLockBadge from '$lib/components/passwords/PasswordLockBadge.svelte';
  import { api, formatError, onSyncChanged } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { userLabels } from '$lib/entity-tags';
  import type { PasswordEntry } from '$lib/types';

  let entries = $state<PasswordEntry[]>([]);
  let search = $state('');
  let error = $state('');

  const canAdd = $derived(!notesLock.locked && notesLock.status.vault !== 'mismatch');

  async function load() {
    try {
      entries = await api.passwords.list();
    } catch (e) {
      error = formatError(e);
    }
  }

  const filtered = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((entry) => {
      const labels = userLabels(entry.tags).join(' ');
      const profiles = entry.tags
        .filter((tag) => tag.startsWith('profile:'))
        .map((tag) => profilesStore.list.find((p) => p.id === tag.slice('profile:'.length))?.name ?? '')
        .join(' ');
      return `${entry.title} ${entry.username ?? ''} ${entry.url ?? ''} ${labels} ${profiles}`.toLowerCase().includes(q);
    });
  });

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
    void profilesStore.ensureLoaded();
    void load();
    const unlisten = onSyncChanged(['password', 'password_vault'], () => void load());
    return () => { void unlisten.then((fn) => fn()); };
  });
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    <h1 class="m-title title-with-badge">{$t('pw_title')}<PasswordLockBadge size={11} /></h1>
    {#if canAdd}
      <a class="m-ibtn" href="/passwords/add" aria-label={$t('pw_btn_add')}><Icon name="plus" size={24} /></a>
    {/if}
  </div>
  <div class="m-search">
    <div class="field">
      <Icon name="search" size={16} />
      <input type="search" bind:value={search} placeholder={$t('pw_search')} />
    </div>
  </div>
  <div class="m-body">
    {#if error}<div class="m-error">{error}</div>{/if}
    {#if entries.length === 0}
      <div class="m-empty">
        <Icon name="lock" size={40} />
        <p>{$t('pw_empty')}</p>
      </div>
    {:else if filtered.length === 0}
      <div class="m-empty"><p>{$t('pw_empty')}</p></div>
    {:else}
      <PasswordList
        entries={filtered}
        onopen={(id) => goto(`/passwords/${id}`)}
        onedit={(entry) => goto(`/passwords/${entry.id}`)}
      />
    {/if}
  </div>
</div>

<style>
  .title-with-badge { display: flex; align-items: center; gap: 2px; }
</style>
