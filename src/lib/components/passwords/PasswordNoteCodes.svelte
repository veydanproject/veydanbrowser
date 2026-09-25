<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { hasErrorCode } from '$lib/utils';
  import { t } from '$lib/i18n';
  import { parseBinding } from '$lib/bindings';
  import { userLabels, systemTags } from '$lib/entity-tags';
  import { notesStore } from '$lib/store/notes.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import ChipMark from '$lib/components/notes/ChipMark.svelte';
  import Icon from '$lib/Icon.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import type { PasswordEntry } from '$lib/types';

  interface Props {
    entries: PasswordEntry[];
  }

  let { entries }: Props = $props();
  let copiedId = $state('');
  let unlockOpen = $state(false);
  let pending = $state<PasswordEntry | null>(null);
  let timer: ReturnType<typeof setTimeout>;

  onMount(() => {
    void notesStore.ensureLoaded();
    void profilesStore.ensureLoaded();
    void workspacesStore.ensureLoaded();
  });

  function tagName(tag: string): string {
    const parsed = parseBinding(tag);
    if (!parsed) return tag;
    if (parsed.kind === 'profile') return profilesStore.list.find((p) => p.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((w) => w.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'note') return notesStore.list.find((n) => n.id === parsed.value)?.title ?? parsed.value;
    return parsed.value;
  }

  function labelColor(name: string): string | undefined {
    return notesStore.allTags.find((tag) => tag.name === name)?.color;
  }

  async function writeClipboard(value: string) {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      const el = document.createElement('textarea');
      el.value = value;
      el.setAttribute('readonly', '');
      el.style.position = 'fixed';
      el.style.left = '-9999px';
      document.body.appendChild(el);
      el.select();
      document.execCommand('copy');
      el.remove();
    }
  }

  async function copyPassword(entry: PasswordEntry) {
    if (notesLock.locked) {
      pending = entry;
      unlockOpen = true;
      return;
    }
    try {
      await api.passwords.copy(entry.id);
    } catch (e) {
      if (hasErrorCode(e, 'vault_locked') || hasErrorCode(e, 'vault_mismatch') || hasErrorCode(e, 'decrypt_failed')) {
        pending = entry;
        unlockOpen = true;
        return;
      }
      try {
        const revealed = await api.passwords.reveal(entry.id, 'password');
        await writeClipboard(revealed.value);
        const value = revealed.value;
        setTimeout(() => {
          navigator.clipboard.readText().then((current) => {
            if (current === value) void navigator.clipboard.writeText('');
          }).catch(() => {});
        }, 30_000);
      } catch {
        return;
      }
    }
    copiedId = entry.id;
    clearTimeout(timer);
    timer = setTimeout(() => (copiedId = ''), 1600);
  }

  async function copyUsername(entry: PasswordEntry) {
    if (!entry.username) return;
    await writeClipboard(entry.username);
    copiedId = `${entry.id}:user`;
    clearTimeout(timer);
    timer = setTimeout(() => (copiedId = ''), 1600);
  }
</script>

{#if entries.length > 0}
  <div class="codes">
    {#each entries as entry (entry.id)}
      <article class="row">
        <div class="head">
          <Icon name="lock" size={14} />
          <span class="name">{entry.title}</span>
        </div>
        {#if entry.username}
          <div class="line">
            <span>{entry.username}</span>
            <button type="button" class="btn btn-ghost btn-xs" onclick={() => copyUsername(entry)}>
              {copiedId === `${entry.id}:user` ? $t('pw_copied') : $t('pw_btn_copy')}
            </button>
          </div>
        {/if}
        <div class="line">
          <span class="mask">{copiedId === entry.id ? $t('pw_copied') : '••••••••'}</span>
          <button type="button" class="btn btn-ghost btn-xs" onclick={() => copyPassword(entry)} aria-label={notesLock.locked ? $t('notes_lock_title') : $t('pw_btn_copy')}>
            {#if notesLock.locked}<Icon name="lock" size={12} />{:else}{$t('pw_btn_copy')}{/if}
          </button>
        </div>
        {#if entry.tags.length}
          <div class="chips">
            {#each userLabels(entry.tags) as label (label)}
              <span class="chip" style:color={labelColor(label)} style:border-color={labelColor(label)}>{label}</span>
            {/each}
            {#each systemTags(entry.tags) as tag (tag)}
              <span class="chip"><ChipMark kind={parseBinding(tag)?.kind ?? 'tag'} />{tagName(tag)}</span>
            {/each}
          </div>
        {/if}
      </article>
    {/each}
  </div>
  <VaultUnlock bind:open={unlockOpen} onunlocked={() => { if (pending) void copyPassword(pending); }} />
{/if}

<style>
  .codes { display: flex; flex-direction: column; gap: var(--sp-2); margin-bottom: var(--sp-3); }
  .row {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
  }
  .head, .line { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
  .chips {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: nowrap;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .chips::-webkit-scrollbar { display: none; }
  .name { font-weight: 700; font-size: 0.9rem; }
  .line span { font-size: 0.82rem; color: var(--text-2); }
  .mask { font-family: var(--font-mono); color: var(--text); }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    border: 1px solid var(--border);
    border-radius: 99px;
    flex-shrink: 0;
    padding: 1px 7px;
    font-size: 0.72rem;
    white-space: nowrap;
  }
</style>
