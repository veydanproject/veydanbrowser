<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { api, formatError, onSyncProgress, onSyncStatus, syncProgressText, type NoteSyncInfo, type SyncProgress, type SyncStatus } from '$lib/mobile/api';
  import { locale, t } from '$lib/mobile/i18n';
  import { fmtDateTime } from '$lib/mobile/notes-editor';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    noteId: string;
    pinned: boolean;
    archived: boolean;
    onclose: () => void;
    onhistory: () => void;
    onpin: () => void;
    onarchive: () => void;
    onmove: () => void;
    ondelete: () => void;
    /** Called after a manual sync finished so the note can reload. */
    onsynced: () => void;
  }

  let { open, noteId, pinned, archived, onclose, onhistory, onpin, onarchive, onmove, ondelete, onsynced }: Props = $props();

  const isNew = $derived(noteId === 'new');

  let status = $state<SyncStatus | null>(null);
  let info = $state<NoteSyncInfo | null>(null);
  let progress = $state<SyncProgress | null>(null);
  let syncing = $state(false);
  let error = $state('');

  async function refresh() {
    try {
      status = await api.sync.status();
      info = isNew ? null : await api.notes.syncInfo(noteId);
    } catch (e) {
      error = formatError(e);
    }
  }

  $effect(() => {
    if (open) void refresh();
  });

  onMount(() => {
    const unStatus = onSyncStatus(() => {
      if (open) void refresh();
    });
    const unProgress = onSyncProgress((p) => (progress = p));
    return () => {
      unStatus.then((f) => f());
      unProgress.then((f) => f());
    };
  });

  $effect(() => {
    if (status && !status.running) progress = null;
  });

  const syncLine = $derived.by(() => {
    if (!status?.enabled || !status.joined) return $t('notes_sync_off');
    if (status.running || syncing) return progress ? syncProgressText($t, progress) : $t('notes_sync_running');
    if (status.last_error) return `${$t('notes_sync_error')}: ${status.last_error}`;
    const last = status.last_run ? fmtDateTime(status.last_run, $locale) : $t('notes_sync_never');
    const note = !info ? '' : !info.tracked ? $t('notes_sync_untracked') : info.pending ? $t('notes_sync_pending') : $t('notes_sync_uptodate');
    return [note, `${$t('notes_sync_last')}: ${last}`].filter(Boolean).join(' · ');
  });

  const canSync = $derived(!!status?.enabled && !!status?.joined && !status?.running && !syncing);

  async function syncNow() {
    syncing = true;
    error = '';
    try {
      status = await api.sync.runNow();
      await refresh();
      onsynced();
    } catch (e) {
      error = formatError(e);
    } finally {
      syncing = false;
    }
  }
</script>

<BottomSheet {open} title={$t('notes_actions')} {onclose}>
  <div class="m-list">
    <button type="button" class="m-row" disabled={isNew} onclick={onpin}>
      <Icon name="pin" size={20} /><span class="m-row-label">{pinned ? $t('notes_unpin') : $t('notes_pin')}</span>
    </button>
    <button type="button" class="m-row" disabled={isNew} onclick={onmove}>
      <Icon name="folder" size={20} /><span class="m-row-label">{$t('notes_move')}</span>
    </button>
    <button type="button" class="m-row" disabled={isNew} onclick={onarchive}>
      <Icon name={archived ? 'archive-restore' : 'archive'} size={20} /><span class="m-row-label">{archived ? $t('notes_unarchive') : $t('notes_archive')}</span>
    </button>
    <button type="button" class="m-row" disabled={isNew} onclick={onhistory}>
      <Icon name="clock" size={20} /><span class="m-row-label">{$t('notes_history')}</span>
    </button>
  </div>

  <div class="m-list">
    <div class="m-row sync">
      <span class:spin={status?.running || syncing}><Icon name="refresh-cw" size={20} /></span>
      <span class="m-row-label">
        <span>{$t('notes_sync')}</span>
        <span class="sub" class:warn={!!status?.last_error || !!error}>{error || syncLine}</span>
      </span>
      <button type="button" class="btn btn-ghost btn-sm" disabled={!canSync} onclick={syncNow}>{$t('notes_sync_now')}</button>
    </div>
  </div>

  <div class="m-list">
    <button type="button" class="m-row" onclick={ondelete}>
      <Icon name="trash-2" size={20} /><span class="m-row-label danger">{$t('notes_delete')}</span>
    </button>
  </div>
</BottomSheet>

<style>
  .m-row:disabled { opacity: 0.45; }
  .danger { color: var(--danger-text); }
  .m-row.sync { min-height: 60px; }
  .m-row.sync .m-row-label { display: flex; flex-direction: column; gap: 2px; min-width: 0; white-space: normal; }
  .sub { font-size: var(--fs-xs); color: var(--text-3); overflow-wrap: anywhere; }
  .sub.warn { color: var(--danger-text); }
  .spin { display: inline-flex; animation: spin 1s linear infinite; }
  .btn-sm { min-height: 36px; }
</style>
