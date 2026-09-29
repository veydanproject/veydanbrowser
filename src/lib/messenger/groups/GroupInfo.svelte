<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  Everything about a group except its conversation: who is in, in which
  role, who asks to be let in, the link, the settings, and the way out.
-->
<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { groupError } from './errors';
  import Icon from '$lib/Icon.svelte';
  import ContextMenu, { type MenuEntry } from '$lib/components/ui/ContextMenu.svelte';
  import Avatar from '../contacts/Avatar.svelte';
  import GroupLink from './GroupLink.svelte';
  import InvitePicker from './InvitePicker.svelte';
  import { groupStore } from './groupStore.svelte';
  import { nameStore, shortKey } from './names.svelte';
  import { canMute, canRemove, isManager, isOwner, rolesFor } from './permissions';
  import { chatStore } from '../chats/chatStore.svelte';
  import { confirmStore } from '../shared/confirm.svelte';
  import { longpress } from '../shared/longpress';
  import { type GroupAction, type MessengerGroup, type MessengerGroupMember } from '../api';

  const tr = (key: string, params?: Record<string, string>) => get(t)(key as "msg_you", params);

  interface Props {
    group: MessengerGroup;
    onclose: () => void;
    onforgotten?: () => void;
  }
  let { group, onclose, onforgotten }: Props = $props();

  let error = $state('');
  let busy = $state(false);
  let editing = $state(false);
  let inviting = $state(false);
  let name = $state('');
  let about = $state('');
  let history = $state(true);
  let menu = $state<{ open: boolean; x: number; y: number; m: MessengerGroupMember | null }>({ open: false, x: 0, y: 0, m: null });

  const joined = $derived(group.membership === 'joined');
  const manager = $derived(isManager(group));

  function explain(e: unknown): string {
    return groupError(e, tr);
  }

  async function run(fn: () => Promise<unknown>) {
    error = ''; busy = true;
    try { await fn(); return true; }
    catch (e) { error = explain(e); return false; }
    finally { busy = false; }
  }

  const act = (a: GroupAction) => run(() => groupStore.act(group.id, a));

  async function sure(text: string, cta: string, a: GroupAction) {
    if (await confirmStore.ask(text, cta, true)) await act(a);
  }

  function startEdit() {
    name = group.name; about = group.about; history = group.history_for_new; editing = true;
  }

  async function save() {
    const a: GroupAction = { op: 'edit_settings' };
    if (name.trim() && name.trim() !== group.name) a.name = name.trim();
    if (about !== group.about) a.about = about;
    if (group.kind === 'private' && history !== group.history_for_new) a.history_for_new = history;
    if (Object.keys(a).length === 1) { editing = false; return; }
    if (await act(a)) { editing = false; chatStore.scheduleChatsRefresh(); }
  }

  function openMenu(e: MouseEvent | { x: number; y: number }, m: MessengerGroupMember) {
    if ('preventDefault' in e) e.preventDefault();
    const x = 'clientX' in e ? e.clientX : e.x;
    const y = 'clientY' in e ? e.clientY : e.y;
    menu = { open: true, x, y, m };
  }

  const items = $derived.by((): MenuEntry[] => {
    const m = menu.m;
    if (!m) return [];
    const who = nameStore.label(m.pubkey);
    const list: MenuEntry[] = [];
    if (!m.is_me) list.push({ label: $t('msg_group_member_write'), icon: 'message-circle', onselect: () => { chatStore.openPeer(m.pubkey).catch(() => {}); onclose(); } });
    for (const role of rolesFor(group, m)) {
      list.push({ label: $t('msg_group_member_make', { role: $t(`msg_group_role_${role}` as 'msg_group_role_admin') }), icon: 'shield', onselect: () => act({ op: 'set_role', who: m.pubkey, role }) });
    }
    if (canMute(group, m)) {
      list.push({ label: m.muted ? $t('msg_group_member_unmute') : $t('msg_group_member_mute'), icon: 'bell-off', onselect: () => act({ op: 'set_muted', who: m.pubkey, muted: !m.muted }) });
    }
    if (isOwner(group) && !m.is_me) {
      list.push({ label: $t('msg_group_member_transfer'), icon: 'key', onselect: () => sure($t('msg_group_transfer_confirm', { name: who }), $t('msg_group_member_transfer'), { op: 'transfer_ownership', to: m.pubkey }) });
    }
    if (canRemove(group, m)) {
      list.push({ type: 'separator' });
      list.push({ label: $t('msg_group_member_remove'), icon: 'x', danger: true, onselect: () => sure($t('msg_group_remove_confirm', { name: who }), $t('msg_group_member_remove'), { op: 'remove', who: m.pubkey }) });
      list.push({ label: $t('msg_group_member_ban'), icon: 'ban', danger: true, onselect: () => sure($t('msg_group_ban_confirm', { name: who }), $t('msg_group_member_ban'), { op: 'ban', who: m.pubkey }) });
    }
    return list;
  });

  async function leave() {
    if (await confirmStore.ask($t('msg_group_leave_confirm', { name: group.name }), $t('msg_group_leave'), true)) await act({ op: 'leave' });
  }

  async function disband() {
    if (await confirmStore.ask($t('msg_group_disband_confirm', { name: group.name }), $t('msg_group_disband'), true)) await act({ op: 'disband' });
  }

  async function forget() {
    if (!(await confirmStore.ask($t('msg_group_forget_confirm', { name: group.name }), $t('msg_group_forget'), true))) return;
    if (await run(() => groupStore.forget(group.id))) onforgotten?.();
  }
</script>

<div class="panel">
  <header class="head">
    <span class="head-title">{$t('msg_group_info')}</span>
    <button class="icon" onclick={onclose} title={$t('msg_back')}><Icon name="x" size={16} /></button>
  </header>

  <div class="scroll">
    <section class="card-top">
      <Avatar url={group.picture || null} label={group.name} seed={group.id} size={64} />
      {#if editing}
        <input class="field" type="text" bind:value={name} maxlength="64" placeholder={$t('msg_group_name')} disabled={busy} />
        <textarea class="field" rows="3" bind:value={about} maxlength="500" placeholder={$t('msg_group_about')} disabled={busy}></textarea>
        {#if group.kind === 'private'}
          <label class="check"><input type="checkbox" bind:checked={history} disabled={busy} /><span>{$t('msg_group_history_for_new')}</span></label>
        {/if}
        <div class="row">
          <button class="btn btn-primary btn-sm" disabled={busy || !name.trim()} onclick={save}>{$t('msg_group_save')}</button>
          <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => (editing = false)}>{$t('msg_group_cancel')}</button>
        </div>
      {:else}
        <div class="name">{group.name}</div>
        <div class="kind">
          <Icon name={group.kind === 'public' ? 'globe' : 'lock'} size={12} />
          {$t(group.kind === 'public' ? 'msg_group_kind_public' : 'msg_group_kind_private')} ·
          {$t('msg_group_members_n', { n: String(group.members.length) })}
        </div>
        {#if group.about}<p class="about">{group.about}</p>{/if}
        <p class="note">
          {#if group.kind === 'public'}{$t('msg_group_note_public')}
          {:else if group.history_for_new}{$t('msg_group_note_history_on')}
          {:else}{$t('msg_group_note_history_off')}{/if}
        </p>
        {#if manager}<button class="btn btn-ghost btn-sm" onclick={startEdit}><Icon name="pencil" size={13} />{$t('msg_group_edit')}</button>{/if}
      {/if}
    </section>

    {#if error}<div class="error-msg">{error}</div>{/if}

    {#if !joined}
      <div class="state">{$t(`msg_group_state_${group.membership}` as 'msg_group_state_left')}</div>
    {/if}

    {#if joined && group.requests.length}
      <section class="block">
        <div class="label">{$t('msg_group_requests')} · {group.requests.length}</div>
        <ul class="list">
          {#each group.requests as r (r)}
            <li class="person">
              <Avatar url={nameStore.picture(r)} label={nameStore.label(r)} seed={r} size={34} />
              <span class="who"><span class="who-name">{nameStore.label(r)}</span><code>{shortKey(r)}</code></span>
              <button class="btn btn-primary btn-sm" disabled={busy} onclick={() => run(() => groupStore.answerRequest(group.id, r, true))}>{$t('msg_group_request_approve')}</button>
              <button class="icon" disabled={busy} title={$t('msg_group_request_reject')} onclick={() => run(() => groupStore.answerRequest(group.id, r, false))}><Icon name="x" size={15} /></button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if joined}<GroupLink {group} />{/if}

    <section class="block">
      <div class="label-row">
        <span class="label">{$t('msg_group_members')} · {group.members.length}</span>
        {#if manager}<button class="btn btn-ghost btn-sm" onclick={() => (inviting = true)}><Icon name="user-plus" size={13} />{$t('msg_group_invite')}</button>{/if}
      </div>
      <ul class="list">
        {#each group.members as m (m.pubkey)}
          <li>
            <button class="person as-button" oncontextmenu={(e) => openMenu(e, m)} onclick={(e) => openMenu(e, m)}
              use:longpress={{ onpress: (p) => openMenu(p, m) }}>
              <Avatar url={nameStore.picture(m.pubkey)} label={nameStore.label(m.pubkey)} seed={m.pubkey} size={34} />
              <span class="who">
                <span class="who-name">{nameStore.label(m.pubkey)}{#if m.is_me} <span class="me">({$t('msg_you')})</span>{/if}</span>
                <code>{shortKey(m.pubkey)}</code>
              </span>
              {#if m.muted}<span class="dim" title={$t('msg_group_member_muted')}><Icon name="bell-off" size={13} /></span>{/if}
              {#if m.role !== 'member'}<span class="role {m.role}">{$t(`msg_group_role_${m.role}` as 'msg_group_role_admin')}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    </section>

    {#if manager && group.banned.length}
      <section class="block">
        <div class="label">{$t('msg_group_banned')} · {group.banned.length}</div>
        <ul class="list">
          {#each group.banned as b (b)}
            <li class="person">
              <Avatar url={nameStore.picture(b)} label={nameStore.label(b)} seed={b} size={34} />
              <span class="who"><span class="who-name">{nameStore.label(b)}</span><code>{shortKey(b)}</code></span>
              <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => act({ op: 'unban', who: b })}>{$t('msg_group_unban')}</button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <section class="block out">
      {#if joined && !isOwner(group)}
        <button class="btn btn-ghost btn-sm danger" disabled={busy} onclick={leave}><Icon name="x" size={13} />{$t('msg_group_leave')}</button>
      {/if}
      {#if isOwner(group)}
        <p class="hint">{$t('msg_group_owner_hint')}</p>
        <button class="btn btn-ghost btn-sm danger" disabled={busy} onclick={disband}><Icon name="trash-2" size={13} />{$t('msg_group_disband')}</button>
      {/if}
      {#if !joined && group.membership !== 'joining'}
        <button class="btn btn-ghost btn-sm danger" disabled={busy} onclick={forget}><Icon name="trash-2" size={13} />{$t('msg_group_forget')}</button>
      {/if}
    </section>
  </div>
</div>

<InvitePicker bind:open={inviting} {group} />
<ContextMenu bind:open={menu.open} x={menu.x} y={menu.y} {items} onclose={() => (menu.open = false)} />

<style>
  .panel { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--surface); }
  .head { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-2) var(--sp-3) var(--sp-2) var(--sp-4); min-height: 56px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .head-title { flex: 1; font-weight: var(--fw-bold); font-size: var(--fs-base); }
  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-4); display: flex; flex-direction: column; gap: var(--sp-5); }
  .card-top { display: flex; flex-direction: column; align-items: center; gap: var(--sp-2); text-align: center; }
  .name { font-size: var(--fs-md); font-weight: var(--fw-extrabold); letter-spacing: -0.2px; overflow-wrap: anywhere; }
  .kind { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-xs); color: var(--text-3); }
  .about { margin: 0; font-size: var(--fs-sm); color: var(--text-body); line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; }
  .note, .hint { margin: 0; font-size: var(--fs-xs); color: var(--text-3); line-height: 1.45; }
  .field {
    width: 100%; font: inherit; font-size: var(--fs-sm); color: var(--text); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: var(--radius-field); padding: 8px 10px; resize: vertical;
  }
  .field:focus { outline: none; border-color: var(--accent-border); }
  .check { display: flex; align-items: flex-start; gap: 8px; font-size: var(--fs-xs); color: var(--text-2); text-align: left; line-height: 1.4; }
  .row { display: flex; gap: 6px; flex-wrap: wrap; justify-content: center; }
  .state { padding: var(--sp-2) var(--sp-3); border-radius: var(--radius-md); background: var(--warn-bg); border: 1px solid var(--warn-border); color: var(--warn-text); font-size: var(--fs-xs); line-height: 1.45; }
  .block { display: flex; flex-direction: column; gap: var(--sp-2); }
  .block.out { align-items: flex-start; }
  .label { font-size: var(--fs-2xs); text-transform: uppercase; letter-spacing: 0.6px; color: var(--text-3); font-weight: var(--fw-bold); }
  .label-row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-2); }
  .list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  .person { display: flex; align-items: center; gap: var(--sp-3); padding: 6px 8px; border-radius: var(--radius-sm); min-width: 0; }
  .as-button { width: 100%; border: none; background: none; color: inherit; font: inherit; text-align: left; cursor: pointer; }
  .as-button:hover { background: var(--surface-row-hover); }
  .who { display: flex; flex-direction: column; gap: 1px; min-width: 0; flex: 1; }
  .who-name { font-size: var(--fs-sm); font-weight: var(--fw-semibold); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who code { font-family: var(--font-mono); font-size: var(--fs-2xs); color: var(--text-3); }
  .me { color: var(--text-3); font-weight: var(--fw-normal, 400); }
  .dim { color: var(--text-3); display: inline-flex; }
  .role { flex-shrink: 0; font-size: var(--fs-2xs); font-weight: var(--fw-bold); padding: 2px 7px; border-radius: var(--radius-pill); background: var(--surface-3); color: var(--text-2); }
  .role.owner { background: var(--accent-tint); color: var(--accent-text-2); }
  .role.admin { background: var(--success-bg); color: var(--success-text, var(--success)); }
  .icon { border: none; background: none; color: var(--text-2); cursor: pointer; display: inline-flex; padding: 6px; border-radius: var(--radius-sm); }
  .icon:hover { color: var(--text); background: var(--surface-3); }
  .danger { color: var(--danger-text); }
  @media (pointer: coarse) {
    .icon { padding: 10px; }
    .person { padding: 9px 8px; }
    .field { font-size: 16px; }
  }
</style>
