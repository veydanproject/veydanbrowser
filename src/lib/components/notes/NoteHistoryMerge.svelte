<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { MergeResult } from '$lib/types';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    /** Fetches the merge; history merge and sync conflicts share this UI */
    load: () => Promise<MergeResult>;
    /** Name of the "theirs" side: history version or other device */
    theirsLabel?: string;
    theirsShort?: string;
    onresolved: (content: string) => void;
    oncancel: () => void;
  }

  let { load, theirsLabel = 'Версия из истории', theirsShort = 'Из истории', onresolved, oncancel }: Props = $props();

  interface NormalBlock {
    kind: 'normal';
    text: string;
  }

  interface ConflictBlock {
    kind: 'conflict';
    current: string;
    history: string;
    // resolved choice: 'current' | 'history' | 'both' | null
    choice: 'current' | 'history' | 'both' | null;
  }

  type Block = NormalBlock | ConflictBlock;

  let blocks = $state<Block[]>([]);
  let loading = $state(true);
  let error = $state('');
  let hasConflicts = $state(false);

  $effect(() => {
    loadMerge();
  });

  async function loadMerge() {
    loading = true;
    error = '';
    try {
      const result = await load();
      hasConflicts = result.has_conflicts;
      blocks = result.blocks.map((b) =>
        b.kind === 'normal'
          ? { kind: 'normal', text: b.text }
          : { kind: 'conflict', current: b.ours, history: b.theirs, choice: null },
      );
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function setChoice(index: number, choice: 'current' | 'history' | 'both') {
    const block = blocks[index];
    if (block.kind !== 'conflict') return;
    blocks[index] = { ...block, choice };
  }

  const allResolved = $derived(
    !hasConflicts ||
    blocks.every(b => b.kind !== 'conflict' || b.choice !== null)
  );

  const unresolvedCount = $derived(
    blocks.filter(b => b.kind === 'conflict' && b.choice === null).length
  );

  /** Block texts keep their trailing newlines, so plain concatenation rebuilds the file. */
  function buildResult(): string {
    return blocks
      .map(b => {
        if (b.kind === 'normal') return b.text;
        switch (b.choice) {
          case 'current': return b.current;
          case 'history': return b.history;
          case 'both': return b.current + b.history;
          default: return b.current;
        }
      })
      .join('');
  }

  function apply() {
    onresolved(buildResult());
  }
</script>

<div class="merge-overlay">
  <div class="merge-panel">
    <div class="merge-header">
      <span class="merge-title">
        <Icon name="git-merge" size={14} />
        Слияние версий
      </span>
      <button class="btn-cancel-header" onclick={oncancel}>
        <Icon name="x" size={14} />
      </button>
    </div>

    {#if loading}
      <div class="loading loading-center">
        <Icon name="loader" size={18} />
        <span>Выполняю слияние…</span>
      </div>
    {:else if error}
      <div class="error-bar">{error}</div>
      <div class="footer">
        <button class="btn btn-ghost btn-sm" onclick={oncancel}>Отмена</button>
      </div>
    {:else if !hasConflicts}
      <div class="no-conflicts">
        <Icon name="check-circle" size={20} />
        <p>Конфликтов нет — слияние прошло успешно</p>
        <span>Изменения будут применены к текущей заметке</span>
      </div>
      <div class="footer">
        <button class="btn btn-ghost btn-sm" onclick={oncancel}>Отмена</button>
        <button class="btn btn-primary btn-sm" onclick={apply}>Применить</button>
      </div>
    {:else}
      <div class="conflicts-summary">
        {#if unresolvedCount > 0}
          <span class="badge badge-warn">
            <Icon name="alert-triangle" size={12} />
            {unresolvedCount} конфликт{unresolvedCount > 1 ? 'а' : ''} не разрешено
          </span>
        {:else}
          <span class="badge badge-ok">
            <Icon name="check-circle" size={12} />
            Все конфликты разрешены
          </span>
        {/if}
        <span class="legend">
          <span class="l-current">Текущая</span>
          <span class="l-history">{theirsShort}</span>
        </span>
      </div>

      <div class="blocks-scroll">
        {#each blocks as block, i}
          {#if block.kind === 'normal'}
            <pre class="normal-block">{block.text}</pre>
          {:else}
            <div class="conflict-block" class:resolved={block.choice !== null}>
              <div class="conflict-side side-current" class:chosen={block.choice === 'current' || block.choice === 'both'}>
                <div class="side-label">
                  <Icon name="arrow-up" size={10} />
                  Текущая версия
                </div>
                <pre class="side-content">{block.current || '(пусто)'}</pre>
              </div>
              <div class="conflict-actions">
                <button
                  class="choice-btn"
                  class:active={block.choice === 'current'}
                  onclick={() => setChoice(i, 'current')}
                >
                  Принять текущую
                </button>
                <button
                  class="choice-btn choice-both"
                  class:active={block.choice === 'both'}
                  onclick={() => setChoice(i, 'both')}
                >
                  Обе версии
                </button>
                <button
                  class="choice-btn"
                  class:active={block.choice === 'history'}
                  onclick={() => setChoice(i, 'history')}
                >
                  Принять: {theirsShort}
                </button>
              </div>
              <div class="conflict-side side-history" class:chosen={block.choice === 'history' || block.choice === 'both'}>
                <div class="side-label">
                  <Icon name="clock" size={10} />
                  {theirsLabel}
                </div>
                <pre class="side-content">{block.history || '(пусто)'}</pre>
              </div>
            </div>
          {/if}
        {/each}
      </div>

      <div class="footer">
        <button class="btn btn-ghost btn-sm" onclick={oncancel}>Отмена</button>
        <button class="btn btn-primary btn-sm" onclick={apply} disabled={!allResolved}>
          Применить
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .merge-overlay {
    position: absolute;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: stretch;
    justify-content: flex-end;
    z-index: var(--z-panel);
  }

  .merge-panel {
    width: 100%;
    max-width: 640px;
    background: var(--surface);
    border-left: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    font-size: var(--fs-sm);
  }

  .merge-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.6rem var(--sp-4);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    background: var(--surface-2);
  }

  .merge-title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-weight: 600;
    font-size: var(--fs-base);
    color: var(--text);
  }

  .btn-cancel-header {
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-3);
    padding: 0.2rem;
    border-radius: var(--radius-sm);
    display: flex;
    align-items: center;
    transition: color 0.15s;
  }

  .btn-cancel-header:hover { color: var(--text); }

  /* .loading covers the layout; keep only the flex-fill delta for the overlay */
  .loading-center { flex: 1; }

  .error-bar {
    background: var(--danger-bg);
    color: var(--danger-text);
    padding: var(--sp-2) var(--sp-4);
    font-size: var(--fs-sm);
  }

  .no-conflicts {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    color: var(--success-text);
    text-align: center;
    padding: var(--sp-8);
  }

  .no-conflicts p { margin: 0; font-size: var(--fs-md); font-weight: var(--fw-medium); }
  .no-conflicts span { font-size: var(--fs-sm); color: var(--text-2); }

  .conflicts-summary {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem var(--sp-4);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: var(--sp-2);
  }

  .legend {
    display: flex;
    gap: var(--sp-3);
    font-size: var(--fs-2xs);
  }

  .l-current { color: var(--accent-text); }
  .l-history { color: var(--success-text); }

  .blocks-scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-2);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }

  .normal-block {
    margin: 0;
    padding: 0.4rem var(--sp-3);
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    color: var(--text-3);
    white-space: pre-wrap;
    word-break: break-all;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
  }

  .conflict-block {
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .conflict-block.resolved { border-color: var(--accent-border); }

  .conflict-side {
    padding: 0.4rem var(--sp-3);
    opacity: 0.5;
    transition: opacity 0.15s;
  }

  .conflict-side.chosen { opacity: 1; }
  .side-current { background: var(--accent-tint); border-bottom: 1px solid var(--border); }
  .side-history { background: var(--success-bg); border-top: 1px solid var(--border); }

  .side-label {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: var(--fs-2xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 0.3rem;
  }

  .side-current .side-label { color: var(--accent-text); }
  .side-history .side-label { color: var(--success-text); }

  .side-content {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-sm);
    white-space: pre-wrap;
    word-break: break-all;
    color: var(--text);
  }

  .conflict-actions {
    display: flex;
    gap: 0;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }

  .choice-btn {
    flex: 1;
    background: none;
    border: none;
    border-right: 1px solid var(--border);
    padding: 0.3rem var(--sp-2);
    font-size: var(--fs-2xs);
    cursor: pointer;
    color: var(--text-2);
    transition: background 0.12s, color 0.12s;
    text-align: center;
  }

  .choice-btn:last-child { border-right: none; }
  .choice-btn:hover { background: var(--surface-hover); color: var(--text); }
  .choice-btn.active { background: var(--accent-bg); color: var(--accent-text); font-weight: var(--fw-medium); }
  .choice-both.active { background: var(--success-bg); color: var(--success-text); }

  .footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
    padding: 0.6rem var(--sp-4);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
    background: var(--surface-drawer-footer);
  }

</style>
