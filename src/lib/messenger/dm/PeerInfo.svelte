<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  Everything about the person of a conversation of two except the
  conversation: who they are, their key, and what the chat has shared.
-->
<script lang="ts">
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import Avatar from '../contacts/Avatar.svelte';
  import CopyField from '../identity/CopyField.svelte';
  import MessageContent from '../content/MessageContent.svelte';
  import SharedMedia from '../content/shared/SharedMedia.svelte';
  import SharedList from '../content/shared/SharedList.svelte';
  import { nameStore } from '../groups/names.svelte';
  import type { MessengerChat, SharedSection } from '../api';

  interface Props {
    chat: MessengerChat;
    onclose: () => void;
  }
  let { chat, onclose }: Props = $props();

  /** A section of what the chat has shared, shown in place of the panel. */
  let section = $state<SharedSection | null>(null);

  const profile = $derived(chat.peer_pubkey ? nameStore.profile(chat.peer_pubkey) : null);
  const name = $derived(profile?.display_name?.trim() || profile?.name?.trim() || '');
</script>

<div class="panel">
  {#if section}
    <SharedList chatId={chat.id} {section} onback={() => (section = null)} />
  {:else}
    <header class="head">
      <span class="head-title">{$t('msg_peer_info')}</span>
      <button class="icon" onclick={onclose} title={$t('msg_back')}><Icon name="x" size={16} /></button>
    </header>

    <div class="scroll">
      <section class="card-top">
        <Avatar url={chat.picture} label={chat.title} seed={chat.peer_pubkey ?? chat.id} size={64} />
        <div class="name">{chat.title}</div>
        {#if name && name !== chat.title}<div class="sub">{name}</div>{/if}
        {#if profile?.nip05}
          <div class="sub nip05" class:ok={profile.nip05_verified}>
            <Icon name={profile.nip05_verified ? 'check-circle' : 'globe'} size={12} />{profile.nip05}
          </div>
        {/if}
        {#if profile?.about}<div class="about"><MessageContent text={profile.about} cards={false} /></div>{/if}
      </section>

      {#if chat.peer_npub}<CopyField label={$t('msg_peer_key')} value={chat.peer_npub} mono />{/if}

      <SharedMedia chatId={chat.id} onopen={(s) => (section = s)} />
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--surface); }
  .head { display: flex; align-items: center; gap: var(--sp-2); padding: var(--sp-2) var(--sp-3) var(--sp-2) var(--sp-4); min-height: 56px; border-bottom: 1px solid var(--border); flex-shrink: 0; }
  .head-title { flex: 1; font-weight: var(--fw-bold); font-size: var(--fs-base); }
  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-4); display: flex; flex-direction: column; gap: var(--sp-5); }
  .card-top { display: flex; flex-direction: column; align-items: center; gap: var(--sp-2); text-align: center; }
  .name { font-size: var(--fs-md); font-weight: var(--fw-extrabold); letter-spacing: -0.2px; overflow-wrap: anywhere; }
  .sub { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-xs); color: var(--text-3); overflow-wrap: anywhere; }
  .nip05.ok { color: var(--success-text, var(--success)); }
  .about { color: var(--text-body); }
  .icon { border: none; background: none; color: var(--text-2); cursor: pointer; display: inline-flex; padding: 6px; border-radius: var(--radius-sm); }
  .icon:hover { color: var(--text); background: var(--surface-3); }
  @media (pointer: coarse) { .icon { padding: 10px; } }
</style>
