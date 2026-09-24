<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import ChipMark from '$lib/components/notes/ChipMark.svelte';
  import NoteLabelSheet from '$lib/components/mobile/NoteLabelSheet.svelte';
  import { api, formatError, type NavChild, type NoteTag } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { parseBinding } from '$lib/bindings';
  import { isSystemTag, mergeTags, systemTags, userLabels } from '$lib/totp-tags';

  interface Props {
    tags: string[];
    onchange: (tags: string[]) => void;
  }

  let { tags, onchange }: Props = $props();

  let open = $state(false);
  let error = $state('');
  let allTags = $state<NoteTag[]>([]);
  let workspaces = $state<NavChild[]>([]);
  let profiles = $state<NavChild[]>([]);

  const labels = $derived(userLabels(tags));
  const bindings = $derived(systemTags(tags));
  let boundNames = $state<Map<string, string>>(new Map());

  $effect(() => {
    const missing = bindings.filter((tag) => {
      const parsed = parseBinding(tag);
      return parsed && parsed.kind !== 'profile' && parsed.kind !== 'workspace' && !boundNames.has(tag);
    });
    if (missing.length === 0) return;
    let cancelled = false;
    api.notes.bindingSummaries(missing).then((list) => {
      if (cancelled) return;
      const next = new Map(boundNames);
      for (const item of list) next.set(item.binding, item.name);
      for (const tag of missing) if (!next.has(tag)) next.set(tag, tag.slice(tag.indexOf(':') + 1));
      boundNames = next;
    }).catch(() => {});
    return () => { cancelled = true; };
  });

  onMount(() => {
    api.notes.tags().then((list) => (allTags = list)).catch(() => {});
    api.notes.nav().then((nav) => {
      workspaces = nav.all_workspaces;
      profiles = nav.all_profiles;
    }).catch(() => {});
  });

  function bindingKind(tag: string): string {
    return parseBinding(tag)?.kind ?? 'workspace';
  }

  function bindingName(tag: string): string {
    const parsed = parseBinding(tag);
    if (!parsed) return tag;
    if (parsed.kind === 'profile') return profiles.find((item) => item.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'workspace') return workspaces.find((item) => item.id === parsed.value)?.name ?? parsed.value;
    return boundNames.get(tag) ?? parsed.value;
  }

  function tagColor(name: string): string | undefined {
    return allTags.find((tag) => tag.name === name)?.color;
  }

  function remove(tag: string) {
    onchange(tags.filter((item) => item !== tag));
  }

  async function addTag(name: string, color?: string) {
    const value = name.trim().replace(/^#/, '');
    if (!value || isSystemTag(value) || labels.includes(value)) {
      open = false;
      return;
    }
    error = '';
    const existing = allTags.find((tag) => tag.name === value);
    try {
      if (!existing && color) {
        const created = await api.notes.tagCreate(value, color);
        allTags = [...allTags, created];
      }
      onchange(mergeTags([], bindings, [...labels, existing?.name ?? value]));
      open = false;
    } catch (err) {
      error = formatError(err);
    }
  }

  function addBinding(binding: string) {
    if (!isSystemTag(binding) || bindings.includes(binding)) {
      open = false;
      return;
    }
    onchange(mergeTags([], [...bindings, binding], labels));
    open = false;
  }
</script>

<div class="field">
  <span class="caption">{$t('totp_field_tags')}</span>
  <div class="m-chips">
    {#each bindings as binding (binding)}
      <button
        type="button"
        class="m-chip small"
        class:ws={binding.startsWith('workspace:')}
        onclick={() => remove(binding)}
      >
        <ChipMark kind={bindingKind(binding)} />
        {bindingName(binding)}
      </button>
    {/each}
    {#each labels as name (name)}
      <button type="button" class="m-chip small" style:--chip={tagColor(name)} onclick={() => remove(name)}>
        <ChipMark kind="tag" />
        {name}
      </button>
    {/each}
    <button type="button" class="m-chip add" onclick={() => (open = true)} aria-label={$t('notes_tags_add')}>
      <Icon name="plus" size={14} />
    </button>
  </div>
  {#if error}
    <div class="m-error">{error}</div>
  {/if}
</div>

<NoteLabelSheet
  {open}
  tags={allTags}
  selectedTags={labels}
  folders={[]}
  folderIds={[]}
  {workspaces}
  {profiles}
  {bindings}
  onclose={() => (open = false)}
  onaddTag={addTag}
  onaddFolder={() => {}}
  onaddBinding={addBinding}
/>

<style>
  .field { display: flex; flex-direction: column; gap: var(--sp-2); margin-bottom: var(--sp-3); }
  .caption {
    font-size: var(--fs-sm);
    color: var(--text-2);
    font-weight: 600;
  }
</style>
