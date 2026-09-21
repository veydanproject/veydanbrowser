<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Sync conflict resolver: quick keep/take choice, or a per-block review.
     Works on one conflictGet snapshot; a changed token reloads it. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { api, formatError, onSyncStatus, type ConflictView, type MergeBlock } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { hasErrorCode } from '$lib/utils';
  import {
    buildMergeContent,
    emptySelections,
    isComplete,
    selectAll,
    unresolvedCount,
    type MergeChoice,
    type MergeSelections,
    type MergeSide,
  } from '$lib/notes/merge-result';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    noteId: string;
    onclose: () => void;
    /** Reloads note and sync status; the sheet closes only after it succeeds. */
    onresolved: () => Promise<void>;
  }

  let { open, noteId, onclose, onresolved }: Props = $props();

  let view = $state<ConflictView | null>(null);
  let blocks = $state<MergeBlock[]>([]);
  let choices = $state<MergeSelections>([]);
  let step = $state<'pick' | 'review'>('pick');
  let loading = $state(false);
  let busy = $state(false);
  let error = $state('');
  let notice = $state('');

  const left = $derived(unresolvedCount(blocks, choices));
  const complete = $derived(isComplete(blocks, choices));

  $effect(() => {
    if (!open) return;
    view = null;
    step = 'pick';
    notice = '';
    void load();
  });

  onMount(() => {
    const un = onSyncStatus(() => {
      if (open && !busy) void load();
    });
    return () => un.then((f) => f());
  });

  /** Fetches the snapshot; a new token resets the choices and tells the user. */
  async function load() {
    loading = true;
    error = '';
    try {
      const next = await api.sync.conflictGet(noteId);
      if (view?.token === next.token) return;
      if (view) notice = $t('note_sync_conflict_changed');
      view = next;
      blocks = next.merge.blocks;
      choices = emptySelections(blocks);
    } catch (e) {
      // Resolved elsewhere: nothing left to choose.
      if (view && hasErrorCode(e, 'not_found')) return finish();
      error = formatError(e);
    } finally {
      loading = false;
    }
  }

  async function finish() {
    await onresolved();
    onclose();
  }

  async function resolve(content: string) {
    if (!view) return;
    busy = true;
    error = '';
    try {
      await api.sync.conflictResolve(noteId, view.token, content);
      await finish();
    } catch (e) {
      if (hasErrorCode(e, 'conflict_changed')) await load();
      else error = formatError(e);
    } finally {
      busy = false;
    }
  }

  const takeSide = (side: MergeSide) => resolve(buildMergeContent(blocks, selectAll(blocks, side)));
  const applyReview = () => resolve(buildMergeContent(blocks, choices));

  function choose(i: number, choice: MergeChoice) {
    choices[i] = choice;
  }

  function chooseAll(side: MergeSide) {
    choices = selectAll(blocks, side);
  }
</script>

<BottomSheet
  {open}
  title={step === 'review' ? $t('note_sync_conflict_review') : $t('note_sync_conflict_title')}
  {onclose}
  onback={step === 'review' ? () => (step = 'pick') : undefined}
>
  {#if error}<div class="m-error">{error}</div>{/if}
  {#if notice}<div class="notice"><Icon name="alert-triangle" size={14} /> {notice}</div>{/if}

  {#if loading && !view}
    <p class="empty"><Icon name="loader" size={16} /></p>
  {:else if !view}
    <p class="empty">{$t('common_nothing_found')}</p>
  {:else if step === 'pick'}
    <p class="m-hint">{$t('note_sync_conflict')}</p>
    <div class="m-list">
      <button type="button" class="m-row" disabled={busy} onclick={() => takeSide('ours')}>
        <Icon name="check" size={18} />
        <span class="m-row-label">{$t('note_sync_conflict_keep')}</span>
      </button>
      <button type="button" class="m-row" disabled={busy} onclick={() => takeSide('theirs')}>
        <Icon name="download" size={18} />
        <span class="m-row-label">{$t('note_sync_conflict_take')}</span>
      </button>
      <button type="button" class="m-row" disabled={busy} onclick={() => (step = 'review')}>
        <Icon name="git-merge" size={18} />
        <span class="m-row-label">{$t('note_sync_conflict_review')}</span>
        <span class="chev"><Icon name="chevron-right" size={16} /></span>
      </button>
    </div>
  {:else}
    <div class="bulk">
      <button type="button" class="m-chip" disabled={busy} onclick={() => chooseAll('ours')}>{$t('note_sync_conflict_all_mine')}</button>
      <button type="button" class="m-chip" disabled={busy} onclick={() => chooseAll('theirs')}>{$t('note_sync_conflict_all_theirs')}</button>
      {#if left > 0}
        <span class="left">{$t('note_sync_conflict_unresolved', { n: String(left) })}</span>
      {/if}
    </div>

    <div class="blocks">
      {#each blocks as block, i (i)}
        {#if block.kind === 'normal'}
          <pre class="normal">{block.text}</pre>
        {:else}
          {@const choice = choices[i]}
          <div class="conflict" class:done={choice !== null}>
            <div class="side ours" class:on={choice === 'ours' || choice === 'both'}>
              <span class="label">{$t('note_sync_conflict_mine')}</span>
              <pre>{block.ours || ' '}</pre>
            </div>
            <div class="pick">
              <button type="button" class:active={choice === 'ours'} onclick={() => choose(i, 'ours')}>{$t('note_sync_conflict_mine')}</button>
              <button type="button" class:active={choice === 'both'} onclick={() => choose(i, 'both')}>{$t('note_sync_conflict_both')}</button>
              <button type="button" class:active={choice === 'theirs'} onclick={() => choose(i, 'theirs')}>{$t('note_sync_conflict_remote_short')}</button>
            </div>
            <div class="side theirs" class:on={choice === 'theirs' || choice === 'both'}>
              <span class="label">{$t('note_sync_conflict_remote')}</span>
              <pre>{block.theirs || ' '}</pre>
            </div>
          </div>
        {/if}
      {/each}
    </div>

    <button type="button" class="btn btn-primary" disabled={busy || !complete} onclick={applyReview}>
      <Icon name="check" size={16} /> {$t('note_sync_conflict_apply')}
    </button>
  {/if}
</BottomSheet>

<style>
  .empty { color: var(--text-3); text-align: center; padding: var(--sp-6) 0; }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    color: var(--warn-text, var(--text-2));
    font-size: var(--fs-sm);
  }
  .m-row:disabled { opacity: 0.5; }
  .bulk { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
  .left { margin-left: auto; font-size: var(--fs-xs); color: var(--text-3); }
  .blocks {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .normal {
    padding: var(--sp-2) var(--sp-3);
    color: var(--text-3);
    background: var(--surface-2);
    border-radius: var(--radius-sm, 6px);
    max-height: 7.5em;
    overflow: hidden;
  }
  .conflict {
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 8px);
    overflow: hidden;
  }
  .conflict.done { border-color: var(--accent-border, var(--accent)); }
  .side { padding: var(--sp-2) var(--sp-3); opacity: 0.55; }
  .side.on { opacity: 1; }
  .side.ours { background: var(--accent-tint, var(--surface-2)); }
  .side.theirs { background: var(--success-bg, var(--surface-2)); }
  .label {
    display: block;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 4px;
    color: var(--text-2);
  }
  .pick {
    display: flex;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .pick button {
    flex: 1;
    min-height: 40px;
    background: none;
    border: 0;
    border-right: 1px solid var(--border);
    font: inherit;
    font-size: 13px;
    color: var(--text-2);
  }
  .pick button:last-child { border-right: 0; }
  .pick button.active { background: var(--accent-bg, var(--surface-2)); color: var(--accent-text, var(--text)); font-weight: 600; }
  .btn { display: inline-flex; align-items: center; justify-content: center; gap: 8px; }
</style>
