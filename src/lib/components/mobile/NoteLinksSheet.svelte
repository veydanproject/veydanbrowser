<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Outgoing wiki links and backlinks of a note. -->
<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { api, formatError, type NoteLinks, type NoteListItem } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    noteId: string;
    onclose: () => void;
    onopen: (id: string) => void;
  }

  let { open, noteId, onclose, onopen }: Props = $props();

  let links = $state<NoteLinks | null>(null);
  let error = $state('');

  $effect(() => {
    if (!open) return;
    api.notes.links(noteId).then((l) => (links = l)).catch((e) => (error = formatError(e)));
  });

  const isEmpty = $derived(!!links && links.outgoing.length === 0 && links.backlinks.length === 0);
</script>

{#snippet group(label: string, items: NoteListItem[])}
  {#if items.length}
    <div>
      <div class="group">{label}</div>
      <div class="m-list">
        {#each items as n (n.id)}
          <button type="button" class="m-row" onclick={() => onopen(n.id)}>
            <Icon name="file-text" size={16} />
            <span class="m-row-label">{n.title || $t('notes_untitled')}</span>
            <Icon name="chevron-right" size={16} />
          </button>
        {/each}
      </div>
    </div>
  {/if}
{/snippet}

<BottomSheet {open} title={$t('notes_links')} {onclose}>
  {#if error}<div class="m-error">{error}</div>{/if}
  {#if isEmpty}
    <p class="empty">{$t('notes_links_empty')}</p>
  {:else if links}
    {@render group($t('notes_links_outgoing'), links.outgoing)}
    {@render group($t('notes_links_backlinks'), links.backlinks)}
  {/if}
</BottomSheet>

<style>
  .empty { color: var(--text-3); text-align: center; padding: var(--sp-5) 0; }
  .group {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: var(--sp-1);
  }
  .m-row-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
