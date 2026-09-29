<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { t } from '$lib/i18n';
  import { messengerStore } from '../store.svelte';
  import { chatStore } from '../chats/chatStore.svelte';
  import ChatWindow from '../dm/ChatWindow.svelte';
  import MobileFrame from './MobileFrame.svelte';
  import { BASE } from './routes';

  const id = $derived(page.url.searchParams.get('id') ?? '');
  let missing = $state(false);

  function back() {
    chatStore.close();
    if (history.length > 1) history.back();
    else goto(BASE, { replaceState: true });
  }

  async function load(chatId: string) {
    missing = false;
    if (!messengerStore.loaded) await messengerStore.refresh().catch(() => {});
    await messengerStore.startListeners().catch(() => {});
    if (!chatStore.chats.length) await chatStore.loadChats().catch(() => {});
    if (!chatStore.chats.some((c) => c.id === chatId)) { missing = true; return; }
    if (chatStore.activeId !== chatId) await chatStore.open(chatId);
  }

  $effect(() => { if (id) load(id); });

  onMount(() => {
    // Coming back from the background: catch up on what arrived meanwhile.
    const onVisible = () => {
      if (document.visibilityState === 'visible' && chatStore.activeId) {
        chatStore.reloadWindow().catch(() => {});
        chatStore.markRead(chatStore.activeId).catch(() => {});
      }
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  });
</script>

{#if chatStore.active && chatStore.active.id === id}
  <MobileFrame bare scroll={false}>
    <ChatWindow chat={chatStore.active} onback={back} />
  </MobileFrame>
{:else}
  <MobileFrame title={$t('msg_title')} onback={back}>
    <div class="note">{missing ? $t('msg_mobile_chat_missing') : $t('loading')}</div>
  </MobileFrame>
{/if}

<style>
  .note { padding: var(--sp-6) var(--sp-4); text-align: center; color: var(--text-2); font-size: var(--fs-sm); }
</style>
