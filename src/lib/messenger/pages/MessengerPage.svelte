<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { messengerStore } from '../store.svelte';
  import IdentityOnboarding from '../identity/IdentityOnboarding.svelte';
  import IdentityCard from '../identity/IdentityCard.svelte';
  import RelaysPanel from '../relays/RelaysPanel.svelte';
  import DebugFeed from '../debug/DebugFeed.svelte';

  onMount(() => { messengerStore.refresh().catch(() => {}); });

  let s = $derived(messengerStore.status);
</script>

<div class="page">
  <div class="page-header">
    <div class="page-title-group">
      <h1>{$t('msg_title')} <span class="alpha-badge">{$t('msg_alpha_badge')}</span></h1>
      <p class="page-sub">{$t('msg_intro')}</p>
    </div>
  </div>

  {#if !s}
    <div class="empty-state">{$t('loading')}</div>
  {:else if !s.compiled}
    <div class="error-msg">{$t('msg_status_not_compiled')}</div>
  {:else if s.error}
    <div class="error-msg">{$t('msg_status_error', { error: s.error })}</div>
  {:else if s.runtime}
    <div class="stack">
      {#if messengerStore.identity}
        <IdentityCard identity={messengerStore.identity} />
      {:else}
        <IdentityOnboarding />
      {/if}

      <RelaysPanel />

      <DebugFeed />

      <div class="card status-card">
        <div class="card-title">{$t('msg_status_title')}</div>
        <dl class="status-grid">
          <dt>{$t('msg_status_version')}</dt><dd><code>{s.runtime.version}</code></dd>
          <dt>{$t('msg_status_schema')}</dt><dd><code>{s.runtime.schema_version}</code></dd>
          <dt>{$t('msg_status_data_dir')}</dt><dd><code>{s.runtime.data_dir}</code></dd>
          <dt>{$t('msg_status_secrets')}</dt>
          <dd>{s.runtime.secrets_unlocked ? $t('msg_status_secrets_unlocked') : $t('msg_status_secrets_locked')}</dd>
          <dt>{$t('msg_status_identity')}</dt>
          <dd>{s.runtime.identity_present ? $t('msg_status_identity_present') : $t('msg_status_identity_none')}</dd>
          <dt>{$t('msg_status_relays')}</dt>
          <dd>{$t('msg_status_relays_value', { connected: String(s.runtime.relays_connected), total: String(s.runtime.relays_total) })}{s.runtime.silent_mode ? ` · ${$t('msg_relays_silent')}` : ''}</dd>
          <dt>{$t('msg_status_session')}</dt>
          <dd>{s.runtime.session_active ? $t('msg_status_session_yes') : $t('msg_status_session_no')}</dd>
          <dt>{$t('msg_status_ingress')}</dt>
          <dd>{$t('msg_status_ingress_value', { received: String(s.runtime.ingress.received), dm: String(s.runtime.ingress.dm), dup: String(s.runtime.ingress.duplicates), ignored: String(s.runtime.ingress.ignored) })}</dd>
          <dt>{$t('msg_status_outbox')}</dt>
          <dd>{String(s.runtime.outbox_pending)}</dd>
        </dl>
      </div>
    </div>
  {/if}
</div>

<style>
  .page-title-group { display: flex; flex-direction: column; gap: 6px; }
  .alpha-badge {
    display: inline-block; vertical-align: middle; margin-left: 8px;
    font-size: var(--fs-2xs); font-weight: var(--fw-bold); text-transform: uppercase; letter-spacing: 0.6px;
    padding: 3px 8px; border-radius: var(--radius-sm);
    background: var(--accent-tint); color: var(--accent-text-2);
  }
  .stack { display: flex; flex-direction: column; gap: var(--sp-4); }
  .status-card { max-width: 640px; }
  .status-grid {
    display: grid; grid-template-columns: max-content 1fr; gap: var(--sp-2) var(--sp-4);
    margin: 0; font-size: var(--fs-sm);
  }
  .status-grid dt { color: var(--text-3); }
  .status-grid dd { margin: 0; color: var(--text-body); word-break: break-all; }
  code {
    font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--text-body);
    background: var(--surface-2); padding: 3px 8px; border-radius: 7px;
  }
</style>
