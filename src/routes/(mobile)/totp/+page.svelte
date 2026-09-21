<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import type { TotpEntry, TotpCode } from '$lib/types';
  import { api, formatError, onSyncChanged } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { NAV_COLORS } from '$lib/mobile/nav-colors';
  import { longpress } from '$lib/mobile/longpress';
  import BottomSheet from '$lib/components/mobile/BottomSheet.svelte';

  let entries = $state<TotpEntry[]>([]);
  let codes = $state<Map<string, TotpCode>>(new Map());
  let error = $state('');
  let toast = $state('');
  let menu = $state<TotpEntry | null>(null);
  let toastTimer: ReturnType<typeof setTimeout>;
  let clipTimer: ReturnType<typeof setTimeout>;
  let tick: ReturnType<typeof setInterval>;

  const RADIUS = 12;
  const CIRC = 2 * Math.PI * RADIUS;

  function offset(code: TotpCode | undefined, period: number): number {
    if (!code) return CIRC;
    return CIRC * (1 - Math.min(1, code.seconds_left / period));
  }

  function ringColor(code: TotpCode | undefined): string {
    if (!code || code.seconds_left <= 5) return 'var(--danger-text)';
    if (code.seconds_left <= 10) return 'var(--warn-text)';
    return 'var(--accent)';
  }

  function formatCode(code: string): string {
    const half = Math.ceil(code.length / 2);
    return `${code.slice(0, half)} ${code.slice(half)}`;
  }

  /** Stable colour per issuer from a string hash. */
  function avatarColor(s: string): string {
    let h = 0;
    for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return NAV_COLORS[h % NAV_COLORS.length];
  }

  function label(entry: TotpEntry): string {
    return entry.issuer || entry.name;
  }

  async function load() {
    try {
      entries = await api.totp.list();
      await refreshCodes();
    } catch (e) {
      error = formatError(e);
    }
  }

  async function refreshCodes() {
    if (entries.length === 0) return;
    try {
      const list = await api.totp.generateCodes(entries.map((e) => e.id));
      codes = new Map(list.map((c) => [c.id, c]));
    } catch (e) {
      error = formatError(e);
    }
  }

  function showToast(msg: string) {
    clearTimeout(toastTimer);
    toast = msg;
    toastTimer = setTimeout(() => (toast = ''), 1600);
  }

  async function copy(entry: TotpEntry) {
    const code = codes.get(entry.id);
    if (!code) return;
    await navigator.clipboard.writeText(code.code);
    showToast($t('totp_copied'));
    // Clear clipboard after 30s so the code does not linger.
    clearTimeout(clipTimer);
    clipTimer = setTimeout(() => navigator.clipboard.writeText('').catch(() => {}), 30_000);
  }

  async function remove() {
    const entry = menu;
    menu = null;
    if (!entry || !confirm($t('totp_delete_confirm', { name: entry.name }))) return;
    try {
      await api.totp.delete(entry.id);
      await load();
    } catch (e) {
      error = formatError(e);
    }
  }

  onMount(() => {
    load();
    tick = setInterval(refreshCodes, 1000);
    api.sync.trigger().catch(() => {});
    const unlisten = onSyncChanged(['totp'], load);
    return () => unlisten.then((f) => f());
  });
  onDestroy(() => {
    clearInterval(tick);
    clearTimeout(toastTimer);
    clearTimeout(clipTimer);
  });
</script>

<div class="m-page totp">
  <div class="m-header">
    <a class="m-ibtn" href="/" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    <h1 class="m-title">{$t('app_totp')}</h1>
    <a class="m-ibtn" href="/totp/add" aria-label={$t('totp_btn_add')}><Icon name="plus" size={24} /></a>
  </div>

  <div class="m-body">
  {#if error}
    <div class="m-error">{error}</div>
  {/if}

  {#if entries.length === 0}
    <div class="m-empty">
      <Icon name="shield" size={40} />
      <p>{$t('totp_empty')}</p>
      <span>{$t('totp_empty_hint')}</span>
    </div>
  {:else}
    <div class="m-cards">
      {#each entries as entry (entry.id)}
        {@const code = codes.get(entry.id)}
        <button class="m-card item" onclick={() => copy(entry)} {@attach longpress(() => (menu = entry))}>
          <span class="m-avatar" style:--c={avatarColor(label(entry))}>{label(entry).charAt(0).toUpperCase()}</span>
          <span class="meta">
            <span class="name">{label(entry)}</span>
            {#if entry.issuer}<span class="account">{entry.name}</span>{/if}
          </span>
          <span class="code">{code ? formatCode(code.code) : '••• •••'}</span>
          <svg class="ring" viewBox="0 0 28 28" width="24" height="24">
            <circle cx="14" cy="14" r={RADIUS} class="track" />
            <circle
              cx="14"
              cy="14"
              r={RADIUS}
              class="progress"
              style="stroke: {ringColor(code)}; stroke-dasharray: {CIRC}; stroke-dashoffset: {offset(code, entry.period)}"
            />
          </svg>
        </button>
      {/each}
    </div>
  {/if}

  <a class="m-btn-grad add" href="/totp/add">
    <Icon name="plus" size={18} />
    {$t('totp_add_account')}
  </a>

  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
  </div>
</div>

<BottomSheet open={!!menu} title={menu ? label(menu) : ''} onclose={() => (menu = null)}>
  <div class="m-list">
    <button type="button" class="m-row" onclick={() => { if (menu) copy(menu); menu = null; }}>
      <Icon name="copy" size={20} /><span class="m-row-label">{$t('pwgen_btn_copy')}</span>
    </button>
    <button type="button" class="m-row" onclick={remove}>
      <Icon name="trash-2" size={20} /><span class="m-row-label danger">{$t('totp_delete')}</span>
    </button>
  </div>
</BottomSheet>

<style>
  .totp { display: flex; flex-direction: column; min-height: 100%; }
  .item {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 64px;
    padding: var(--sp-3) var(--sp-4);
    color: var(--text);
    font: inherit;
    text-align: left;
    width: 100%;
  }
  .item:active { background: var(--surface-2); }
  .meta { flex: 1; display: flex; flex-direction: column; min-width: 0; gap: 1px; }
  .name { font-size: 15px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .account { font-size: 13px; color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .code {
    font-family: var(--font-mono);
    font-size: 20px;
    font-weight: 700;
    letter-spacing: 0.04em;
    color: var(--text);
    white-space: nowrap;
  }
  .ring { transform: rotate(-90deg); flex-shrink: 0; }
  .ring circle { fill: none; stroke-width: 3; }
  .track { stroke: var(--border); }
  .progress { transition: stroke-dashoffset 0.9s linear; }
  .add { margin-top: auto; }
  .m-cards { padding-bottom: var(--sp-6); }
  .danger { color: var(--danger-text); }
</style>
