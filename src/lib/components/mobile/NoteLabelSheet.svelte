<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import type { NavChild, NoteTag } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    tags: NoteTag[];
    selectedTags: string[];
    folders: NavChild[];
    folderIds: string[];
    workspaces: NavChild[];
    profiles: NavChild[];
    bindings: string[];
    busy?: boolean;
    onclose: () => void;
    onaddTag: (name: string, color?: string) => void;
    onaddFolder: (folderId: string) => void;
    onaddBinding: (binding: string) => void;
  }

  let {
    open, tags, selectedTags, folders, folderIds, workspaces, profiles, bindings,
    busy = false, onclose, onaddTag, onaddFolder, onaddBinding,
  }: Props = $props();

  const TAG_COLORS = [
    '#8b7bff', '#60a5fa', '#2dd4bf', '#f472b6',
    '#f5c451', '#34d399', '#f26d6d', '#f97316',
  ];

  let query = $state('');
  let color = $state(TAG_COLORS[0]);

  $effect(() => {
    if (open) {
      query = '';
      color = TAG_COLORS[0];
    }
  });

  const q = $derived(query.trim().toLowerCase());
  const tagHits = $derived(
    tags.filter((t) => !selectedTags.includes(t.name) && t.name.toLowerCase().includes(q)),
  );
  const folderHits = $derived(
    q ? folders.filter((f) => !folderIds.includes(f.id) && f.name.toLowerCase().includes(q)) : [],
  );
  const workspaceHits = $derived(
    q ? workspaces.filter((w) => !bindings.includes(`workspace:${w.id}`) && w.name.toLowerCase().includes(q)) : [],
  );
  const profileHits = $derived(
    q ? profiles.filter((p) => !bindings.includes(`profile:${p.id}`) && p.name.toLowerCase().includes(q)) : [],
  );
  const canCreate = $derived(query.trim().length > 0 && !tags.some((t) => t.name === query.trim()));
  const hasHits = $derived(tagHits.length + folderHits.length + workspaceHits.length + profileHits.length > 0);

  function submitTag() {
    const name = query.trim().replace(/^#/, '');
    if (!name) return;
    onaddTag(name, canCreate ? color : undefined);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      submitTag();
    }
  }
</script>

<BottomSheet {open} title={$t('notes_tags_add')} {onclose}>
  <div class="find">
    <Icon name="search" size={16} />
    <input
      type="text"
      bind:value={query}
      placeholder={$t('notes_tags_placeholder')}
      onkeydown={onKeydown}
      {@attach (el: HTMLInputElement) => el.focus()}
    />
  </div>

  {#if hasHits}
    <div class="hits">
      {#each tagHits as tag (tag.id)}
        <button type="button" class="hit" disabled={busy} onpointerdown={(e) => e.preventDefault()} onclick={() => onaddTag(tag.name)}>
          <span class="dot" style:background-color={tag.color}></span>
          <span class="name" style:color={tag.color}>{tag.name}</span>
        </button>
      {/each}
      {#each folderHits as f (f.id)}
        <button type="button" class="hit" disabled={busy} onpointerdown={(e) => e.preventDefault()} onclick={() => onaddFolder(f.id)}>
          <span class="dot" style:background={f.color}></span>
          <span class="name">{f.name}</span>
          <span class="kind">{$t('notes_kind_folder')}</span>
        </button>
      {/each}
      {#each workspaceHits as w (w.id)}
        <button type="button" class="hit" disabled={busy} onpointerdown={(e) => e.preventDefault()} onclick={() => onaddBinding(`workspace:${w.id}`)}>
          <span class="dot" style:background={w.color}></span>
          <span class="name">{w.name}</span>
          <span class="kind">{$t('notes_kind_workspace')}</span>
        </button>
      {/each}
      {#each profileHits as p (p.id)}
        <button type="button" class="hit" disabled={busy} onpointerdown={(e) => e.preventDefault()} onclick={() => onaddBinding(`profile:${p.id}`)}>
          <span class="dot"></span>
          <span class="name">{p.name}</span>
          <span class="kind">{$t('notes_kind_profile')}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#if canCreate}
    <div class="colors">
      {#each TAG_COLORS as c (c)}
        <button type="button" class="swatch" class:on={color === c} style:background={c} aria-label={c} onclick={() => (color = c)}></button>
      {/each}
      <label class="swatch custom" class:on={!TAG_COLORS.includes(color)}>
        <input type="color" bind:value={color} />
      </label>
    </div>
    <button type="button" class="m-btn-grad" disabled={busy} onclick={submitTag}>{$t('notes_tags_add')}</button>
  {/if}
</BottomSheet>

<style>
  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 12px;
    border-radius: 12px;
    background: var(--m-field);
    flex-shrink: 0;
  }
  .find input {
    flex: 1;
    min-width: 0;
    width: auto;
    min-height: 44px;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
    font-size: 15px;
  }
  .find input:focus { border: 0; box-shadow: none; }
  .hits {
    max-height: min(280px, 42vh);
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    touch-action: pan-y;
    display: flex;
    flex-direction: column;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 44px;
    padding: 0 var(--sp-2);
    border: 0;
    border-radius: 12px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 15px;
    text-align: left;
  }
  .hit:disabled { opacity: 0.5; }
  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .kind { color: var(--text-3); font-size: 12px; }
  .colors { display: flex; gap: 8px; padding: var(--sp-2) 0; }
  .swatch {
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid transparent;
  }
  .swatch.on { border-color: var(--text); }
  .custom { position: relative; background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red); overflow: hidden; }
  .custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    border: 0;
    padding: 0;
  }
</style>
