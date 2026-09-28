<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import { messengerStore } from '../store.svelte';
  import { contactLabel, messengerError, type MessengerContact } from '../api';
  import Avatar from './Avatar.svelte';

  let key = $state('');
  let nickname = $state('');
  let busy = $state(false);
  let error = $state('');
  let openId = $state<string | null>(null);
  let editNick = $state('');
  let editNote = $state('');
  let nip05Result = $state<Record<string, boolean>>({});

  const canAct = $derived(!!messengerStore.status?.runtime?.session_active);

  async function run(fn: () => Promise<unknown>) {
    error = '';
    busy = true;
    try { await fn(); }
    catch (e) { error = messengerError(e); }
    finally { busy = false; }
  }

  async function add() {
    if (!key.trim()) return;
    await run(async () => { await messengerStore.addContact(key, nickname); key = ''; nickname = ''; });
  }

  function toggle(c: MessengerContact) {
    if (openId === c.pubkey) { openId = null; return; }
    openId = c.pubkey;
    editNick = c.nickname ?? '';
    editNote = c.note ?? '';
  }

  const levels = ['all', 'mentions', 'none'] as const;
</script>

<div class="card contacts">
  <div class="card-title"><Icon name="user" size={16} /> {$t('msg_contacts_title')}</div>
  <p class="muted">{$t('msg_contacts_text')}</p>

  <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
    <input type="text" bind:value={key} placeholder={$t('msg_contacts_key_placeholder')} spellcheck="false" disabled={busy || !canAct} />
    <input type="text" class="nick" bind:value={nickname} placeholder={$t('msg_contacts_nick_placeholder')} disabled={busy || !canAct} />
    <button class="btn btn-primary" type="submit" disabled={busy || !canAct || !key.trim()}>
      <Icon name="plus" size={14} />{$t('msg_contacts_add')}
    </button>
  </form>
  {#if !canAct}<div class="muted small">{$t('msg_debug_no_session')}</div>{/if}
  {#if error}<div class="error-msg">{error}</div>{/if}

  {#if messengerStore.contacts.length === 0}
    <div class="muted small">{$t('msg_contacts_empty')}</div>
  {:else}
    <ul class="list">
      {#each messengerStore.contacts as c (c.pubkey)}
        <li class="row" class:open={openId === c.pubkey}>
          <button class="head" onclick={() => toggle(c)}>
            <Avatar url={c.profile?.picture ?? null} label={contactLabel(c)} />
            <span class="info">
              <span class="name">
                {contactLabel(c)}
                {#if c.is_muted}<Icon name="lock" size={11} />{/if}
                {#if c.followed}<span class="tag">{$t('msg_contacts_following')}</span>{/if}
              </span>
              <span class="meta">
                {#if c.profile?.nip05}
                  <span class:verified={c.profile.nip05_verified}>{c.profile.nip05}{c.profile.nip05_verified ? ' ✓' : ''}</span> ·
                {/if}
                <code>{c.npub.slice(0, 16)}…</code>
              </span>
            </span>
          </button>

          {#if openId === c.pubkey}
            <div class="details">
              {#if c.profile?.about}<p class="about">{c.profile.about}</p>{/if}
              <div class="grid">
                <label><span>{$t('msg_contacts_nickname')}</span><input type="text" bind:value={editNick} disabled={busy} /></label>
                <label><span>{$t('msg_contacts_note')}</span><input type="text" bind:value={editNote} disabled={busy} /></label>
                <label><span>{$t('msg_contacts_notifications')}</span>
                  <select value={c.notification_level} disabled={busy}
                    onchange={(e) => run(() => messengerStore.updateContact(c.pubkey, { notification_level: (e.currentTarget as HTMLSelectElement).value as typeof levels[number] }))}>
                    {#each levels as l}<option value={l}>{$t(`msg_contacts_level_${l}` as 'msg_contacts_level_all')}</option>{/each}
                  </select>
                </label>
              </div>
              <div class="actions">
                <button class="btn btn-primary btn-sm" disabled={busy}
                  onclick={() => run(() => messengerStore.updateContact(c.pubkey, { nickname: editNick.trim() || null, note: editNote.trim() || null }))}>
                  {$t('msg_contacts_save')}
                </button>
                <button class="btn btn-ghost btn-sm" disabled={busy}
                  onclick={() => run(() => messengerStore.updateContact(c.pubkey, { is_muted: !c.is_muted }))}>
                  {c.is_muted ? $t('msg_contacts_unmute') : $t('msg_contacts_mute')}
                </button>
                <button class="btn btn-ghost btn-sm" disabled={busy}
                  onclick={() => run(() => messengerStore.setFollowed(c.pubkey, !c.followed))}>
                  {c.followed ? $t('msg_contacts_unfollow') : $t('msg_contacts_follow')}
                </button>
                <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => run(() => messengerStore.requestProfile(c.pubkey))}>
                  <Icon name="refresh-cw" size={12} />{$t('msg_contacts_refresh')}
                </button>
                {#if c.profile?.nip05}
                  <button class="btn btn-ghost btn-sm" disabled={busy}
                    onclick={() => run(async () => { nip05Result = { ...nip05Result, [c.pubkey]: await messengerStore.verifyNip05(c.pubkey) }; })}>
                    {$t('msg_contacts_verify_nip05')}{c.pubkey in nip05Result ? (nip05Result[c.pubkey] ? ' ✓' : ' ✗') : ''}
                  </button>
                {/if}
                <span class="spacer"></span>
                <button class="btn btn-ghost btn-sm danger" disabled={busy} onclick={() => run(() => messengerStore.removeContact(c.pubkey))}>
                  <Icon name="trash-2" size={12} />{$t('msg_contacts_remove')}
                </button>
              </div>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .contacts { max-width: 640px; display: flex; flex-direction: column; gap: var(--sp-3); }
  .card-title { display: flex; align-items: center; gap: var(--sp-2); }
  .muted { color: var(--text-2); font-size: var(--fs-sm); margin: 0; }
  .small { font-size: var(--fs-xs); }
  .add { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
  .add input {
    flex: 1; min-width: 180px; font: inherit; font-size: var(--fs-sm); color: var(--text);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 8px 10px;
  }
  .add input:first-child { font-family: var(--font-mono); font-size: var(--fs-xs); }
  .add input.nick { flex: 0 1 140px; }
  .add input:focus { outline: none; border-color: var(--accent-border); }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  .row { border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); }
  .row.open { border-color: var(--accent-border); }
  .head {
    display: flex; align-items: center; gap: var(--sp-3); width: 100%; text-align: left;
    background: none; border: none; color: inherit; font: inherit; cursor: pointer; padding: var(--sp-2) var(--sp-3);
  }
  .info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .name { display: flex; align-items: center; gap: 6px; font-weight: var(--fw-semibold); font-size: var(--fs-sm); }
  .tag { font-size: var(--fs-2xs); text-transform: uppercase; letter-spacing: 0.5px; padding: 1px 6px; border-radius: var(--radius-sm); background: var(--accent-tint); color: var(--accent-text-2); }
  .meta { color: var(--text-3); font-size: var(--fs-xs); }
  .meta code { font-family: var(--font-mono); }
  .verified { color: var(--success-text); }
  .details { display: flex; flex-direction: column; gap: var(--sp-2); padding: 0 var(--sp-3) var(--sp-3); border-top: 1px solid var(--border); padding-top: var(--sp-2); }
  .about { margin: 0; font-size: var(--fs-sm); color: var(--text-body); white-space: pre-wrap; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(160px, 1fr)); gap: var(--sp-2); }
  .grid label { display: flex; flex-direction: column; gap: 4px; font-size: var(--fs-xs); color: var(--text-3); }
  .grid input, .grid select {
    font: inherit; font-size: var(--fs-sm); color: var(--text);
    background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 6px 8px;
  }
  .actions { display: flex; gap: var(--sp-1); flex-wrap: wrap; align-items: center; }
  .spacer { flex: 1; }
  .btn.danger { color: var(--danger-text); }
</style>
