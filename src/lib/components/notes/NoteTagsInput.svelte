<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { NoteTag, NoteTagInfo, NoteFolder, Workspace, Profile } from '$lib/types';
  import { api } from '$lib/api';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';
  import ChipMark from './ChipMark.svelte';

  interface ContextChip {
    kind: string;
    label: string;
    color: string;
    onremove?: () => void;
  }

  interface Props {
    selectedTags: NoteTagInfo[];
    allTags: NoteTag[];
    onchange: (tagNames: string[]) => void;
    contextChips?: ContextChip[];
    folders?: NoteFolder[];
    activeFolderIds?: string[];
    onaddFolder?: (folderId: string) => void;
    workspaces?: Workspace[];
    profiles?: Profile[];
    activeBindings?: string[];
    onaddBinding?: (binding: string) => void;
  }

  let { selectedTags, allTags, onchange, contextChips = [], folders = [], activeFolderIds = [], onaddFolder, workspaces = [], profiles = [], activeBindings = [], onaddBinding }: Props = $props();

  const TAG_COLORS = [
    '#8b7bff', '#60a5fa', '#2dd4bf', '#f472b6',
    '#f5c451', '#34d399', '#f26d6d', '#f97316',
  ];

  let open = $state(false);
  let inputValue = $state('');
  let selectedColor = $state(TAG_COLORS[0]);
  let inputEl: HTMLInputElement | null = $state(null);
  let wrapEl: HTMLDivElement | null = $state(null);

  const selectedNames = $derived(new Set(selectedTags.map((t) => t.name)));

  const suggestions = $derived(
    allTags
      .filter((t) => !selectedNames.has(t.name) && t.name.toLowerCase().includes(inputValue.toLowerCase()))
      .slice(0, 6)
  );

  const folderSuggestions = $derived(
    inputValue.trim().length > 0
      ? folders
          .filter(f => !activeFolderIds.includes(f.id) && f.name.toLowerCase().includes(inputValue.toLowerCase()))
          .slice(0, 4)
      : []
  );

  const workspaceSuggestions = $derived(
    inputValue.trim().length > 0
      ? workspaces
          .filter(w => !activeBindings.includes(`workspace:${w.id}`) && w.name.toLowerCase().includes(inputValue.toLowerCase()))
          .slice(0, 3)
      : []
  );

  const profileSuggestions = $derived(
    inputValue.trim().length > 0
      ? profiles
          .filter(p => !activeBindings.includes(`profile:${p.id}`) && p.name.toLowerCase().includes(inputValue.toLowerCase()))
          .slice(0, 3)
      : []
  );

  const isNew = $derived(
    inputValue.trim().length > 0 && !allTags.some((t) => t.name === inputValue.trim())
  );

  function openPopup() {
    open = true;
    inputValue = '';
    selectedColor = TAG_COLORS[0];
    setTimeout(() => inputEl?.focus(), 50);
  }

  function close() {
    open = false;
    inputValue = '';
  }

  async function addTag(name: string, color?: string) {
    const trimmed = name.trim();
    if (!trimmed || selectedNames.has(trimmed)) { close(); return; }

    const existing = allTags.find((t) => t.name === trimmed);
    if (!existing && color) {
      await api.notes.tagCreate(trimmed, color);
    }

    onchange([...selectedNames, trimmed]);
    close();
  }

  function removeTag(name: string) {
    onchange([...selectedNames].filter((n) => n !== name));
  }

  function onKeydown(e: KeyboardEvent) {
    if ((e.key === 'Enter' || e.key === ',') && inputValue.trim()) {
      e.preventDefault();
      void addTag(inputValue, selectedColor);
    }
    if (e.key === 'Escape') close();
  }

  function onOutsideClick(e: MouseEvent) {
    if (wrapEl && !wrapEl.contains(e.target as Node)) close();
  }
</script>

<svelte:window onclick={onOutsideClick} />

<div class="tags-outer" bind:this={wrapEl}>
  <div class="tags-wrap">
    {#each contextChips as chip}
      <span class="chip chip-context" style="border-color:{chip.color}22;color:{chip.color};background:{chip.color}18">
        <ChipMark kind={chip.kind} />
        {chip.label}
        {#if chip.onremove}
          <button class="chip-x" onclick={chip.onremove}>×</button>
        {/if}
      </span>
    {/each}

    {#each selectedTags as tag (tag.id)}
      <span class="chip" style="border-color:{tag.color};color:{tag.color};background:{tag.color}18">
        <ChipMark kind="tag" />
        {tag.name}
        <button class="chip-x" onclick={() => removeTag(tag.name)}>×</button>
      </span>
    {/each}
  </div>

  <div class="add-wrap">
    <button class="add-btn" onclick={openPopup} title={$t('notes_tags_add')}>
      <Icon name="plus" size={12} />
    </button>

    {#if open}
      <div class="popup">
        <input
          bind:this={inputEl}
          bind:value={inputValue}
          type="text"
          class="popup-input"
          placeholder={$t('notes_tags_placeholder')}
          onkeydown={onKeydown}
        />

        {#if suggestions.length > 0 || folderSuggestions.length > 0 || workspaceSuggestions.length > 0 || profileSuggestions.length > 0}
          <div class="suggestions">
            {#each suggestions as s (s.id)}
              <button class="sug-item" onmousedown={(e) => { e.preventDefault(); void addTag(s.name); }}>
                <span class="sug-dot" style="background:{s.color}"></span>
                {s.name}
              </button>
            {/each}
            {#each folderSuggestions as f (f.id)}
              <button class="sug-item" onmousedown={(e) => { e.preventDefault(); onaddFolder?.(f.id); close(); }}>
                <span class="sug-dot" style="background:{f.color}"></span>
                {f.name}
                <span class="sug-folder-label">папка</span>
              </button>
            {/each}
            {#each workspaceSuggestions as w (w.id)}
              <button class="sug-item" onmousedown={(e) => { e.preventDefault(); onaddBinding?.(`workspace:${w.id}`); close(); }}>
                <span class="sug-dot" style="background:{w.color}"></span>
                {w.name}
                <span class="sug-folder-label">воркспейс</span>
              </button>
            {/each}
            {#each profileSuggestions as p (p.id)}
              <button class="sug-item" onmousedown={(e) => { e.preventDefault(); onaddBinding?.(`profile:${p.id}`); close(); }}>
                <span class="sug-dot" style="background:var(--accent)"></span>
                {p.name}
                <span class="sug-folder-label">профиль</span>
              </button>
            {/each}
          </div>
        {/if}

        {#if isNew}
          <div class="color-row">
            {#each TAG_COLORS as c}
              <button
                class="color-swatch"
                class:active={selectedColor === c}
                style="background:{c}"
                aria-label="Цвет {c}"
                onclick={() => (selectedColor = c)}
              ></button>
            {/each}
            <label
              class="color-swatch color-swatch-custom"
              class:active={!TAG_COLORS.includes(selectedColor)}
              title="Custom color"
            >
              <input type="color" bind:value={selectedColor} />
              {#if !TAG_COLORS.includes(selectedColor)}
                <span class="custom-dot" style="background:{selectedColor}"></span>
              {/if}
            </label>
          </div>
          <button class="btn btn-primary btn-sm create-btn" onclick={() => addTag(inputValue, selectedColor)}>
            Добавить тег
          </button>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .tags-outer {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex: 1;
    min-width: 0;
  }

  .tags-wrap {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    gap: 0.3rem;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tags-wrap::-webkit-scrollbar { display: none; }

  .chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.2rem;
    font-size: var(--fs-2xs);
    line-height: 1;
    padding: 0.2rem 0.45rem;
    border-radius: 999px;
    border: 1px solid;
    font-weight: 500;
    white-space: nowrap;
  }

  .chip-context {
    opacity: 0.9;
  }

  .chip-x {
    background: none;
    border: none;
    cursor: pointer;
    padding: 0;
    font-size: var(--fs-sm);
    line-height: 1;
    color: inherit;
    opacity: 0.6;
    display: flex;
    align-items: center;
  }
  .chip-x:hover { opacity: 1; }

  .add-wrap {
    position: relative;
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: none;
    border: 1px solid var(--border-2);
    color: var(--text-2);
    cursor: pointer;
    padding: 0;
    transition: all 0.15s;
  }
  .add-btn:hover { border-color: var(--accent); color: var(--accent); }

  .popup {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    min-width: 240px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-lg);
    z-index: 500;
    padding: var(--sp-2);
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .popup-input {
    width: 100%;
    padding: 0.3rem var(--sp-2);
    font-size: var(--fs-sm);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    outline: none;
  }
  .popup-input:focus { border-color: var(--accent); }

  .suggestions {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .sug-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.3rem var(--sp-2);
    background: none;
    border: none;
    cursor: pointer;
    font-size: var(--fs-sm);
    color: var(--text);
    text-align: left;
    transition: background 0.1s;
  }
  .sug-item:hover { background: var(--surface); }

  .sug-folder-label {
    margin-left: auto;
    font-size: var(--fs-2xs);
    color: var(--text-3);
    flex-shrink: 0;
  }

  .sug-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .color-row {
    display: flex;
    gap: 0.3rem;
    flex-wrap: wrap;
  }

  .color-swatch {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2px solid transparent;
    cursor: pointer;
    padding: 0;
    transition: transform 0.1s;
  }
  .color-swatch.active { border-color: var(--text); transform: scale(1.2); }
  .color-swatch:hover { transform: scale(1.15); }

  .color-swatch-custom {
    position: relative;
    background: conic-gradient(#f43f5e, #f97316, #eab308, #22c55e, #06b6d4, #6366f1, #ec4899, #f43f5e);
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .color-swatch-custom input[type="color"] {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
    border: none;
    padding: 0;
    margin: 0;
  }

  .custom-dot {
    position: absolute;
    inset: 3px;
    border-radius: 50%;
    border: 1.5px solid rgba(255,255,255,0.7);
    pointer-events: none;
  }

  /* .btn.btn-primary covers colors/hover; keep only the left-aligned label delta */
  .create-btn { justify-content: flex-start; text-align: left; }
</style>
