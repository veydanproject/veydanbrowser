<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import { passwordErrorKey } from '$lib/password-error';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { parseBinding } from '$lib/bindings';
  import ChipMark from '$lib/components/notes/ChipMark.svelte';
  import type { PasswordEntry } from '$lib/types';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    entry: PasswordEntry;
    onedit: () => void;
    ondeleted: () => void;
  }

  let { entry, onedit, ondeleted }: Props = $props();

  let revealed = $state('');
  let note = $state('');
  let error = $state('');
  let toast = $state('');
  let unlockOpen = $state(false);
  let afterUnlock = $state<(() => void) | null>(null);

  const totps = $derived(
    entry.totp_ids
      .map((id) => totpStore.list.find((item) => item.id === id))
      .filter((item): item is NonNullable<typeof item> => !!item),
  );
  const unlocked = $derived(!notesLock.locked);

  $effect(() => {
    void notesStore.ensureLoaded();
    void profilesStore.ensureLoaded();
    void workspacesStore.ensureLoaded();
    void totpStore.ensureLoaded();
  });

  $effect(() => {
    if (!unlocked) {
      revealed = '';
      note = '';
      return;
    }
    if (!entry.has_note) {
      note = '';
      return;
    }
    const id = entry.id;
    let cancelled = false;
    api.passwords.reveal(id, 'note').then((result) => {
      if (!cancelled) note = result.value;
    }).catch(() => {});
    return () => { cancelled = true; };
  });

  function tagColor(tag: string): string | undefined {
    const parsed = parseBinding(tag);
    if (!parsed) return notesStore.allTags.find((item) => item.name === tag)?.color;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((ws) => ws.id === parsed.value)?.color;
    if (parsed.kind === 'profile') return 'var(--accent)';
    return undefined;
  }

  function tagLabel(tag: string): string {
    const parsed = parseBinding(tag);
    if (!parsed) return tag;
    if (parsed.kind === 'profile') return profilesStore.list.find((item) => item.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((ws) => ws.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'note') return notesStore.list.find((n) => n.id === parsed.value)?.title ?? parsed.value;
    return parsed.value;
  }

  function fail(e: unknown) {
    const key = passwordErrorKey(e);
    error = key ? $t(key) : formatError(e);
  }

  async function reveal() {
    error = '';
    try {
      if (revealed) {
        revealed = '';
        return;
      }
      revealed = (await api.passwords.reveal(entry.id, 'password')).value;
    } catch (e) {
      fail(e);
    }
  }

  function askUnlock(then: () => void) {
    afterUnlock = then;
    unlockOpen = true;
  }

  function onUnlocked() {
    const next = afterUnlock;
    afterUnlock = null;
    next?.();
  }

  function promote(totpId: string) {
    const run = () => void passwordStore.makePrimaryTotp(entry.id, totpId).catch(fail);
    if (notesLock.locked) askUnlock(run);
    else run();
  }

  async function copyPassword() {
    if (notesLock.locked) {
      askUnlock(() => void copyPassword());
      return;
    }
    error = '';
    try {
      await api.passwords.copy(entry.id);
      toast = $t('pw_copied');
    } catch (e) {
      const code = e && typeof e === 'object' && 'code' in e ? String((e as { code: string }).code) : '';
      if (code === 'vault_locked' || code === 'vault_mismatch' || code === 'decrypt_failed') {
        askUnlock(() => void copyPassword());
        return;
      }
      try {
        const value = (await api.passwords.reveal(entry.id, 'password')).value;
        await navigator.clipboard.writeText(value);
        toast = $t('pw_copied');
        setTimeout(() => {
          navigator.clipboard.readText().then((current) => {
            if (current === value) void navigator.clipboard.writeText('');
          }).catch(() => {});
        }, 30_000);
      } catch (err) {
        fail(err);
      }
    }
  }

  async function copyUsername() {
    if (!entry.username) return;
    await navigator.clipboard.writeText(entry.username);
    toast = $t('pw_copied');
  }

  async function remove() {
    if (!confirm($t('pw_delete_confirm', { name: entry.title }))) return;
    try {
      await api.passwords.delete(entry.id);
      await passwordStore.refresh();
      ondeleted();
    } catch (e) {
      fail(e);
    }
  }
</script>

<article class="card">
  <h2>{entry.title}</h2>
  {#if entry.username}
    <div class="line">
      <span>{$t('pw_field_username')}</span>
      <strong>{entry.username}</strong>
      <button type="button" class="btn btn-ghost btn-sm" onclick={copyUsername}>{$t('pw_btn_copy')}</button>
    </div>
  {/if}
  {#if entry.url}
    <div class="line">
      <span>{$t('pw_field_url')}</span>
      <a href={entry.url} target="_blank" rel="noreferrer">{entry.url}</a>
    </div>
  {/if}
  <div class="line">
    <span>{$t('pw_field_password')}</span>
    <strong class="mono">{unlocked && revealed ? revealed : '••••••••'}</strong>
    {#if unlocked}
      <button type="button" class="icon-btn" onclick={reveal} aria-label={$t(revealed ? 'pw_btn_hide' : 'pw_btn_reveal')}>
        <Icon name={revealed ? 'eye-off' : 'eye'} size={14} />
      </button>
      <button type="button" class="icon-btn" class:ok={toast === $t('pw_copied')} onclick={copyPassword} aria-label={$t('pw_btn_copy')}>
        <Icon name={toast === $t('pw_copied') ? 'check' : 'copy'} size={14} />
      </button>
    {:else}
      <button type="button" class="icon-btn" onclick={() => askUnlock(() => void copyPassword())} aria-label={$t('notes_lock_title')}>
        <Icon name="lock" size={14} />
      </button>
    {/if}
  </div>
  {#if note}
    <div class="line">
      <span>{$t('pw_field_note')}</span>
      <p>{note || '—'}</p>
    </div>
  {/if}
  {#if entry.tags.length}
    <div class="chips">
      {#each entry.tags as tag (tag)}
        {@const color = tagColor(tag)}
        <span class="chip" style:color style:border-color={color} style:background={color ? `color-mix(in srgb, ${color} 16%, transparent)` : undefined}>
          <ChipMark kind={parseBinding(tag)?.kind ?? 'tag'} />
          {tagLabel(tag)}
        </span>
      {/each}
    </div>
  {/if}
  {#each totps as totp, i (totp.id)}
    <div class="line">
      <span>{$t('pw_field_totp')}</span>
      <strong class="totp-name">{totp.issuer ? `${totp.issuer} · ${totp.name}` : totp.name}</strong>
      {#if totps.length > 1}
        {#if i === 0}
          <span class="primary" title={$t('pw_totp_primary')}><Icon name="pin" size={12} /></span>
        {:else}
          <button type="button" class="icon-btn" title={$t('pw_totp_make_primary')} onclick={() => promote(totp.id)}>
            <Icon name="arrow-up" size={12} />
          </button>
        {/if}
      {/if}
      <TotpLiveCode entryId={totp.id} period={totp.period} copiedLabel={$t('totp_copy')} compact />
    </div>
  {/each}
  {#if error}<p class="err">{error}</p>{/if}
  {#if toast}<p class="ok">{toast}</p>{/if}
  <VaultUnlock bind:open={unlockOpen} onunlocked={onUnlocked} />
  {#if unlocked}
    <div class="actions">
      <button type="button" class="btn btn-ghost btn-sm" onclick={onedit}><Icon name="pencil" size={12} /> {$t('pw_btn_edit')}</button>
      <button type="button" class="btn btn-ghost btn-sm" onclick={remove}><Icon name="trash-2" size={12} /> {$t('pw_btn_delete')}</button>
    </div>
  {/if}
</article>

<style>
  .card { display: flex; flex-direction: column; gap: var(--sp-3); }
  h2 { margin: 0; font-size: 1.05rem; }
  .line { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-2); font-size: 0.85rem; }
  .line span { color: var(--text-2); min-width: 72px; }
  .mono { font-family: var(--font-mono); }
  .totp-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .primary { display: inline-flex; color: var(--accent); }
  a { color: var(--text); }
  .err { color: var(--danger-text); margin: 0; }
  .ok { color: var(--success-text); margin: 0; }
  .icon-btn.ok { color: var(--success-text); }
  .actions { display: flex; gap: var(--sp-2); }
  .chips { display: flex; flex-wrap: nowrap; gap: 4px; overflow-x: auto; scrollbar-width: none; }
  .chips::-webkit-scrollbar { display: none; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 99px;
    flex-shrink: 0;
    padding: 2px 8px;
    font-size: 0.75rem;
    white-space: nowrap;
  }
</style>
