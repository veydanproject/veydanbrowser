<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import type { NoteListItem } from '$lib/mobile/api';
  import { onKeyboard } from '$lib/mobile/keyboard';
  import { t } from '$lib/mobile/i18n';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    seed: string;
    notes: NoteListItem[];
    excludeId: string;
    onclose: () => void;
    onpick: (title: string) => void;
  }

  let { open, seed, notes, excludeId, onclose, onpick }: Props = $props();

  let query = $state('');
  let expanded = $state(false);
  let kb = $state(0);

  $effect(() => {
    if (!open) {
      expanded = false;
      return;
    }
    if (!expanded) query = seed;
  });

  $effect(() => {
    if (!open || expanded) return;
    return onKeyboard((n) => (kb = n));
  });

  const LIMIT = 60;

  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const live = notes.filter((n) => n.id !== excludeId && n.title);
    const list = q ? live.filter((n) => n.title.toLowerCase().includes(q)) : live;
    return list
      .sort((a, b) => {
        const as = a.title.toLowerCase().startsWith(q) ? 0 : 1;
        const bs = b.title.toLowerCase().startsWith(q) ? 0 : 1;
        return as - bs || b.updated_at.localeCompare(a.updated_at);
      })
      .slice(0, LIMIT);
  });

  const canCreate = $derived(
    query.trim().length > 0 && !matches.some((n) => n.title.toLowerCase() === query.trim().toLowerCase()),
  );

  function close() {
    expanded = false;
    onclose();
  }
</script>

{#if open && !expanded}
  <div class="bar" style:bottom="{kb}px" style:padding-bottom="{kb > 0 ? '8px' : 'calc(8px + env(safe-area-inset-bottom))'}">
    <button type="button" class="find" onclick={() => (expanded = true)}>
      <span class="ico"><Icon name="search" size={18} /></span>
      <span class="ph">{query.trim() || $t('notes_link_search')}</span>
    </button>
  </div>
{/if}

<BottomSheet open={open && expanded} title={$t('notes_links')} onclose={close}>
  <div class="find sheet">
    <span class="ico"><Icon name="search" size={18} /></span>
    <input
      type="search"
      bind:value={query}
      placeholder={$t('notes_link_search')}
      {@attach (el: HTMLInputElement) => { if (open && expanded) el.focus(); }}
    />
  </div>

  <div class="m-list">
    {#each matches as n (n.id)}
      <button type="button" class="m-row" onclick={() => onpick(n.title)}>
        <Icon name="file-text" size={20} />
        <span class="m-row-label">{n.title}</span>
      </button>
    {/each}
    {#if canCreate}
      <button type="button" class="m-row" onclick={() => onpick(query.trim())}>
        <Icon name="plus" size={20} />
        <span class="m-row-label">{$t('notes_link_create', { title: query.trim() })}</span>
      </button>
    {/if}
    {#if matches.length === 0 && !canCreate}
      <div class="m-row"><span class="m-row-label muted">{$t('common_nothing_found')}</span></div>
    {/if}
  </div>
</BottomSheet>

<style>
  .bar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 21;
    padding: 8px 12px;
    background: var(--m-nav);
    border-top: 1px solid var(--border);
  }
  .find {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: 44px;
    padding: 0 14px;
    border: 0;
    border-radius: 12px;
    background: var(--m-field);
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .find.sheet { flex-shrink: 0; }
  .find input {
    flex: 1;
    min-width: 0;
    min-height: 44px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
  }
  .find input:focus { box-shadow: none; }
  .ico { color: var(--text-3); display: inline-flex; flex-shrink: 0; }
  .ph { color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .muted { color: var(--text-3); }
</style>
