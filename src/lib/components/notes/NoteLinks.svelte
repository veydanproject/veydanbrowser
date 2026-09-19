<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { api } from '$lib/api';
  import type { NoteListItem } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    noteId: string;
    /** Bumped by the editor after a save so backlinks refresh */
    version?: number;
    onopen: (id: string) => void;
  }

  let { noteId, version = 0, onopen }: Props = $props();

  let backlinks = $state<NoteListItem[]>([]);
  let related = $state<NoteListItem[]>([]);

  $effect(() => {
    const id = noteId;
    void version;
    Promise.all([api.notes.backlinks(id), api.notes.related(id)])
      .then(([b, r]) => {
        if (id !== noteId) return;
        backlinks = b;
        related = r;
      })
      .catch(() => {});
  });
</script>

<div class="links">
  <div class="group">
    <div class="label"><Icon name="arrow-left" size={11} /> {$t('note_links_backlinks')} <span class="count">{backlinks.length}</span></div>
    {#if backlinks.length === 0}
      <span class="empty">{$t('note_links_none')}</span>
    {:else}
      {#each backlinks as n (n.id)}
        <button class="item" onclick={() => onopen(n.id)}>
          <span class="title">{n.title}</span>
          {#if n.preview}<span class="preview">{n.preview}</span>{/if}
        </button>
      {/each}
    {/if}
  </div>
  <div class="group">
    <div class="label"><Icon name="link" size={11} /> {$t('note_links_related')} <span class="count">{related.length}</span></div>
    {#if related.length === 0}
      <span class="empty">{$t('note_links_none')}</span>
    {:else}
      {#each related as n (n.id)}
        <button class="item" onclick={() => onopen(n.id)}>
          <span class="title">{n.title}</span>
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .links {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-3);
    max-height: 200px;
    overflow-y: auto;
    padding: var(--sp-2) var(--sp-4);
    border-top: 1px solid var(--border);
    background: var(--surface-1);
  }
  .group { display: flex; flex-direction: column; gap: 0.2rem; min-width: 0; }
  .label {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: var(--fs-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-3);
    font-weight: 600;
    margin-bottom: 0.2rem;
  }
  .count { margin-left: auto; font-weight: 400; }
  .item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.1rem;
    padding: 0.3rem 0.45rem;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
    min-width: 0;
    width: 100%;
  }
  .item:hover { background: var(--surface-2); }
  .title { font-size: var(--fs-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .preview { font-size: var(--fs-2xs); color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .empty { font-size: var(--fs-xs); color: var(--text-3); padding: 0.2rem 0.45rem; }
</style>
