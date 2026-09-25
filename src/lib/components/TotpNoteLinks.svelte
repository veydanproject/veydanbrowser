<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { t } from '$lib/i18n';
  import ChipMark from '$lib/components/notes/ChipMark.svelte';

  interface NoteOpt {
    id: string;
    title: string;
  }

  interface Props {
    notes: NoteOpt[];
    selected: string[];
    onchange: (ids: string[]) => void;
  }

  let { notes, selected, onchange }: Props = $props();

  let query = $state('');
  let open = $state(false);

  const q = $derived(query.trim().toLowerCase());
  const hits = $derived(
    q ? notes.filter((n) => !selected.includes(n.id) && n.title.toLowerCase().includes(q)).slice(0, 6) : [],
  );

  function titleOf(id: string): string {
    return notes.find((n) => n.id === id)?.title ?? id;
  }

  function add(id: string) {
    onchange([...selected, id]);
    query = '';
    open = false;
  }

  function remove(id: string) {
    onchange(selected.filter((item) => item !== id));
  }
</script>

<div class="links">
  <span class="caption">{$t('pw_bind_note')}</span>
  {#if selected.length}
    <div class="chips">
      {#each selected as id (id)}
        <button type="button" class="chip" onclick={() => remove(id)}>
          <ChipMark kind="note" />
          {titleOf(id)}
          <span aria-hidden="true">×</span>
        </button>
      {/each}
    </div>
  {/if}
  <input
    type="text"
    bind:value={query}
    placeholder={$t('pw_bind_note')}
    autocomplete="off"
    onfocus={() => (open = true)}
    onblur={() => (open = false)}
  />
  {#if open && hits.length}
    <div class="pick" role="presentation" onmousedown={(e) => e.preventDefault()}>
      {#each hits as note (note.id)}
        <button type="button" class="pick-item" onclick={() => add(note.id)}>
          <ChipMark kind="note" />
          {note.title}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .links { position: relative; display: flex; flex-direction: column; gap: 4px; margin-bottom: var(--sp-3); }
  .caption { font-size: 0.78rem; color: var(--text-2); font-weight: 600; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 99px;
    padding: 2px 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
  }
  input {
    width: 100%;
    min-height: 36px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .pick {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    top: 100%;
    margin-top: 4px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    overflow: hidden;
  }
  .pick-item {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .pick-item:hover { background: var(--surface-2); }
</style>
