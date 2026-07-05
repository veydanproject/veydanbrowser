<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { totpStore } from '$lib/store/totp.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import type { TotpEntry, TotpCode } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Modal from '$lib/Modal.svelte';

  interface Props {
    entries: TotpEntry[];
    showProfileBadge?: boolean;
    onrequestAdd?: () => void;
    emptyText?: string;
  }

  let { entries, showProfileBadge = false, onrequestAdd, emptyText }: Props = $props();

  let codes = $state<Map<string, TotpCode>>(new Map());
  let copiedId = $state('');
  let copyTimer: ReturnType<typeof setTimeout>;
  let clipClearTimer: ReturnType<typeof setTimeout>;
  let deleteModal = $state<{ open: boolean; id: string; name: string }>({ open: false, id: '', name: '' });

  // Countdown ring
  const RADIUS = 10;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

  function strokeOffset(secondsLeft: number, period: number): number {
    const frac = Math.max(0, Math.min(1, secondsLeft / period));
    return CIRCUMFERENCE * (1 - frac);
  }

  function ringColor(secondsLeft: number): string {
    if (secondsLeft <= 5) return 'var(--danger, #ef4444)';
    if (secondsLeft <= 10) return 'var(--warning, #f59e0b)';
    return 'var(--accent)';
  }

  async function loadCodes() {
    if (entries.length === 0) return;
    const ids = entries.map((e) => e.id);
    try {
      const results = await api.totp.generateCodes(ids);
      const map = new Map<string, TotpCode>();
      for (const r of results) map.set(r.id, r);
      codes = map;
    } catch {}
  }

  let interval: ReturnType<typeof setInterval>;

  onMount(async () => {
    await loadCodes();
    interval = setInterval(loadCodes, 1000);
  });

  onDestroy(() => {
    clearInterval(interval);
    clearTimeout(copyTimer);
    clearTimeout(clipClearTimer);
  });

  $effect(() => {
    // Reload when entries change
    entries;
    loadCodes();
  });

  async function copy(entry: TotpEntry) {
    const code = codes.get(entry.id);
    if (!code) return;
    try {
      await navigator.clipboard.writeText(code.code);
      copiedId = entry.id;
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedId = ''), 2000);

      // Auto-clear clipboard after 30s
      clearTimeout(clipClearTimer);
      clipClearTimer = setTimeout(() => {
        navigator.clipboard.writeText('').catch(() => {});
      }, 30_000);
    } catch {}
  }

  function profileName(tags: string[]): string | null {
    for (const tag of tags) {
      if (tag.startsWith('profile:')) {
        const pid = tag.slice('profile:'.length);
        const p = profilesStore.list.find((x) => x.id === pid);
        return p?.name ?? null;
      }
    }
    return null;
  }

  async function confirmDelete() {
    await api.totp.delete(deleteModal.id);
    await totpStore.refresh();
    deleteModal = { open: false, id: '', name: '' };
  }

  function formatCode(code: string): string {
    // Split 6-digit code as "123 456", 8-digit as "1234 5678"
    if (code.length === 6) return `${code.slice(0, 3)} ${code.slice(3)}`;
    if (code.length === 8) return `${code.slice(0, 4)} ${code.slice(4)}`;
    return code;
  }
</script>

{#if entries.length === 0}
  <div class="empty">
    <span style="opacity:0.3;display:flex"><Icon name="shield-off" size={28} /></span>
    <span>{emptyText ?? $t('totp_empty')}</span>
    {#if onrequestAdd}
      <button class="btn-ghost btn-sm" onclick={onrequestAdd}>
        <Icon name="plus" size={13} /> {$t('totp_btn_add')}
      </button>
    {/if}
  </div>
{:else}
  <div class="list">
    {#each entries as entry (entry.id)}
      {@const code = codes.get(entry.id)}
      {@const pname = showProfileBadge ? profileName(entry.tags) : null}
      <div class="entry">
        <div class="entry-info">
          <div class="entry-name">
            {entry.name}
            {#if pname}
              <span class="profile-badge">{pname}</span>
            {/if}
          </div>
          {#if entry.issuer}
            <div class="entry-issuer">{entry.issuer}</div>
          {/if}
        </div>

        <div class="entry-code">
          <svg class="ring" width="28" height="28" viewBox="-2 -2 28 28">
            <circle cx="12" cy="12" r={RADIUS} fill="none" stroke="var(--border)" stroke-width="2.5" />
            {#if code}
              <circle
                cx="12" cy="12" r={RADIUS}
                fill="none"
                stroke={ringColor(code.seconds_left)}
                stroke-width="2.5"
                stroke-dasharray={CIRCUMFERENCE}
                stroke-dashoffset={strokeOffset(code.seconds_left, entry.period)}
                stroke-linecap="round"
                transform="rotate(-90 12 12)"
                style="transition: stroke-dashoffset 0.9s linear, stroke 0.3s"
              />
            {/if}
          </svg>
          <span class="code-value" style={code ? `color: ${ringColor(code.seconds_left)}` : ''}>
            {code ? formatCode(code.code) : '••• •••'}
          </span>
        </div>

        <div class="entry-actions">
          <button
            class="icon-btn"
            class:success={copiedId === entry.id}
            onclick={() => copy(entry)}
            title={$t('totp_copy')}
            disabled={!code}
          >
            <Icon name={copiedId === entry.id ? 'check' : 'copy'} size={13} />
          </button>
          <button
            class="icon-btn danger-soft"
            onclick={() => (deleteModal = { open: true, id: entry.id, name: entry.name })}
            title="Delete"
          >
            <Icon name="trash" size={13} />
          </button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<Modal
  open={deleteModal.open}
  title="Delete TOTP"
  message={$t('totp_delete_confirm', { name: deleteModal.name })}
  confirmLabel="Delete"
  cancelLabel="Cancel"
  variant="danger"
  onconfirm={confirmDelete}
  oncancel={() => (deleteModal = { open: false, id: '', name: '' })}
/>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .entry {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 0.6rem var(--sp-3);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    transition: border-color 0.15s;
  }

  .entry:hover { border-color: var(--border-2, var(--accent)); }

  .entry-info {
    flex: 1;
    min-width: 0;
  }

  .entry-name {
    font-size: var(--fs-base);
    font-weight: 500;
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
  }

  .entry-issuer {
    font-size: var(--fs-xs);
    color: var(--text-2);
    margin-top: 0.1rem;
  }

  .profile-badge {
    background: var(--accent-bg);
    border: 1px solid var(--accent);
    color: var(--accent);
    border-radius: 999px;
    font-size: var(--fs-2xs);
    padding: 0.1rem 0.4rem;
    font-weight: 400;
  }

  .entry-code {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-shrink: 0;
  }

  .ring {
    flex-shrink: 0;
  }

  .code-value {
    font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
    font-size: var(--fs-lg);
    font-weight: 700;
    letter-spacing: 0.1em;
    min-width: 5.5ch;
    text-align: center;
  }

  .entry-actions {
    display: flex;
    gap: var(--sp-1);
    flex-shrink: 0;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-10) var(--sp-4);
    color: var(--text-2);
    font-size: var(--fs-base);
    text-align: center;
  }

  .icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: none;
    cursor: pointer;
    color: var(--text-2);
    transition: all 0.15s;
  }

  .icon-btn:hover { border-color: var(--border-2, var(--accent)); color: var(--text); }
  .icon-btn.success { color: var(--success); border-color: var(--success); }
  .icon-btn.danger-soft:hover { color: var(--danger, #ef4444); border-color: var(--danger, #ef4444); }
  .icon-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .btn-ghost {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 0.4rem 0.85rem;
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
