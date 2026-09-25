<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  Veydan objects bound to or mentioned in a note. Tap a row to see the other
  notes about that object; long-press for bind / unbind.
-->
<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { goto } from '$app/navigation';
  import { api, formatError, type BindingSummary, type NoteListItem } from '$lib/mobile/api';
  import { t, type MobileKey } from '$lib/mobile/i18n';
  import { longpress } from '$lib/mobile/longpress';
  import { portal } from '$lib/portal';
  import { hasErrorCode } from '$lib/utils';
  import type { Attachment } from 'svelte/attachments';
  import { parseBinding, type EntityKind } from '$lib/bindings';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import BottomSheet from './BottomSheet.svelte';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';
  import VaultUnlock from '$lib/components/passwords/VaultUnlock.svelte';

  interface Props {
    open: boolean;
    /** Current note; excluded from "other notes" */
    noteId: string;
    /** Entity bindings stored on the note */
    bindings: string[];
    /** `[[kind:id]]` mentions found in the body */
    mentions: string[];
    /** Binding to highlight, e.g. after tapping a mention */
    focus?: string | null;
    busy?: boolean;
    onclose: () => void;
    onopen: (noteId: string) => void;
    onbind: (binding: string) => void;
    onunbind: (binding: string) => void;
  }

  let { open, noteId, bindings, mentions, focus = null, busy = false, onclose, onopen, onbind, onunbind }: Props = $props();

  const ICON: Record<EntityKind, string> = {
    workspace: 'layers',
    profile: 'globe',
    proxy: 'shield',
    ssh: 'terminal',
    totp: 'key',
    password: 'lock',
  };
  const KIND_LABEL: Record<EntityKind, MobileKey> = {
    workspace: 'notes_context_kind_workspace',
    profile: 'notes_context_kind_profile',
    proxy: 'notes_context_kind_proxy',
    ssh: 'notes_context_kind_ssh',
    totp: 'notes_context_kind_totp',
    password: 'notes_context_kind_password',
  };
  /** "Other notes of this workspace / profile / ..." in the right grammatical form */
  const RELATED_LABEL: Record<EntityKind, MobileKey> = {
    workspace: 'notes_context_related_workspace',
    profile: 'notes_context_related_profile',
    proxy: 'notes_context_related_proxy',
    ssh: 'notes_context_related_ssh',
    totp: 'notes_context_related_totp',
    password: 'notes_context_related_password',
  };

  interface Row {
    binding: string;
    kind: EntityKind;
    id: string;
    bound: boolean;
    /** Link stored on the password tags, not on the note. */
    fromTag: boolean;
    summary: BindingSummary | null;
  }

  let summaries = $state<Map<string, BindingSummary>>(new Map());
  let error = $state('');
  /** TOTP refresh windows, for the countdown ring */
  let totpPeriods = $state<Map<string, number>>(new Map());
  /** Row whose other notes are shown */
  let expanded = $state<string | null>(null);
  let related = $state<NoteListItem[]>([]);
  let loadingRelated = $state(false);
  /** Row the long-press menu is open for */
  let menuRow = $state<Row | null>(null);
  /** `id:user` or `id:pass` of the value just copied */
  let copied = $state('');
  let copyTimer: ReturnType<typeof setTimeout>;
  let unlockOpen = $state(false);
  let pendingCopy = $state<string | null>(null);

  const linked = $derived(
    passwordStore.list.filter((entry) => entry.tags.includes(`note:${noteId}`)).map((entry) => `password:${entry.id}`),
  );
  const all = $derived([...new Set([...bindings, ...mentions, ...linked])]);
  /** Names are also needed for TOTP codes hanging off password rows */
  const summaryKeys = $derived([
    ...new Set([
      ...all,
      ...all.flatMap((b) => {
        const p = parseBinding(b);
        return p?.kind === 'password' ? passwordTotps(p.value).map((tid) => `totp:${tid}`) : [];
      }),
    ]),
  ]);

  const rows = $derived.by((): Row[] =>
    all.flatMap((b) => {
      const p = parseBinding(b);
      if (!p || !p.value || !(p.kind in ICON)) return [];
      return [{
        binding: b,
        kind: p.kind as EntityKind,
        id: p.value,
        bound: bindings.includes(b),
        fromTag: linked.includes(b) && !bindings.includes(b),
        summary: summaries.get(b) ?? null,
      }];
    }),
  );

  $effect(() => {
    if (!open) { menuRow = null; return; }
    void passwordStore.ensureLoaded();
    const wanted = summaryKeys;
    error = '';
    expanded = null;
    api.notes
      .bindingSummaries(wanted)
      .then((list) => (summaries = new Map(list.map((s) => [s.binding, s]))))
      .catch((e) => (error = formatError(e)));
    if (wanted.some((b) => b.startsWith('totp:') || b.startsWith('password:'))) {
      api.totp
        .list()
        .then((list) => (totpPeriods = new Map(list.map((e) => [e.id, e.period]))))
        .catch(() => {});
    }
  });

  /** TOTP ids linked to a password row */
  function passwordTotps(id: string): string[] {
    return passwordStore.list.find((e) => e.id === id)?.totp_ids ?? [];
  }

  async function toggle(row: Row) {
    if (expanded === row.binding) { expanded = null; return; }
    expanded = row.binding;
    loadingRelated = true;
    try {
      related = (await api.notes.entityNotes(row.binding)).filter((n) => n.id !== noteId);
    } catch (e) {
      error = formatError(e);
    } finally {
      loadingRelated = false;
    }
  }

  /**
   * Stable attachment: an inline `longpress(() => ...)` would be recreated on every
   * re-render (e.g. the 1s TOTP tick), which cancels the 500ms timer before it fires.
   * The row is read back from `data-binding` instead.
   */
  const pressMenu: Attachment<HTMLElement> = (el) =>
    longpress(() => openMenu(rows.find((r) => r.binding === el.dataset.binding) ?? null))(el);

  /** The release after a long press lands as a click; ignore it if it hits the freshly opened menu. */
  let menuOpenedAt = 0;

  function openMenu(row: Row | null) {
    menuRow = row;
    menuOpenedAt = Date.now();
  }

  function menuAction(action: 'bind' | 'unbind' | 'open') {
    if (Date.now() - menuOpenedAt < 400) return;
    const row = menuRow;
    menuRow = null;
    if (!row) return;
    if (action === 'open') {
      onclose();
      void goto(`/passwords/${row.id}`);
      return;
    }
    if (row.kind === 'password' && action === 'unbind') {
      void passwordStore.unlinkNote(row.id, noteId);
      return;
    }
    if (action === 'bind') onbind(row.binding);
    else onunbind(row.binding);
  }

  function markCopied(key: string) {
    copied = key;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied = ''), 1600);
  }

  async function writeClipboard(value: string) {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      const el = document.createElement('textarea');
      el.value = value;
      el.setAttribute('readonly', '');
      el.style.position = 'fixed';
      el.style.left = '-9999px';
      document.body.appendChild(el);
      el.select();
      document.execCommand('copy');
      el.remove();
    }
  }

  async function copyUsername(id: string) {
    const entry = passwordStore.list.find((e) => e.id === id);
    if (!entry?.username) return;
    await writeClipboard(entry.username);
    markCopied(`${id}:user`);
  }

  async function copyPassword(id: string) {
    if (notesLock.locked) {
      pendingCopy = id;
      unlockOpen = true;
      return;
    }
    try {
      const value = (await api.passwords.reveal(id, 'password')).value;
      await writeClipboard(value);
      markCopied(`${id}:pass`);
      setTimeout(() => {
        navigator.clipboard.readText().then((current) => {
          if (current === value) void navigator.clipboard.writeText('');
        }).catch(() => {});
      }, 30_000);
    } catch (e) {
      if (hasErrorCode(e, 'vault_locked') || hasErrorCode(e, 'vault_mismatch') || hasErrorCode(e, 'decrypt_failed')) {
        pendingCopy = id;
        unlockOpen = true;
        return;
      }
      error = formatError(e);
    }
  }
</script>

<BottomSheet {open} title={$t('notes_context')} {onclose}>
  {#if error}<div class="m-error">{error}</div>{/if}
  {#if rows.length === 0}
    <p class="empty">{$t('notes_context_empty')}</p>
  {:else}
    <p class="hint">{$t('notes_context_hint')}</p>
    <div class="m-list">
      {#each rows as row (row.binding)}
        {@const isOpen = expanded === row.binding}
        <div class="entity" class:focus={focus === row.binding} class:mention={!row.bound}>
          <!-- Row is a div so the live TOTP chip (a button itself) can sit inline -->
          <div class="m-row entity-row" data-binding={row.binding} {@attach pressMenu}>
            <button type="button" class="main" onclick={() => toggle(row)}>
              <Icon name={ICON[row.kind]} size={18} />
              <div class="text">
                <span class="m-row-label">{row.summary?.name ?? $t('notes_context_missing')}</span>
                <span class="sub">
                  {$t(KIND_LABEL[row.kind])}{#if row.summary?.subtitle} · {row.summary.subtitle}{/if}{#if !row.bound} · {$t('notes_context_mentioned')}{/if}
                </span>
              </div>
            </button>
            {#if row.kind === 'totp' && row.summary}
              <TotpLiveCode entryId={row.id} period={totpPeriods.get(row.id) ?? 30} copiedLabel={$t('notes_context_copied')} compact />
            {/if}
            {#if row.kind === 'password' && row.summary}
              {#if passwordStore.list.find((e) => e.id === row.id)?.username}
                <button type="button" class="chev" class:ok={copied === `${row.id}:user`} onclick={() => copyUsername(row.id)} aria-label={$t('ctx_action_copy_username')}>
                  <Icon name={copied === `${row.id}:user` ? 'check' : 'user'} size={18} />
                </button>
              {/if}
              <button type="button" class="chev" class:ok={copied === `${row.id}:pass`} onclick={() => copyPassword(row.id)} aria-label={$t('ctx_action_copy_password')}>
                <Icon name={copied === `${row.id}:pass` ? 'check' : notesLock.locked ? 'lock' : 'copy'} size={18} />
              </button>
            {/if}
            <button type="button" class="chev" onclick={() => toggle(row)} aria-label={$t(RELATED_LABEL[row.kind])}>
              <Icon name={isOpen ? 'chevron-down' : 'chevron-right'} size={18} />
            </button>
            {#if !row.fromTag || row.kind === 'password'}
            <button type="button" class="chev" onclick={() => openMenu(row)} aria-label={$t('notes_context_more')}>
              <Icon name="more-vertical" size={18} />
            </button>
            {/if}
          </div>
          {#if row.kind === 'password' && row.summary}
            {@const codes = passwordTotps(row.id)}
            {#if codes.length}
              <div class="pw-codes">
                {#each codes as tid (tid)}
                  <div class="pw-code">
                    <Icon name="key" size={14} />
                    <span class="sub pw-code-name">{summaries.get(`totp:${tid}`)?.name ?? ''}</span>
                    <TotpLiveCode entryId={tid} period={totpPeriods.get(tid) ?? 30} copiedLabel={$t('notes_context_copied')} compact />
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
          {#if isOpen}
            <div class="related">
              <span class="sub related-title">{$t(RELATED_LABEL[row.kind])}</span>
              {#if loadingRelated}
                <span class="sub pad">{$t('loading')}</span>
              {:else if related.length === 0}
                <span class="sub pad">{$t('notes_context_related_empty')}</span>
              {:else}
                {#each related as n (n.id)}
                  <button type="button" class="m-row nested" onclick={() => onopen(n.id)}>
                    <Icon name="file-text" size={16} />
                    <span class="m-row-label">{n.title || $t('notes_untitled')}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</BottomSheet>

<!-- Long-press menu: bind / unbind. Portalled: a fixed sheet inside the transformed parent sheet would be clipped. -->
<div use:portal>
<BottomSheet open={menuRow !== null} title={menuRow?.summary?.name ?? $t('notes_context')} onclose={() => (menuRow = null)}>
  <div class="m-list">
    {#if menuRow?.kind === 'password'}
      <button type="button" class="m-row" onclick={() => menuAction('open')}>
        <Icon name="external-link" size={20} /><span class="m-row-label">{$t('ctx_action_open')}</span>
      </button>
    {/if}
    {#if menuRow && (menuRow.bound || (menuRow.fromTag && menuRow.kind === 'password'))}
      <button type="button" class="m-row" disabled={busy} onclick={() => menuAction('unbind')}>
        <Icon name="x" size={20} /><span class="m-row-label danger-text">{$t('notes_context_unbind')}</span>
      </button>
      <p class="hint">{$t('notes_context_unbind_hint')}</p>
    {:else}
      <button type="button" class="m-row" disabled={busy} onclick={() => menuAction('bind')}>
        <Icon name="plus" size={20} /><span class="m-row-label">{$t('notes_context_bind')}</span>
      </button>
      <p class="hint">{$t('notes_context_bind_hint')}</p>
    {/if}
  </div>
</BottomSheet>
<VaultUnlock bind:open={unlockOpen} onunlocked={() => { const id = pendingCopy; pendingCopy = null; if (id) void copyPassword(id); }} />
</div>

<style>
  .empty { margin: var(--sp-4); color: var(--text-3); text-align: center; }
  .hint { margin: 0 var(--sp-2) var(--sp-3); color: var(--text-3); font-size: var(--fs-xs); }
  .entity { display: flex; flex-direction: column; }
  .entity + .entity { border-top: 1px solid var(--border); }
  .entity.focus { background: var(--accent-bg); border-radius: 12px; }
  .entity.mention .m-row-label { color: var(--text-2); }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .sub { font-size: var(--fs-xs); color: var(--text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entity-row { padding-right: 0; gap: var(--sp-2); }
  .main {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: var(--sp-3);
    min-height: 52px; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; text-align: left;
  }
  .chev {
    display: inline-flex; align-items: center; justify-content: center;
    width: 40px; height: 44px; border: 0; background: transparent; color: var(--text-3);
  }
  .chev.ok { color: var(--success-text); }
  .pw-codes { display: flex; flex-direction: column; padding: 0 var(--sp-2) var(--sp-1) var(--sp-5); color: var(--text-3); }
  .pw-code { display: flex; align-items: center; gap: var(--sp-2); min-height: 36px; }
  .pw-code-name { flex: 1; min-width: 0; }
  .related { display: flex; flex-direction: column; padding: 0 0 var(--sp-2) var(--sp-5); }
  .related-title { padding: 0 var(--sp-2) var(--sp-1); }
  .pad { padding: 0 var(--sp-2); }
  .m-row.nested { min-height: 44px; }
</style>
