<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { get } from 'svelte/store';
  import Icon from '$lib/Icon.svelte';
  import EntityLabelField from '$lib/components/mobile/EntityLabelField.svelte';
  import PasswordTotpField from '$lib/components/mobile/PasswordTotpField.svelte';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';
  import { api, formatError } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import { generatePassword, pwSettings } from '$lib/password-gen';
  import { passwordErrorKey } from '$lib/password-error';
  import { hasErrorCode } from '$lib/utils';
  import { NAV_COLORS } from '$lib/mobile/nav-colors';
  import type { PasswordEntry, TotpEntry } from '$lib/types';

  const id = $derived(page.params.id ?? '');

  let entry = $state<PasswordEntry | null>(null);
  let missing = $state(false);
  let editing = $state(false);
  let title = $state('');
  let username = $state('');
  let url = $state('');
  let password = $state('');
  let note = $state('');
  let noteTouched = $state(false);
  let totpIds = $state<string[]>([]);
  let addTotp = $state(false);
  let totpName = $state('');
  let totpIssuer = $state('');
  let totpSecret = $state('');
  let tags = $state<string[]>([]);
  let revealed = $state('');
  let copiedKey = $state('');
  let unlockOpen = $state(false);
  let afterUnlock = $state<'reveal' | 'copy' | 'edit' | 'delete' | 'tags' | 'primary' | null>(null);
  let pendingTags = $state<string[] | null>(null);
  let pendingPrimary = $state<string | null>(null);
  let shownNote = $state('');
  let copyTimer: ReturnType<typeof setTimeout>;
  let totp = $state<TotpEntry[]>([]);
  let toast = $state('');
  let error = $state('');
  let saving = $state(false);

  const unlocked = $derived(!notesLock.locked);
  const linked = $derived(
    (entry?.totp_ids ?? []).map((tid) => totp.find((item) => item.id === tid)).filter((item): item is TotpEntry => !!item),
  );

  function fail(e: unknown) {
    const key = passwordErrorKey(e);
    error = key ? $t(key) : formatError(e);
  }

  async function load() {
    try {
      const list = await api.passwords.list();
      const found = list.find((item) => item.id === id) ?? null;
      entry = found;
      missing = !found;
      if (found) {
        title = found.title;
        username = found.username ?? '';
        url = found.url ?? '';
        totpIds = [...found.totp_ids];
        tags = [...found.tags];
      }
    } catch (e) {
      fail(e);
    }
  }

  $effect(() => {
    if (!unlocked || !entry?.has_note) {
      if (!unlocked) shownNote = '';
      return;
    }
    const noteId = entry.id;
    let cancelled = false;
    api.passwords.reveal(noteId, 'note').then((result) => {
      if (!cancelled) {
        shownNote = result.value;
        if (!noteTouched) note = result.value;
      }
    }).catch(() => {});
    return () => { cancelled = true; };
  });

  onMount(() => {
    void notesLock.refresh();
    void notesLock.listen();
    void load();
    api.totp.list().then((list) => (totp = list)).catch(() => {});
  });

  function askUnlock(action: 'reveal' | 'copy' | 'edit' | 'delete' | 'tags' | 'primary') {
    afterUnlock = action;
    unlockOpen = true;
  }

  function onUnlocked() {
    const action = afterUnlock;
    afterUnlock = null;
    if (action === 'reveal') void reveal();
    else if (action === 'copy') void copyPassword();
    else if (action === 'edit') editing = true;
    else if (action === 'delete') void remove();
    else if (action === 'tags' && pendingTags) {
      const next = pendingTags;
      pendingTags = null;
      void saveTags(next);
    } else if (action === 'primary' && pendingPrimary) {
      const tid = pendingPrimary;
      pendingPrimary = null;
      void makePrimary(tid);
    }
  }

  async function reveal() {
    if (notesLock.locked) return askUnlock('reveal');
    error = '';
    try {
      revealed = revealed ? '' : (await api.passwords.reveal(id, 'password')).value;
    } catch (e) {
      if (hasErrorCode(e, 'vault_locked') || hasErrorCode(e, 'vault_mismatch') || hasErrorCode(e, 'decrypt_failed')) {
        return askUnlock('reveal');
      }
      fail(e);
    }
  }

  async function copyPassword() {
    if (notesLock.locked) return askUnlock('copy');
    error = '';
    try {
      const value = (await api.passwords.reveal(id, 'password')).value;
      revealed = value;
      await writeClipboard(value);
      markCopied('password');
      toast = $t('pw_copied');
    } catch (e) {
      if (hasErrorCode(e, 'vault_locked') || hasErrorCode(e, 'vault_mismatch') || hasErrorCode(e, 'decrypt_failed')) {
        return askUnlock('copy');
      }
      fail(e);
    }
  }

  function markCopied(key: string) {
    copiedKey = key;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copiedKey = ''), 1600);
  }

  async function copyField(key: string, value: string) {
    try {
      await writeClipboard(value);
      markCopied(key);
    } catch (e) {
      fail(e);
    }
  }

  async function writeClipboard(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      const el = document.createElement('textarea');
      el.value = value;
      el.setAttribute('readonly', '');
      el.style.position = 'fixed';
      el.style.left = '-9999px';
      document.body.appendChild(el);
      el.select();
      const ok = document.execCommand('copy');
      el.remove();
      if (!ok) throw new Error('copy');
    }
  }

  async function save() {
    if (!title.trim()) return (error = $t('pw_err_title'));
    saving = true;
    error = '';
    try {
      const next = [...totpIds];
      // Created before the password; removed again if the password save fails.
      let newTotpId: string | null = null;
      if (addTotp && totpSecret.trim()) {
        const code = await api.totp.add({
          name: (totpName || title).trim(),
          issuer: totpIssuer.trim() || null,
          secret: totpSecret.trim(),
          tags,
        });
        newTotpId = code.id;
        next.push(code.id);
      }
      try {
        entry = await api.passwords.update(id, {
          title: title.trim(),
          username,
          url,
          password: password || null,
          note: noteTouched || note ? note : null,
          clear_note: noteTouched && !note.trim(),
          totp_ids: next,
          tags,
        });
      } catch (e) {
        if (newTotpId) await api.totp.delete(newTotpId).catch(() => {});
        throw e;
      } finally {
        if (newTotpId) totp = await api.totp.list();
      }
      await passwordStore.refresh();
      await passwordStore.dropStaleNoteBindings(id, tags);
      totpIds = next;
      addTotp = false;
      totpSecret = '';
      shownNote = note;
      password = '';
      editing = false;
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!entry || !confirm($t('pw_delete_confirm', { name: entry.title }))) return;
    try {
      await api.passwords.delete(id);
      goto('/passwords', { replaceState: true });
    } catch (e) {
      fail(e);
    }
  }

  function generate() {
    password = generatePassword(get(pwSettings));
  }

  /** Leave the edit form and show the same password card again. */
  function closeEdit() {
    editing = false;
    addTotp = false;
    error = '';
    password = '';
    if (!entry) return;
    title = entry.title;
    username = entry.username ?? '';
    url = entry.url ?? '';
    totpIds = [...entry.totp_ids];
    tags = [...entry.tags];
    noteTouched = false;
    note = shownNote;
  }

  function avatarColor(s: string): string {
    let h = 0;
    for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return NAV_COLORS[h % NAV_COLORS.length];
  }

  /** Move a code to the front; the first one shows in the password list. */
  async function makePrimary(totpId: string) {
    if (!entry) return;
    if (notesLock.locked) {
      pendingPrimary = totpId;
      askUnlock('primary');
      return;
    }
    const next = [totpId, ...entry.totp_ids.filter((tid) => tid !== totpId)];
    try {
      entry = await api.passwords.update(id, { totp_ids: next });
      totpIds = next;
      await passwordStore.refresh();
    } catch (e) {
      fail(e);
    }
  }

  function onTags(next: string[]) {
    if (notesLock.locked) {
      pendingTags = next;
      askUnlock('tags');
      return;
    }
    void saveTags(next);
  }

  /** Persist tag removals from the cloud without opening the edit form. */
  async function saveTags(next: string[]) {
    const prev = tags;
    tags = next;
    try {
      entry = await api.passwords.update(id, { tags: next });
      await passwordStore.refresh();
      await passwordStore.dropStaleNoteBindings(id, next);
    } catch (e) {
      tags = prev;
      fail(e);
    }
  }
</script>

<div class="m-page">
  <div class="m-header">
    {#if editing}
      <button type="button" class="m-ibtn" onclick={closeEdit} aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></button>
    {:else}
      <a class="m-ibtn" href="/passwords" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    {/if}
    <h1 class="m-title">{entry?.title ?? $t('pw_title')}</h1>
    {#if entry && !editing}
      <button type="button" class="m-ibtn" onclick={() => (notesLock.locked ? askUnlock('edit') : (editing = true))} aria-label={$t('pw_btn_edit')}>
        <Icon name="pencil" size={22} />
      </button>
      <button type="button" class="m-ibtn" onclick={() => (notesLock.locked ? askUnlock('delete') : remove())} aria-label={$t('pw_btn_delete')}>
        <Icon name="trash-2" size={22} />
      </button>
    {/if}
  </div>
  <div class="m-body">
    {#if error}<div class="m-error">{error}</div>{/if}
    {#if missing}
      <div class="m-empty"><p>{$t('pw_empty')}</p></div>
    {:else if entry && editing}
      <div class="m-field"><label for="title">{$t('pw_field_title')}</label><input id="title" bind:value={title} /></div>
      <div class="m-field"><label for="user">{$t('pw_field_username')}</label><input id="user" bind:value={username} /></div>
      <div class="m-field"><label for="url">{$t('pw_field_url')}</label><input id="url" bind:value={url} /></div>
      <div class="m-field">
        <label for="pw">{$t('pw_field_password')}</label>
        <input id="pw" type="password" bind:value={password} placeholder={$t('pw_unchanged')} autocomplete="new-password" />
        <button type="button" class="btn btn-ghost btn-sm" onclick={generate}>{$t('pw_btn_generate')}</button>
      </div>
      <div class="m-field">
        <label for="note">{$t('pw_field_note')}</label>
        <textarea id="note" rows="3" bind:value={note} oninput={() => (noteTouched = true)}></textarea>
      </div>
      <PasswordTotpField
        entries={totp}
        bind:totpIds
        bind:creating={addTotp}
        bind:totpName
        bind:totpIssuer
        bind:totpSecret
        suggestName={title}
      />
      <EntityLabelField linkNotes {tags} onchange={(next) => (tags = next)} />
      <button class="btn btn-primary" onclick={save} disabled={saving}>{$t('pw_btn_save')}</button>
    {:else if entry}
      {#if entry.username}
        <p class="line">
          <span>{$t('pw_field_username')}</span>
          <strong class="val">{entry.username}</strong>
          <button type="button" class="icon-btn" class:ok={copiedKey === 'user'} onclick={() => copyField('user', entry?.username ?? '')} aria-label={$t('pw_btn_copy')}>
            <Icon name={copiedKey === 'user' ? 'check' : 'copy'} size={16} />
          </button>
        </p>
      {/if}
      {#if entry.url}
        <p class="line">
          <span>{$t('pw_field_url')}</span>
          <strong class="val">{entry.url}</strong>
          <button type="button" class="icon-btn" class:ok={copiedKey === 'url'} onclick={() => copyField('url', entry?.url ?? '')} aria-label={$t('pw_btn_copy')}>
            <Icon name={copiedKey === 'url' ? 'check' : 'copy'} size={16} />
          </button>
        </p>
      {/if}
      <p class="line">
        <span>{$t('pw_field_password')}</span>
        <strong class="mono val">{unlocked && revealed ? revealed : '••••••••'}</strong>
        <button type="button" class="icon-btn" onclick={reveal} aria-label={$t(revealed ? 'pw_btn_hide' : 'pw_btn_reveal')}>
          <Icon name={revealed ? 'eye-off' : 'eye'} size={16} />
        </button>
        <button type="button" class="icon-btn" class:ok={copiedKey === 'password'} onclick={copyPassword} aria-label={$t('pw_btn_copy')}>
          <Icon name={copiedKey === 'password' ? 'check' : 'copy'} size={16} />
        </button>
      </p>
      {#if shownNote}
        <div class="note">
          <span>{$t('pw_field_note')}</span>
          <p>{shownNote}</p>
        </div>
      {/if}
      {#each linked as item, i (item.id)}
        {@const totpLabel = item.issuer || item.name}
        <div class="m-card totp-card">
          <span class="m-avatar" style:--c={avatarColor(totpLabel)}>{totpLabel.charAt(0).toUpperCase()}</span>
          <span class="totp-meta">
            <span class="totp-name">{totpLabel}</span>
            {#if item.issuer}<span class="totp-account">{item.name}</span>{/if}
          </span>
          {#if linked.length > 1}
            {#if i === 0}
              <span class="primary" aria-label={$t('pw_totp_primary')}><Icon name="pin" size={16} /></span>
            {:else}
              <button type="button" class="icon-btn" onclick={() => makePrimary(item.id)} aria-label={$t('pw_totp_make_primary')}>
                <Icon name="arrow-up" size={16} />
              </button>
            {/if}
          {/if}
          <TotpLiveCode entryId={item.id} period={item.period} copiedLabel={$t('totp_copy')} compact />
        </div>
      {/each}
      {#if entry.tags.length || unlocked}
        <EntityLabelField linkNotes allowAdd={false} {tags} onchange={onTags} />
      {/if}
      {#if toast}<p class="ok">{toast}</p>{/if}
      <VaultUnlock bind:open={unlockOpen} onunlocked={onUnlocked} />
    {/if}
  </div>
</div>

<style>
  .line { display: flex; align-items: center; gap: var(--sp-2); margin: 0 0 var(--sp-2); min-width: 0; }
  .line span { color: var(--text-2); flex-shrink: 0; }
  .val { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .mono { font-family: var(--font-mono); }
  .icon-btn.ok { color: var(--success-text); }
  .note { display: flex; flex-direction: column; gap: 4px; margin: 0 0 var(--sp-3); }
  .note span { color: var(--text-2); font-size: var(--fs-sm); }
  .note p { margin: 0; white-space: pre-wrap; }
  .totp-card {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 64px;
    margin-bottom: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
  }
  .totp-meta { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .totp-name { font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .totp-account { font-size: 13px; color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .primary { display: inline-flex; color: var(--accent); }
  .ok { color: var(--success-text); }
</style>
