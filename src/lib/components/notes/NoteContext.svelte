<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Context cards: Veydan entities bound to or mentioned in a note, with actions. -->
<script lang="ts">
  import { goto } from '$app/navigation';
  import Icon from '$lib/Icon.svelte';
  import { t, type TranslationKey } from '$lib/i18n';
  import { formatError, hasErrorCode } from '$lib/utils';
  import { parseBinding, isEntityKind, type EntityKind } from '$lib/bindings';
  import { ENTITY_DEFS, entitySummary, type EntityAction, type EntityStatus } from '$lib/notes-context';
  import { totpStore } from '$lib/store/totp.svelte';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { onMount } from 'svelte';

  const totpPeriod = (id: string) => totpStore.list.find((e) => e.id === id)?.period ?? 30;

  function totpName(id: string): string {
    const entry = totpStore.list.find((e) => e.id === id);
    if (!entry) return '';
    return entry.issuer ? `${entry.issuer} · ${entry.name}` : entry.name;
  }

  function markCopied(key: string) {
    copied = '';
    requestAnimationFrame(() => (copied = key));
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied = ''), 1600);
  }

  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    void totpStore.ensureLoaded();
    void passwordStore.ensureLoaded();
    void notesStore.ensureLoaded();
    void profilesStore.ensureLoaded();
    void workspacesStore.ensureLoaded();
    return () => clearTimeout(copyTimer);
  });

  const STATUS_LABEL: Record<EntityStatus, TranslationKey> = {
    ok: 'ctx_status_ok',
    bad: 'ctx_status_bad',
    unknown: 'ctx_status_unknown',
  };

  interface Props {
    /** Bindings stored on the note */
    bindings: string[];
    /** When set, passwords tagged `note:id` also appear as context cards. */
    noteId?: string | null;
    /** `[[kind:id]]` mentions found in the body */
    mentions: string[];
    /** Binding to highlight, e.g. after clicking a mention in the text */
    focus?: string | null;
    readonly?: boolean;
    onbind: (binding: string) => void;
    onunbind: (binding: string) => void;
  }

  let { bindings, mentions, focus = null, readonly = false, noteId = null, onbind, onunbind }: Props = $props();

  interface Card {
    binding: string;
    kind: EntityKind;
    id: string;
    bound: boolean;
    /** Link stored on the password tags, not on the note. */
    fromTag: boolean;
  }

  const cards = $derived.by((): Card[] => {
    const out: Card[] = [];
    const seen = new Set<string>();
    const push = (b: string, bound: boolean, fromTag = false) => {
      const p = parseBinding(b);
      if (!p || !isEntityKind(p.kind) || !p.value || seen.has(b)) return;
      seen.add(b);
      out.push({ binding: b, kind: p.kind, id: p.value, bound, fromTag });
    };
    for (const b of bindings) push(b, true);
    for (const b of mentions) push(b, false);
    if (noteId) {
      for (const entry of passwordStore.list) {
        if (entry.tags.includes(`note:${noteId}`)) push(`password:${entry.id}`, true, true);
      }
    }
    return out;
  });

  let busy = $state<string | null>(null);
  let copied = $state('');
  let codesOpen = $state<Record<string, boolean>>({});
  let error = $state<string | null>(null);
  let unlockOpen = $state(false);
  let pendingCopy = $state<Card | null>(null);

  function chipColor(tag: string): string | undefined {
    const parsed = parseBinding(tag);
    if (!parsed) return notesStore.allTags.find((item) => item.name === tag)?.color;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((ws) => ws.id === parsed.value)?.color;
    if (parsed.kind === 'profile') return 'var(--accent)';
    if (parsed.kind === 'note') return 'var(--text-2)';
    return undefined;
  }

  function chipLabel(tag: string): string {
    const parsed = parseBinding(tag);
    if (!parsed) return tag;
    if (parsed.kind === 'profile') return profilesStore.list.find((p) => p.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((ws) => ws.id === parsed.value)?.name ?? parsed.value;
    if (parsed.kind === 'note') return notesStore.list.find((n) => n.id === parsed.value)?.title ?? parsed.value;
    return parsed.value;
  }

  function unbind(card: Card, name?: string) {
    if (!confirm($t('ctx_unbind_confirm', { name: name ?? card.id }))) return;
    if (card.kind === 'password' && noteId) {
      void passwordStore.unlinkNote(card.id, noteId);
      return;
    }
    onunbind(card.binding);
  }

  function toggleCodes(binding: string) {
    codesOpen[binding] = !codesOpen[binding];
  }

  function openEntity(card: Card) {
    if (card.kind === 'workspace') {
      void goto(`/workspace/${card.id}`);
      return;
    }
    if (card.kind === 'profile') {
      const ws = profilesStore.list.find((p) => p.id === card.id)?.workspace_id;
      if (ws) void goto(`/workspace/${ws}`);
      return;
    }
    if (card.kind === 'proxy') {
      void goto('/proxies');
      return;
    }
    if (card.kind === 'ssh') {
      void goto('/terminal');
      return;
    }
    if (card.kind === 'password') {
      passwordStore.openId = card.id;
      return;
    }
    totpStore.pendingSearch = totpName(card.id);
  }

  async function run(card: Card, action: EntityAction) {
    if (action.id === 'copy-password' && notesLock.locked) {
      pendingCopy = card;
      unlockOpen = true;
      return;
    }
    const key = `${card.binding}/${action.id}`;
    busy = key;
    error = null;
    try {
      await action.run(card.id);
      if (action.id.startsWith('copy')) markCopied(key);
    } catch (e) {
      if (action.id === 'copy-password' && (hasErrorCode(e, 'vault_locked') || hasErrorCode(e, 'vault_mismatch') || hasErrorCode(e, 'decrypt_failed'))) {
        pendingCopy = card;
        unlockOpen = true;
      } else {
        error = formatError(e);
      }
    } finally {
      busy = null;
    }
  }
</script>

{#if cards.length > 0}
  <div class="context">
    {#each cards as card (card.binding)}
      {@const def = ENTITY_DEFS[card.kind]}
      {@const entity = entitySummary(card.kind, card.id)}
      <div class="card" class:focus={focus === card.binding} class:mention={!card.bound} style:--mark={entity?.color ?? def.color}>
        <div class="head">
          <span class="mark"><Icon name={def.icon} size={13} /></span>
          <span class="name" title={entity?.name ?? card.id}>{entity?.name ?? $t('ctx_missing')}</span>
          {#if entity?.status}
            <span class="dot {entity.status}" title={$t(STATUS_LABEL[entity.status])}></span>
          {/if}
          {#if !readonly && (card.kind === 'password' ? !!noteId : card.bound && !card.fromTag)}
            <button class="x" onclick={() => unbind(card, entity?.name)} title={$t('ctx_unbind')}>×</button>
          {/if}
        </div>
        <div class="sub">
          <span class="kind mark">{$t(def.label)}</span>
          {#if entity?.subtitle}<span class="sep">·</span><span class="subtitle">{entity.subtitle}</span>{/if}
        </div>
        {#if entity}
          {@const entry = card.kind === 'password' ? passwordStore.list.find((e) => e.id === card.id) : undefined}
          {@const totpIds = entry?.totp_ids ?? []}
          {@const shownTotp = codesOpen[card.binding] ? totpIds : totpIds.slice(0, 1)}
          <div class="actions">
            {#if card.kind === 'totp' || card.kind === 'password'}
              {@const tagged = (card.kind === 'totp' ? totpStore : passwordStore).list.find((e) => e.id === card.id)}
              {#if tagged && tagged.tags.length}
                <span class="pw-tags">
                  {#each tagged.tags as tag (tag)}
                    {@const color = chipColor(tag)}
                    <span class="pw-tag" style:color style:border-color={color} style:background={color ? `color-mix(in srgb, ${color} 16%, transparent)` : undefined}>{chipLabel(tag)}</span>
                  {/each}
                </span>
              {/if}
            {/if}
            {#if card.kind === 'totp'}
              <TotpLiveCode entryId={card.id} period={totpPeriod(card.id)} copiedLabel={$t('totp_copy')} />
            {/if}
            {#if card.kind === 'password'}
              {#each shownTotp as tid (tid)}
                <div class="pw-code">
                  <span class="pw-code-name">{totpName(tid)}</span>
                  <TotpLiveCode entryId={tid} period={totpPeriod(tid)} copiedLabel={$t('totp_copy')} compact />
                </div>
              {/each}
            {/if}
            {#if !card.bound && !card.fromTag && !readonly}
              <button class="btn btn-ghost btn-xs bind" onclick={() => onbind(card.binding)}>
                <Icon name="plus" size={11} />
                {$t('ctx_bind')}
              </button>
            {/if}
            <div class="foot">
              {#if card.kind === 'password' && totpIds.length > 1}
                <button type="button" class="more" onclick={() => toggleCodes(card.binding)}>
                  {codesOpen[card.binding] ? $t('ctx_totp_less') : $t('ctx_totp_more', { n: String(totpIds.length - 1) })}
                </button>
              {/if}
            <div class="pw-actions">
              <button type="button" class="icon-btn" title={$t('ctx_action_open')} onclick={() => openEntity(card)}>
                <Icon name="external-link" size={13} />
              </button>
              {#if card.kind === 'password'}
                {@const userKey = `${card.binding}/copy-username`}
                {@const passKey = `${card.binding}/copy-password`}
                {#if entry?.username}
                  <button
                    type="button"
                    class="icon-btn"
                    class:success={copied === userKey}
                    class:pop={copied === userKey}
                    title={$t('ctx_action_copy_username')}
                    disabled={busy !== null}
                    onclick={() => {
                      const action = def.actions.find((a) => a.id === 'copy-username');
                      if (action) void run(card, action);
                    }}
                  >
                    <Icon name={copied === userKey ? 'check' : 'user'} size={13} />
                  </button>
                {/if}
                <button
                  type="button"
                  class="icon-btn"
                  class:success={copied === passKey}
                  class:pop={copied === passKey}
                  title={notesLock.locked ? $t('notes_lock_title') : $t('ctx_action_copy_password')}
                  disabled={busy !== null}
                  onclick={() => {
                    const action = def.actions.find((a) => a.id === 'copy-password');
                    if (action) void run(card, action);
                  }}
                >
                  <Icon name={copied === passKey ? 'check' : notesLock.locked ? 'lock' : 'copy'} size={13} />
                </button>
              {:else}
                {#each def.actions.filter((a) => a.id !== 'open' && !(card.kind === 'totp' && a.id === 'copy-code')) as action (action.id)}
                  {@const key = `${card.binding}/${action.id}`}
                  <button
                    type="button"
                    class="icon-btn"
                    class:success={copied === key}
                    class:pop={copied === key}
                    title={$t(action.label)}
                    disabled={busy !== null}
                    onclick={() => run(card, action)}
                  >
                    <Icon name={copied === key ? 'check' : action.icon} size={13} />
                  </button>
                {/each}
              {/if}
            </div>
            </div>
          </div>
        {/if}
      </div>
    {/each}
    {#if error}
      <div class="error">{error}</div>
    {/if}
    <VaultUnlock
      bind:open={unlockOpen}
      onunlocked={() => {
        const card = pendingCopy;
        pendingCopy = null;
        const action = card ? ENTITY_DEFS[card.kind].actions.find((item) => item.id === 'copy-password') : undefined;
        if (card && action) void run(card, action);
      }}
    />
  </div>
{/if}

<style>
  .context {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-top: 1px solid var(--border);
    background: var(--bg-2);
  }
  .card {
    display: flex;
    flex-direction: column;
    height: 100%;
    gap: 0.25rem;
    padding: 0.45rem 0.6rem;
    border: 1px solid color-mix(in srgb, var(--mark, var(--border)) 55%, var(--border));
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--mark, transparent) 12%, var(--bg-2));
    min-width: 0;
  }
  .card.focus { border-color: var(--accent); }
  .card.mention { border-style: dashed; }
  .head { display: flex; align-items: center; gap: 0.35rem; min-width: 0; }
  .name { font-size: var(--fs-sm); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; color: var(--mark, var(--text)); }
  .mark { color: var(--mark, var(--text-2)); display: inline-flex; }
  .dot { width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0; }
  .dot.ok { background: var(--success); }
  .dot.bad { background: var(--danger-text); }
  .dot.unknown { background: var(--text-3); }
  .x {
    background: none; border: 0; padding: 0; cursor: pointer;
    color: var(--text-3); font-size: var(--fs-sm); line-height: 1;
  }
  .x:hover { color: var(--text); }
  .sub { display: flex; align-items: center; gap: 0.3rem; font-size: var(--fs-2xs); color: var(--text-3); min-width: 0; }
  .kind { text-transform: uppercase; letter-spacing: 0.06em; }
  .subtitle { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; flex-direction: column; align-items: flex-start; gap: 0.25rem; margin-top: 0.15rem; flex: 1; }
  .foot {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    margin-top: auto;
  }
  .btn-xs { font-size: var(--fs-2xs); padding: 0.15rem 0.45rem; gap: 0.25rem; }
  .bind { color: var(--accent); }
  .spinning { opacity: 0.6; }
  .error { grid-column: 1 / -1; font-size: var(--fs-xs); color: var(--danger-text); }
  .pw-tags {
    display: flex;
    flex-wrap: nowrap;
    gap: 4px;
    width: 100%;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .pw-tags::-webkit-scrollbar { display: none; }
  .pw-tag {
    flex-shrink: 0;
    border: 1px solid var(--border);
    border-radius: 99px;
    padding: 0 6px;
    font-size: 0.68rem;
    color: var(--text-2);
    white-space: nowrap;
  }
  .pw-actions { display: flex; gap: 0.25rem; margin-left: auto; }
  .icon-btn.pop { animation: icon-pop 0.35s ease; }
  @keyframes icon-pop {
    0% { transform: scale(1); }
    40% { transform: scale(1.12); }
    100% { transform: scale(1); }
  }
  .pw-code {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    min-width: 0;
  }
  .pw-code-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-2xs);
    color: var(--text-2);
  }
  .pw-code :global(.live.compact) { min-height: 0; }
  .more {
    background: none;
    border: 0;
    padding: 0;
    color: var(--text-3);
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .more:hover { color: var(--text); }
</style>
