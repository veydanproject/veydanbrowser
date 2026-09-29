<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import { clock } from '../shared/time';
  import type { MessengerMessage } from '../api';

  interface Props {
    message: MessengerMessage;
    /** First bubble of a run from the same sender: gets the tail corner. */
    first: boolean;
    peerTitle: string;
    highlighted?: boolean;
    onmenu: (e: MouseEvent, m: MessengerMessage) => void;
    onreplyclick: (id: string) => void;
    onretry: (m: MessengerMessage) => void;
    /** Renders the attachment of a `media` message (stage 6). */
    media?: Snippet<[MessengerMessage]>;
  }
  let { message: m, first, peerTitle, highlighted = false, onmenu, onreplyclick, onretry, media }: Props = $props();

  const out = $derived(m.direction === 'out');
  const statusIcon = $derived(
    m.status === 'sent' ? 'check' : m.status === 'failed' ? 'alert-triangle' : m.status === 'uploading' ? 'upload' : 'clock',
  );
  const statusTitle = $derived($t(`msg_status_${m.status}` as 'msg_status_sent'));
</script>

<div class="line" class:out class:first class:highlighted data-mid={m.id}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="bubble" class:deleted={m.deleted} class:failed={m.status === 'failed'} oncontextmenu={(e) => onmenu(e, m)}>
    {#if m.reply_to && !m.deleted}
      <button class="reply" onclick={() => onreplyclick(m.reply_to!.id)}>
        <span class="reply-who">{m.reply_to.sender_pubkey === m.sender_pubkey && out || m.reply_to.sender_pubkey !== m.sender_pubkey && !out ? $t('msg_you') : peerTitle}</span>
        <span class="reply-text">{m.reply_to.text ?? $t('msg_message_deleted')}</span>
      </button>
    {/if}

    {#if m.deleted}
      <span class="tomb"><Icon name="ban" size={12} /> {$t('msg_message_deleted')}</span>
    {:else if m.content_type === 'media' && media}
      {@render media(m)}
      {#if m.text}<span class="text">{m.text}</span>{/if}
    {:else if m.text}
      <span class="text">{m.text}</span>
    {:else}
      <span class="tomb">{$t('msg_message_unsupported', { type: m.content_type })}</span>
    {/if}

    <span class="meta">
      {#if m.edited_at && !m.deleted}<span>{$t('msg_message_edited')}</span>{/if}
      <span>{clock(m.created_at)}</span>
      {#if out && !m.deleted}
        <span class="status {m.status}" title={statusTitle}><Icon name={statusIcon} size={12} /></span>
      {/if}
    </span>
  </div>
  {#if m.status === 'failed' && out && !m.id.startsWith('local:')}
    <button class="retry" onclick={() => onretry(m)} title={m.failure_reason ?? ''}>
      <Icon name="refresh-cw" size={12} />{$t('msg_message_retry')}
    </button>
  {/if}
</div>

<style>
  .line { display: flex; flex-direction: column; align-items: flex-start; padding: 1px var(--sp-4); border-radius: var(--radius-sm); transition: background 0.6s var(--ease); }
  .line.out { align-items: flex-end; }
  .line.first { margin-top: var(--sp-2); }
  .line.highlighted { background: var(--accent-tint); }
  .bubble {
    position: relative; max-width: min(620px, 78%); padding: 7px 11px 6px;
    background: var(--surface-2); color: var(--text); border: 1px solid var(--border);
    border-radius: 14px; display: flex; flex-direction: column; gap: 3px; min-width: 64px;
  }
  .line.first .bubble { border-top-left-radius: 5px; }
  .line.out .bubble { background: var(--accent-tint); border-color: var(--accent-tint-border); }
  .line.out.first .bubble { border-top-left-radius: 14px; border-top-right-radius: 5px; }
  .bubble.failed { border-color: var(--danger-border); }
  .text { font-size: var(--fs-sm); line-height: 1.45; white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
  .tomb { display: inline-flex; align-items: center; gap: 5px; font-size: var(--fs-xs); color: var(--text-3); font-style: italic; }
  .meta { display: inline-flex; align-items: center; gap: 5px; align-self: flex-end; font-size: var(--fs-2xs); color: var(--text-3); line-height: 1; }
  .status { display: inline-flex; }
  .status.sent { color: var(--accent-text-2); }
  .status.failed { color: var(--danger-text); }
  .reply {
    display: flex; flex-direction: column; gap: 1px; text-align: left; width: 100%;
    border: none; border-left: 2px solid var(--accent); border-radius: 4px; background: var(--surface-3);
    padding: 3px 8px; color: inherit; font: inherit; cursor: pointer; min-width: 0;
  }
  .line.out .reply { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .reply-who { font-size: var(--fs-2xs); font-weight: var(--fw-bold); color: var(--accent-text-2); }
  .reply-text { font-size: var(--fs-xs); color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 420px; }
  .retry {
    display: inline-flex; align-items: center; gap: 4px; margin-top: 2px; border: none; background: none;
    color: var(--danger-text); font: inherit; font-size: var(--fs-2xs); cursor: pointer; padding: 2px 4px;
  }
  .retry:hover { text-decoration: underline; }
</style>
