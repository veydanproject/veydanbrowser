<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { NoteListItem } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    query: string;
    notes: NoteListItem[];
    excludeId?: string | null;
    /** Highlighted row; moved by arrow keys via `handleKeydown` */
    index?: number;
    onpick: (title: string) => void;
  }

  let { query, notes, excludeId = null, index = $bindable(0), onpick }: Props = $props();

  const MAX = 8;

  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const live = notes.filter((n) => n.id !== excludeId && !n.deleted);
    const list = q ? live.filter((n) => n.title.toLowerCase().includes(q)) : live;
    return list
      .sort((a, b) => {
        const as = a.title.toLowerCase().startsWith(q) ? 0 : 1;
        const bs = b.title.toLowerCase().startsWith(q) ? 0 : 1;
        return as - bs || b.updated_at.localeCompare(a.updated_at);
      })
      .slice(0, MAX);
  });

  const canCreate = $derived(
    query.trim().length > 0 && !matches.some((n) => n.title.toLowerCase() === query.trim().toLowerCase())
  );

  /** Rows in display order: existing notes, then "create" */
  function pickAt(i: number): string | null {
    if (i < matches.length) return matches[i].title;
    if (canCreate && i === matches.length) return query.trim();
    return null;
  }

  const rowCount = () => matches.length + (canCreate ? 1 : 0);

  /** Arrow navigation and Enter/Tab pick; returns true when the key was consumed. */
  export function handleKeydown(e: KeyboardEvent): boolean {
    const count = rowCount();
    if (!count) return false;
    if (e.key === 'ArrowDown') { e.preventDefault(); index = (index + 1) % count; return true; }
    if (e.key === 'ArrowUp') { e.preventDefault(); index = (index - 1 + count) % count; return true; }
    if (e.key === 'Enter' || e.key === 'Tab') {
      const title = pickAt(index);
      if (title) { e.preventDefault(); onpick(title); return true; }
    }
    return false;
  }
</script>

<div class="picker" role="listbox">
  {#each matches as n, i (n.id)}
    <button class="row" class:active={i === index} role="option" aria-selected={i === index} onmousedown={(e) => { e.preventDefault(); onpick(n.title); }}>
      <Icon name="file-text" size={12} />
      <span class="title">{n.title}</span>
    </button>
  {/each}
  {#if canCreate}
    <button class="row create" class:active={index === matches.length} role="option" aria-selected={index === matches.length} onmousedown={(e) => { e.preventDefault(); onpick(query.trim()); }}>
      <Icon name="plus" size={12} />
      <span class="title">{$t('note_link_create', { title: query.trim() })}</span>
    </button>
  {/if}
  {#if matches.length === 0 && !canCreate}
    <div class="row empty">{$t('note_link_type_to_search')}</div>
  {/if}
</div>

<style>
  .picker {
    position: absolute;
    left: var(--sp-4);
    bottom: var(--sp-3);
    z-index: 5;
    min-width: 240px;
    max-width: 360px;
    padding: 0.25rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.35rem 0.5rem;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text);
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .row.active, .row:hover { background: var(--surface-2); }
  .row.create { color: var(--accent); }
  .row.empty { color: var(--text-3); cursor: default; }
  .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
