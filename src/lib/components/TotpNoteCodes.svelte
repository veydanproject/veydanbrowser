<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import type { TotpCode, TotpEntry } from '$lib/types';

  interface Props {
    entries: TotpEntry[];
  }

  let { entries }: Props = $props();

  let codes = $state<Map<string, TotpCode>>(new Map());
  let open = $state(false);
  let copiedId = $state('');
  let clipTimer: ReturnType<typeof setTimeout>;
  let copyTimer: ReturnType<typeof setTimeout>;

  const RADIUS = 10;
  const CIRC = 2 * Math.PI * RADIUS;
  const ids = $derived(entries.map((entry) => entry.id).join(','));

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

  async function refresh() {
    if (entries.length === 0) {
      codes = new Map();
      return;
    }
    try {
      const list = await api.totp.generateCodes(entries.map((entry) => entry.id));
      codes = new Map(list.map((code) => [code.id, code]));
    } catch {
      /* keep the previous codes while the next tick retries */
    }
  }

  const folded = $derived(entries.length > 3 && !open);

  let seen = '';
  $effect(() => {
    if (ids === seen) return;
    seen = ids;
    open = false;
    void refresh();
  });

  onMount(() => {
    const tick = setInterval(refresh, 1000);
    return () => {
      clearInterval(tick);
      clearTimeout(clipTimer);
      clearTimeout(copyTimer);
    };
  });

  async function copy(entry: TotpEntry) {
    const code = codes.get(entry.id);
    if (!code) return;
    await navigator.clipboard.writeText(code.code);
    copiedId = '';
    requestAnimationFrame(() => (copiedId = entry.id));
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copiedId = ''), 1600);
    clearTimeout(clipTimer);
    clipTimer = setTimeout(() => navigator.clipboard.writeText('').catch(() => {}), 30_000);
  }
</script>

{#if entries.length > 0}
  <div class="codes">
    {#if entries.length > 3}
      <button type="button" class="fold" aria-expanded={open} onclick={() => (open = !open)}>
        <Icon name={open ? 'chevron-down' : 'chevron-right'} size={16} />
        <span class="fold-name">{$t('totp_title')}</span>
        <span class="count">{entries.length}</span>
      </button>
    {/if}
    {#if !folded}
    {#each entries as entry (entry.id)}
      {@const code = codes.get(entry.id)}
      {@const copied = copiedId === entry.id}
      <button type="button" class="row" class:copied onclick={() => copy(entry)} aria-label={$t('totp_copy')}>
        <span class="name">{entry.issuer || entry.name}</span>
        <span class="code" class:pop={copied} style:color={copied ? 'var(--success-text)' : ringColor(code)}>{code ? formatCode(code.code) : '••• •••'}</span>
        <svg class="ring" viewBox="0 0 28 28" width="22" height="22" aria-hidden="true">
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
    {/if}
  </div>
{/if}

<style>
  .codes {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin-bottom: var(--sp-3);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    min-height: 48px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--m-card-border, var(--border));
    border-radius: var(--m-radius, var(--radius-md));
    background: var(--m-card, var(--surface-2));
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .row:hover { border-color: var(--border-2); }
  .row.copied { border-color: var(--success-border, var(--success-text)); }
  .fold {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    min-height: 44px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--m-card-border, var(--border));
    border-radius: var(--m-radius, var(--radius-md));
    background: var(--m-card, var(--surface-2));
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .fold-name { flex: 1; font-size: 14px; font-weight: 700; }
  .count {
    font-family: var(--font-mono);
    font-size: 12px;
    font-weight: 700;
    color: var(--text-2);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 700;
  }
  .code {
    font-family: var(--font-mono);
    font-size: 16px;
    font-weight: 700;
    letter-spacing: 0.04em;
    white-space: nowrap;
  }
  .code.pop { animation: code-pop 0.4s ease; }
  @keyframes code-pop {
    0% { transform: scale(1); }
    40% { transform: scale(1.12); }
    100% { transform: scale(1); }
  }
  .ring { transform: rotate(-90deg); flex-shrink: 0; }
  .ring circle { fill: none; stroke-width: 3; }
  .track { stroke: var(--border); }
  .progress { transition: stroke-dashoffset 0.9s linear; }
</style>
