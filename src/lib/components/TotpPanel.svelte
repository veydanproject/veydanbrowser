<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { totpStore } from '$lib/store/totp.svelte';
  import TotpList from './TotpList.svelte';
  import TotpAddModal from './TotpAddModal.svelte';

  interface Props {
    profileId: string;
  }

  let { profileId }: Props = $props();

  let showAdd = $state(false);

  const profileTag = $derived(`profile:${profileId}`);
  const entries = $derived(totpStore.byProfile(profileId));

  onMount(() => {
    totpStore.ensureLoaded();
  });
</script>

<div class="totp-panel">
  <div class="panel-toolbar">
    <span class="toolbar-count">{entries.length} TOTP</span>
    <button class="btn-ghost btn-sm" onclick={() => (showAdd = true)}>
      + {$t('totp_btn_add')}
    </button>
  </div>

  {#if totpStore.loading && !totpStore.loaded}
    <div class="loading">{$t('loading')}</div>
  {:else}
    <TotpList
      {entries}
      emptyText={$t('totp_empty_profile')}
      onrequestAdd={() => (showAdd = true)}
    />
  {/if}
</div>

{#if showAdd}
  <TotpAddModal
    initialTags={[profileTag]}
    onclose={() => (showAdd = false)}
  />
{/if}

<style>
  .totp-panel {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }

  .panel-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .toolbar-count {
    font-size: var(--fs-sm);
    color: var(--text-2);
  }

  .loading {
    font-size: var(--fs-sm);
    color: var(--text-2);
    padding: var(--sp-4) 0;
    text-align: center;
  }

  .btn-ghost {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 0.35rem var(--sp-3);
    font-size: var(--fs-sm);
    cursor: pointer;
    color: var(--text-2);
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    transition: all 0.15s;
  }

  .btn-ghost:hover { border-color: var(--accent); color: var(--accent); }
  .btn-sm { padding: 0.3rem 0.65rem; }
</style>
