<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  A message came while the app is on the screen: the system notification
  stays away (the push handler knows the app is up), and this card says
  it instead, unless the chat it is about is open. A tap opens the chat;
  the card goes by itself after a moment.
-->
<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import { messengerStore } from '../store.svelte';
  import { chatStore } from '../chats/chatStore.svelte';
  import { BASE, chatHref } from '../mobile/routes';

  interface Notify { title: string; body: string | null; chat_id: string | null }

  const SHOWN_FOR_MS = 4500;
  const GROUP_CODES: Record<string, 'msg_notice_group_invite'> = {
    group_invite: 'msg_notice_group_invite',
    group_request: 'msg_notice_group_request' as 'msg_notice_group_invite',
    group_welcome: 'msg_notice_group_welcome' as 'msg_notice_group_invite',
  };

  let shown = $state<(Notify & { at: number }) | null>(null);
  let seenAt = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const head = messengerStore.feed[0];
    if (!head || head.name !== 'notify' || head.at <= seenAt) return;
    seenAt = head.at;
    const n = head.payload as Notify;
    // The chat it is about is on the screen: the message is already there.
    if (n.chat_id && page.url.pathname.startsWith(BASE) && chatStore.activeId === n.chat_id) return;
    shown = { ...n, at: head.at };
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => { shown = null; }, SHOWN_FOR_MS);
  });

  function text(n: Notify): string {
    if (!n.body) return $t('msg_notice_new');
    const code = GROUP_CODES[n.body];
    return code ? $t(code) : n.body;
  }

  function open() {
    const n = shown;
    shown = null;
    if (!n) return;
    goto(n.chat_id ? chatHref(n.chat_id) : BASE);
  }
</script>

{#if shown}
  <button class="notice" onclick={open} aria-live="polite">
    <span class="icon"><Icon name="bell" size={16} /></span>
    <span class="text">
      <span class="title">{shown.title}</span>
      <span class="body">{text(shown)}</span>
    </span>
  </button>
{/if}

<style>
  .notice {
    position: fixed; top: calc(var(--sat) + 8px); left: 12px; right: 12px; z-index: 70;
    display: flex; align-items: center; gap: var(--sp-3); text-align: left;
    padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--radius-md, 14px);
    background: var(--surface-1, var(--bg)); color: var(--text); font: inherit; cursor: pointer;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    animation: drop 220ms ease-out;
  }
  .icon { display: inline-flex; color: var(--accent-text-2); flex-shrink: 0; }
  .text { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .title { font-weight: var(--fw-semibold); font-size: var(--fs-sm); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .body { font-size: var(--fs-sm); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @keyframes drop { from { transform: translateY(-16px); opacity: 0; } to { transform: none; opacity: 1; } }
</style>
