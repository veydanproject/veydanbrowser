<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import { messengerStore } from '../store.svelte';
  import { chatStore } from '../chats/chatStore.svelte';
  import ContactsPanel from '../contacts/ContactsPanel.svelte';
  import MobileFrame from './MobileFrame.svelte';
  import { BASE, chatHref } from './routes';

  onMount(() => { messengerStore.ensureLoaded().catch(() => {}); });

  async function write(pubkey: string) {
    const chat = await chatStore.openPeer(pubkey);
    goto(chatHref(chat.id));
  }
</script>

<MobileFrame title={$t('msg_contacts_title')} onback={() => goto(BASE)}>
  <ContactsPanel onchat={(pk) => write(pk).catch(() => {})} />
</MobileFrame>
