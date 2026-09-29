<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import ContextMenu, { type MenuEntry } from '$lib/components/ui/ContextMenu.svelte';
  import Avatar from '../contacts/Avatar.svelte';
  import MessageBubble from './MessageBubble.svelte';
  import Composer from './Composer.svelte';
  import RelationBanner from './RelationBanner.svelte';
  import { chatStore } from '../chats/chatStore.svelte';
  import { messengerStore } from '../store.svelte';
  import { dayKey, dayLabel } from '../shared/time';
  import { dmErrorCode, messengerError, type DmAction, type MessengerChat, type MessengerMessage } from '../api';

  interface Props {
    chat: MessengerChat;
    onback?: () => void;
    actions?: Snippet;
    banner?: Snippet;
    /** Replaces the composer when the chat cannot be written to. */
    footer?: Snippet;
  }
  let { chat, onback, actions, banner, footer }: Props = $props();

  let scroller = $state<HTMLDivElement | null>(null);
  let replyTo = $state<MessengerMessage | null>(null);
  let editing = $state<MessengerMessage | null>(null);
  let error = $state('');
  let atBottom = $state(true);
  let highlighted = $state<string | null>(null);
  let menu = $state<{ open: boolean; x: number; y: number; m: MessengerMessage | null }>({ open: false, x: 0, y: 0, m: null });
  let chatMenu = $state<{ open: boolean; x: number; y: number }>({ open: false, x: 0, y: 0 });
  let acting = $state(false);

  const online = $derived((messengerStore.status?.runtime?.relays_connected ?? 0) > 0);
  const sessionActive = $derived(!!messengerStore.status?.runtime?.session_active);

  // Leaving a chat drops reply/edit state.
  let lastChat = '';
  $effect(() => {
    if (chat.id !== lastChat) { lastChat = chat.id; replyTo = null; editing = null; error = ''; atBottom = true; }
  });

  // New message at the tail: follow it when the user is already at the bottom.
  $effect(() => {
    void chatStore.tailTick;
    if (atBottom) tick().then(scrollToBottom);
  });

  function scrollToBottom() {
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }

  async function onscroll() {
    if (!scroller) return;
    atBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 80;
    if (scroller.scrollTop < 120 && chatStore.hasMore && !chatStore.loadingOlder) {
      const before = scroller.scrollHeight;
      await chatStore.loadOlder();
      await tick();
      // Keep the viewport on the same message after older ones are prepended.
      if (scroller) scroller.scrollTop += scroller.scrollHeight - before;
    }
  }

  /** Relationship refusals arrive as stable codes; everything else as text. */
  function explain(e: unknown): string {
    const code = dmErrorCode(e);
    return code ? $t(`msg_err_${code}` as "msg_err_dm_blocked", { name: chat.title }) : messengerError(e);
  }

  async function act(a: DmAction) {
    if (a === "block" && !confirm($t("msg_rel_confirm_block", { name: chat.title }))) return;
    if (a === "remove" && chat.is_contact && chat.mode === "full_chat" && !confirm($t("msg_rel_confirm_remove", { name: chat.title }))) return;
    error = ""; acting = true;
    try { await chatStore.act(chat.id, a); }
    catch (e) { error = explain(e); }
    finally { acting = false; }
  }

  function openChatMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    chatMenu = { open: true, x: r.right - 200, y: r.bottom + 4 };
  }

  const chatItems = $derived.by((): MenuEntry[] => {
    const list: MenuEntry[] = [];
    if (chat.mode === "blocked") list.push({ label: $t("msg_rel_cta_unblock"), icon: "lock-open", onselect: () => act("unblock") });
    else {
      if (chat.is_contact) list.push({ label: $t("msg_rel_cta_remove"), icon: "user", onselect: () => act("remove") });
      else if (chat.mode !== "request_received") list.push({ label: $t("msg_rel_cta_add"), icon: "user-plus", onselect: () => act("request") });
      list.push({ label: $t("msg_rel_cta_block"), icon: "ban", danger: true, onselect: () => act("block") });
    }
    list.push({ type: "separator" });
    list.push({ label: chat.pinned ? $t("msg_chat_unpin") : $t("msg_chat_pin"), icon: "pin", onselect: () => chatStore.setPinned(chat.id, !chat.pinned) });
    list.push({ label: chat.archived ? $t("msg_chat_unarchive") : $t("msg_chat_archive"), icon: "archive", onselect: () => chatStore.setArchived(chat.id, !chat.archived) });
    list.push({ label: $t("msg_chat_delete"), icon: "trash-2", danger: true, onselect: () => { if (confirm($t("msg_chat_delete_confirm", { name: chat.title }))) chatStore.deleteChat(chat.id); } });
    return list;
  });

  async function guard(fn: () => Promise<unknown>) {
    error = '';
    try { await fn(); } catch (e) { error = explain(e); }
  }

  async function send(text: string) {
    error = '';
    try {
      if (editing) {
        const id = editing.id;
        editing = null;
        await chatStore.edit(id, text);
      } else {
        const r = replyTo?.id;
        replyTo = null;
        atBottom = true;
        await chatStore.send(text, r);
      }
    } catch (e) {
      error = explain(e);
      throw e;
    }
  }

  function openMenu(e: MouseEvent, m: MessengerMessage) {
    e.preventDefault();
    menu = { open: true, x: e.clientX, y: e.clientY, m };
  }

  async function jumpTo(id: string) {
    const el = scroller?.querySelector(`[data-mid="${id}"]`);
    if (!el) return;
    el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    highlighted = id;
    setTimeout(() => { if (highlighted === id) highlighted = null; }, 1400);
  }

  const items = $derived.by((): MenuEntry[] => {
    const m = menu.m;
    if (!m) return [];
    const own = m.direction === 'out';
    const list: MenuEntry[] = [];
    if (!m.deleted) {
      if (chat.can_send) list.push({ label: $t('msg_message_reply'), icon: 'reply', onselect: () => { editing = null; replyTo = m; } });
      if (m.text) list.push({ label: $t('msg_copy'), icon: 'copy', onselect: () => navigator.clipboard.writeText(m.text ?? '').catch(() => {}) });
      if (own && m.content_type === 'text' && chat.can_send) list.push({ label: $t('msg_message_edit'), icon: 'pencil', onselect: () => { replyTo = null; editing = m; } });
      if (own && m.status === 'failed') list.push({ label: $t('msg_message_retry'), icon: 'refresh-cw', onselect: () => guard(() => chatStore.retry(m.id)) });
      list.push({ type: 'separator' });
      list.push({ label: $t('msg_message_delete_me'), icon: 'trash-2', danger: true, onselect: () => guard(() => chatStore.remove(m.id, false)) });
      if (own) list.push({ label: $t('msg_message_delete_all'), icon: 'trash-2', danger: true, onselect: () => guard(() => chatStore.remove(m.id, true)) });
    }
    return list;
  });

  interface Row { m: MessengerMessage; first: boolean; day: string | null }
  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    let prev: MessengerMessage | null = null;
    for (const m of chatStore.messages) {
      const newDay = !prev || dayKey(prev.created_at) !== dayKey(m.created_at);
      const first = newDay || !prev || prev.sender_pubkey !== m.sender_pubkey || prev.content_type === 'system' || m.created_at - prev.created_at > 300;
      let day: string | null = null;
      if (newDay) {
        const l = dayLabel(m.created_at);
        day = l.key ? $t(`msg_day_${l.key}` as 'msg_day_today') : l.text;
      }
      out.push({ m, first, day });
      prev = m;
    }
    return out;
  });
</script>

<section class="window">
  <header class="head">
    {#if onback}<button class="icon back" onclick={onback} title={$t('msg_back')}><Icon name="arrow-left" size={16} /></button>{/if}
    <Avatar url={chat.picture} label={chat.title} size={36} />
    <div class="who">
      <div class="title">{chat.title}{#if chat.is_muted}<span class="dim"><Icon name="bell-off" size={12} /></span>{/if}</div>
      <div class="sub">
        {#if !sessionActive}{$t('msg_chat_locked')}
        {:else if !online}<span class="offline">{$t('msg_chat_offline')}</span>
        {:else}<code>{chat.peer_npub ? `${chat.peer_npub.slice(0, 14)}…${chat.peer_npub.slice(-6)}` : ''}</code>{/if}
      </div>
    </div>
    {#if actions}{@render actions()}{/if}
    <button class="icon" onclick={openChatMenu} title={$t("msg_chat_menu")}><Icon name="more-vertical" size={16} /></button>
  </header>

  <RelationBanner {chat} busy={acting} onaction={act} />
  {#if banner}{@render banner()}{/if}

  <div class="scroll" bind:this={scroller} {onscroll}>
    {#if chatStore.loadingOlder}<div class="loading"><Icon name="loader" size={14} /></div>{/if}
    {#if chatStore.loading && rows.length === 0}
      <div class="placeholder">{$t('loading')}</div>
    {:else if rows.length === 0}
      <div class="placeholder">
        <Icon name="lock" size={22} />
        <p>{$t('msg_chat_empty')}</p>
        <span>{$t('msg_chat_empty_hint')}</span>
      </div>
    {:else}
      {#each rows as r (r.m.id)}
        {#if r.day}<div class="day"><span>{r.day}</span></div>{/if}
        {#if r.m.content_type === 'system'}
          <div class="system"><span>{$t(`msg_sys_${r.m.text}` as 'msg_sys_request_sent', { name: chat.title })}</span></div>
        {:else}
          <MessageBubble message={r.m} first={r.first} peerTitle={chat.title} highlighted={highlighted === r.m.id}
            onmenu={openMenu} onreplyclick={jumpTo} onretry={(m) => guard(() => chatStore.retry(m.id))} />
        {/if}
      {/each}
    {/if}
  </div>

  {#if !atBottom}
    <button class="to-bottom" onclick={() => { atBottom = true; scrollToBottom(); }} title={$t('msg_chat_to_bottom')}>
      <Icon name="chevrons-down" size={16} />
    </button>
  {/if}

  {#if error}<div class="error-line">{error}</div>{/if}

  {#if chat.can_send}
    <Composer {replyTo} {editing} peerTitle={chat.title} disabled={!sessionActive}
      oncancel={() => { replyTo = null; editing = null; }} onsend={send} />
  {:else if footer}
    {@render footer()}
  {:else}
    <div class="no-composer"><Icon name="lock" size={13} />{$t("msg_chat_cannot_send")}</div>
  {/if}
</section>

<ContextMenu bind:open={menu.open} x={menu.x} y={menu.y} {items} onclose={() => (menu.open = false)} />
<ContextMenu bind:open={chatMenu.open} x={chatMenu.x} y={chatMenu.y} items={chatItems} onclose={() => (chatMenu.open = false)} />

<style>
  .window { position: relative; display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--bg); }
  .head { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-4); border-bottom: 1px solid var(--border); background: var(--surface); flex-shrink: 0; min-height: 56px; }
  .who { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .title { display: flex; align-items: center; gap: 6px; font-weight: var(--fw-bold); font-size: var(--fs-base); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dim { color: var(--text-3); display: inline-flex; }
  .sub { font-size: var(--fs-2xs); color: var(--text-3); }
  .sub code { font-family: var(--font-mono); }
  .offline { color: var(--warn-text); }
  .icon { border: none; background: none; color: var(--text-2); cursor: pointer; display: inline-flex; padding: 6px; border-radius: var(--radius-sm); }
  .icon:hover { color: var(--text); background: var(--surface-3); }
  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-3) 0; display: flex; flex-direction: column; }
  .scroll > :global(:first-child) { margin-top: auto; }
  .day, .system { display: flex; justify-content: center; margin: var(--sp-3) 0 var(--sp-1); }
  .day span { font-size: var(--fs-2xs); color: var(--text-3); background: var(--surface-2); border: 1px solid var(--border); padding: 3px 10px; border-radius: var(--radius-pill); }
  .system { margin: var(--sp-2) var(--sp-4); }
  .system span { font-size: var(--fs-xs); color: var(--text-2); text-align: center; line-height: 1.4; }
  .placeholder { margin: auto; display: flex; flex-direction: column; align-items: center; gap: var(--sp-2); color: var(--text-3); text-align: center; padding: var(--sp-6); }
  .placeholder p { margin: 0; color: var(--text-body); font-weight: var(--fw-semibold); font-size: var(--fs-sm); }
  .placeholder span { font-size: var(--fs-xs); max-width: 320px; line-height: 1.5; }
  .loading { display: flex; justify-content: center; color: var(--text-3); padding: var(--sp-2); }
  .to-bottom {
    position: absolute; right: var(--sp-4); bottom: 84px; width: 36px; height: 36px; border-radius: 50%;
    border: 1px solid var(--border); background: var(--surface); color: var(--text-2); cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center; box-shadow: var(--shadow);
  }
  .to-bottom:hover { color: var(--text); }
  .error-line { padding: 6px var(--sp-4); font-size: var(--fs-xs); color: var(--danger-text); background: var(--danger-bg); border-top: 1px solid var(--danger-border); }
  .no-composer { display: flex; align-items: center; justify-content: center; gap: 6px; padding: var(--sp-3); border-top: 1px solid var(--border); background: var(--surface); font-size: var(--fs-xs); color: var(--text-3); }
</style>
