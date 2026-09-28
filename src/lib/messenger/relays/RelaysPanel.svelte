<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import { messengerStore } from '../store.svelte';
  import { messengerError, type RelayState } from '../api';

  let newUrl = $state('');
  let busy = $state(false);
  let error = $state('');

  onMount(() => { messengerStore.startListeners().catch(() => {}); });

  const stateLabel: Record<RelayState, string> = {
    connected: 'msg_relay_state_connected',
    connecting: 'msg_relay_state_connecting',
    disconnected: 'msg_relay_state_disconnected',
    paused: 'msg_relay_state_paused',
  };

  async function run(fn: () => Promise<unknown>) {
    error = '';
    busy = true;
    try { await fn(); }
    catch (e) { error = messengerError(e); }
    finally { busy = false; }
  }

  async function add() {
    const url = newUrl.trim();
    if (!url) return;
    await run(async () => { await messengerStore.addRelay(url); newUrl = ''; });
  }
</script>

<div class="card relays">
  <div class="card-title"><Icon name="globe" size={16} /> {$t('msg_relays_title')}</div>
  <p class="muted">{$t('msg_relays_text')}</p>

  {#if messengerStore.manifest}
    <div class="controls">
      <label class="control">
        <span>{$t('msg_relays_region')}</span>
        <select
          value={messengerStore.manifest.region}
          disabled={busy}
          onchange={(e) => run(() => messengerStore.setRegion((e.currentTarget as HTMLSelectElement).value))}
        >
          {#each messengerStore.manifest.regions as r}
            <option value={r}>{r}</option>
          {/each}
          {#if !messengerStore.manifest.regions.includes(messengerStore.manifest.region)}
            <option value={messengerStore.manifest.region}>{messengerStore.manifest.region}</option>
          {/if}
        </select>
      </label>
      <div class="control silent">
        <div class="control-info">
          <span>{$t('msg_relays_silent')}</span>
          <span class="muted small">{$t('msg_relays_silent_hint')}</span>
        </div>
        <button
          class="toggle"
          class:on={messengerStore.manifest.silent_mode}
          disabled={busy}
          onclick={() => run(() => messengerStore.setSilent(!messengerStore.manifest?.silent_mode))}
          aria-pressed={messengerStore.manifest.silent_mode}
          aria-label={$t('msg_relays_silent')}
        ></button>
      </div>
      <span class="meta">
        {$t('msg_relays_manifest', {
          serial: String(messengerStore.manifest.serial ?? '—'),
          date: messengerStore.manifest.issued_at ? new Date(messengerStore.manifest.issued_at * 1000).toLocaleDateString() : '—',
        })}
      </span>
    </div>
  {/if}

  <ul class="list">
    {#each messengerStore.relays as r (r.url)}
      <li class="row" class:off={!r.enabled}>
        <span class="dot {r.state}" title={$t(stateLabel[r.state] as 'msg_relay_state_connected')}></span>
        <div class="info">
          <code>{r.url}</code>
          <span class="meta">
            {r.source === 'manifest' ? $t('msg_relay_source_manifest') : $t('msg_relay_source_user')}
            {#if r.relay_id} · {r.relay_id}{/if}
            {#if r.auth_type === 'api_key'} · {$t('msg_relay_auth_api_key')}{:else if r.auth_type === 'nip42'} · NIP-42{/if}
            · {$t(stateLabel[r.state] as 'msg_relay_state_connected')}
          </span>
        </div>
        <button
          class="toggle"
          class:on={r.enabled}
          disabled={busy}
          onclick={() => run(() => messengerStore.setRelayEnabled(r.url, !r.enabled))}
          aria-pressed={r.enabled}
          aria-label={$t('msg_relay_enabled')}
        ></button>
        {#if r.source === 'user'}
          <button class="icon-btn danger-soft" title={$t('msg_relay_remove')} disabled={busy} onclick={() => run(() => messengerStore.removeRelay(r.url))}>
            <Icon name="trash-2" size={13} />
          </button>
        {:else}
          <span class="icon-spacer"></span>
        {/if}
      </li>
    {/each}
  </ul>

  <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <input type="text" bind:value={newUrl} placeholder="wss://relay.example.com" spellcheck="false" disabled={busy} />
    <button class="btn btn-ghost" type="submit" disabled={busy || !newUrl.trim()}>
      <Icon name="plus" size={14} />{$t('msg_relay_add')}
    </button>
  </form>
  {#if error}<div class="error-msg">{error}</div>{/if}
</div>

<style>
  .relays { max-width: 640px; display: flex; flex-direction: column; gap: var(--sp-3); }
  .card-title { display: flex; align-items: center; gap: var(--sp-2); }
  .muted { color: var(--text-2); font-size: var(--fs-sm); margin: 0; }
  .small { font-size: var(--fs-xs); }
  .meta { color: var(--text-3); font-size: var(--fs-xs); }
  .controls { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-3) var(--sp-4); }
  .control { display: flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-sm); }
  .control select {
    font: inherit; color: var(--text); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 5px 8px;
  }
  .control-info { display: flex; flex-direction: column; }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  .row {
    display: flex; align-items: center; gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--radius-sm);
    background: var(--surface);
  }
  .row.off { opacity: 0.6; }
  .info { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  code { font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--text-body); word-break: break-all; }
  .dot { width: 9px; height: 9px; border-radius: 50%; flex-shrink: 0; background: var(--text-3); }
  .dot.connected { background: var(--success-text); }
  .dot.connecting { background: var(--color-warning); }
  .dot.paused { background: var(--danger-text); }
  .icon-spacer { width: 28px; }
  .add { display: flex; gap: var(--sp-2); }
  .add input {
    flex: 1; font: inherit; font-family: var(--font-mono); font-size: var(--fs-xs); color: var(--text);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 8px 10px;
  }
  .add input:focus { outline: none; border-color: var(--accent-border); }
</style>
