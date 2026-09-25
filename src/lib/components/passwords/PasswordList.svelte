<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import { parseBinding } from '$lib/bindings';
  import { userLabels } from '$lib/entity-tags';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import Icon from '$lib/Icon.svelte';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';
  import type { PasswordEntry } from '$lib/types';

  interface Props {
    entries: PasswordEntry[];
    onopen: (id: string) => void;
    onedit?: (entry: PasswordEntry) => void;
  }

  let { entries, onopen, onedit }: Props = $props();

  let copied = $state('');
  let error = $state('');
  let unlockOpen = $state(false);
  let pending = $state<PasswordEntry | null>(null);
  let copyTimer: ReturnType<typeof setTimeout>;
  /** Horizontal tag drag must not open the row. */
  let tagDrag = false;

  onMount(() => {
    void profilesStore.ensureLoaded();
    void notesStore.ensureLoaded();
    void totpStore.ensureLoaded();
    return () => clearTimeout(copyTimer);
  });

  function profilesOf(tags: string[]): string[] {
    return tags.flatMap((tag) => {
      const parsed = parseBinding(tag);
      if (parsed?.kind !== 'profile') return [];
      const name = profilesStore.list.find((p) => p.id === parsed.value)?.name;
      return name ? [name] : [];
    });
  }

  function notesOf(tags: string[]): string[] {
    return tags.flatMap((tag) => {
      const parsed = parseBinding(tag);
      if (parsed?.kind !== 'note') return [];
      const title = notesStore.list.find((n) => n.id === parsed.value)?.title;
      return title ? [title] : [];
    });
  }
  function labelColor(name: string): string | undefined {
    return notesStore.allTags.find((tag) => tag.name === name)?.color;
  }

  function markCopied(key: string) {
    copied = '';
    requestAnimationFrame(() => (copied = key));
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied = ''), 2000);
  }

  async function copyUsername(entry: PasswordEntry) {
    if (!entry.username) return;
    error = '';
    try {
      await navigator.clipboard.writeText(entry.username);
      markCopied(`${entry.id}:user`);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function askUnlock(entry: PasswordEntry) {
    pending = entry;
    unlockOpen = true;
  }

  async function copyPassword(entry: PasswordEntry) {
    if (notesLock.locked) {
      askUnlock(entry);
      return;
    }
    error = '';
    try {
      await api.passwords.copy(entry.id);
      markCopied(`${entry.id}:pass`);
    } catch (e) {
      const code = e && typeof e === 'object' && 'code' in e ? String((e as { code: string }).code) : '';
      if (code === 'vault_locked' || code === 'vault_mismatch' || code === 'decrypt_failed') {
        askUnlock(entry);
        return;
      }
      try {
        const value = (await api.passwords.reveal(entry.id, 'password')).value;
        await navigator.clipboard.writeText(value);
        markCopied(`${entry.id}:pass`);
        const secret = value;
        setTimeout(() => {
          navigator.clipboard.readText().then((current) => {
            if (current === secret) void navigator.clipboard.writeText('');
          }).catch(() => {});
        }, 30_000);
      } catch (err) {
        error = err instanceof Error ? err.message : String(err);
      }
    }
  }
</script>

{#if error}<p class="err">{error}</p>{/if}
<div class="list">
  {#each entries as entry (entry.id)}
    {@const profiles = profilesOf(entry.tags)}
    {@const labels = [...userLabels(entry.tags), ...notesOf(entry.tags)]}
    {@const firstTotp = entry.totp_ids[0]}
    {@const totp = firstTotp ? totpStore.list.find((item) => item.id === firstTotp) : undefined}
    <div class="entry">
      <button type="button" class="entry-info" onclick={() => onopen(entry.id)}>
        <div class="entry-name">{entry.title}</div>
        {#if entry.username || entry.url}
          <div class="entry-sub">{entry.username || entry.url}</div>
        {/if}
        {#if profiles.length || labels.length}
          <!-- Horizontal strip: a drag must not open the row. -->
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <div
            class="entry-labels"
            ontouchstart={(e) => { tagDrag = false; e.stopPropagation(); }}
            ontouchmove={(e) => { tagDrag = true; e.stopPropagation(); }}
            ontouchend={(e) => e.stopPropagation()}
            onclick={(e) => { if (tagDrag) { e.stopPropagation(); e.preventDefault(); } }}
          >
            {#each profiles as name (name)}
              <span class="profile-badge">{name}</span>
            {/each}
            {#each labels as label (label)}
              <span class="label-badge" style:color={labelColor(label)} style:border-color={labelColor(label)}>{label}</span>
            {/each}
          </div>
        {/if}
      </button>
      {#if firstTotp}
        <TotpLiveCode entryId={firstTotp} period={totp?.period ?? 30} copiedLabel={$t('totp_copy')} compact />
        {#if entry.totp_ids.length > 1}<span class="totp-more">+{entry.totp_ids.length - 1}</span>{/if}
      {/if}
      <div class="entry-actions">
        {#if entry.username}
          <button
            type="button"
            class="icon-btn"
            class:success={copied === `${entry.id}:user`}
            title={$t('ctx_action_copy_username')}
            onclick={() => copyUsername(entry)}
          >
            <Icon name={copied === `${entry.id}:user` ? 'check' : 'user'} size={13} />
          </button>
        {/if}
        <button
          type="button"
          class="icon-btn"
          class:success={copied === `${entry.id}:pass`}
          title={notesLock.locked ? $t('notes_lock_title') : $t('ctx_action_copy_password')}
          onclick={() => copyPassword(entry)}
        >
          <Icon name={copied === `${entry.id}:pass` ? 'check' : notesLock.locked ? 'lock' : 'copy'} size={13} />
        </button>
        {#if onedit && !notesLock.locked}
          <button type="button" class="icon-btn" title={$t('pw_btn_edit')} onclick={() => onedit(entry)}>
            <Icon name="pencil" size={13} />
          </button>
        {/if}
      </div>
    </div>
  {/each}
</div>

<VaultUnlock bind:open={unlockOpen} onunlocked={() => { if (pending) void copyPassword(pending); }} />

<style>
  .list { display: flex; flex-direction: column; gap: 0.4rem; }
  .entry {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    gap: var(--sp-2);
    min-height: 64px;
    padding: 0.55rem 0.75rem;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
  }
  .entry:hover { border-color: var(--border-2); }
  .entry-info {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .entry-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-base);
    font-weight: var(--fw-semibold);
  }
  .entry-sub {
    margin-top: 0.1rem;
    font-size: var(--fs-xs);
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .entry-labels {
    display: flex;
    flex-wrap: nowrap;
    gap: 0.25rem;
    margin-top: 0.25rem;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    touch-action: pan-x pan-y;
  }
  .entry-labels::-webkit-scrollbar { display: none; }
  .label-badge {
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-2);
    font-size: var(--fs-2xs);
    padding: 0.05rem 0.35rem;
    white-space: nowrap;
  }
  .profile-badge {
    flex-shrink: 0;
    background: var(--accent-tint);
    border: 1px solid var(--accent-tint-border);
    color: var(--accent-text-2);
    border-radius: var(--radius-sm);
    font-size: var(--fs-2xs);
    padding: 0.1rem 0.4rem;
    font-weight: var(--fw-medium);
    white-space: nowrap;
  }
  .entry-actions { display: flex; gap: var(--sp-1); flex-shrink: 0; }
  .totp-more { font-size: var(--fs-2xs); color: var(--text-2); flex-shrink: 0; }
  .icon-btn.success { color: var(--success-text); }
  .err { margin: 0 0 var(--sp-2); color: var(--danger-text); font-size: var(--fs-xs); }
</style>
