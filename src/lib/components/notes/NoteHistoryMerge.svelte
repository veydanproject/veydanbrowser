<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { api } from '$lib/api';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    noteId: string;
    historyId: string;
    onresolved: (content: string) => void;
    oncancel: () => void;
  }

  let { noteId, historyId, onresolved, oncancel }: Props = $props();

  type BlockKind = 'normal' | 'conflict';

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
      const result = await api.notes.historyMerge(noteId, historyId);
      hasConflicts = result.has_conflicts;
      blocks = parseBlocks(result.content);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function parseBlocks(content: string): Block[] {
    if (!content.includes('<<<<<<<')) {
      return [{ kind: 'normal', text: content }];
    }

    const result: Block[] = [];
    const lines = content.split('\n');
    let i = 0;

    while (i < lines.length) {
      if (lines[i].startsWith('<<<<<<<')) {
        // Start of conflict block
        let currentLines: string[] = [];
        let historyLines: string[] = [];
        let inHistory = false;
        i++;
        while (i < lines.length && !lines[i].startsWith('>>>>>>>')) {
          if (lines[i].startsWith('=======')) {
            inHistory = true;
          } else if (inHistory) {
            historyLines.push(lines[i]);
          } else {
            currentLines.push(lines[i]);
          }
          i++;
        }
        i++; // skip >>>>>>>
        result.push({
          kind: 'conflict',
          current: currentLines.join('\n'),
          history: historyLines.join('\n'),
          choice: null,
        });
      } else {
        // Normal text — accumulate until next conflict marker
        const normalLines: string[] = [];
        while (i < lines.length && !lines[i].startsWith('<<<<<<<')) {
          normalLines.push(lines[i]);
          i++;
        }
        const text = normalLines.join('\n');
        if (text) {
          result.push({ kind: 'normal', text });
        }
      }
    }

    return result;
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

  function buildResult(): string {
    return blocks
      .map(b => {
        if (b.kind === 'normal') return b.text;
        switch (b.choice) {
          case 'current': return b.current;
          case 'history': return b.history;
          case 'both': return `${b.current}\n${b.history}`;
          default: return `<<<<<<< Current\n${b.current}\n=======\n${b.history}\n>>>>>>> History`;
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
          <span class="l-history">Из истории</span>
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
                  Принять историю
                </button>
              </div>
              <div class="conflict-side side-history" class:chosen={block.choice === 'history' || block.choice === 'both'}>
                <div class="side-label">
                  <Icon name="clock" size={10} />
                  Версия из истории
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
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: stretch;
    justify-content: flex-end;
    z-index: 20;
  }

  .merge-panel {
    width: 100%;
    max-width: 640px;
    background: var(--bg-2);
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
    background: var(--bg-1);
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
    background: rgba(220, 38, 38, 0.1);
    color: var(--danger-text, #dc2626);
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
    color: var(--success-text, #16a34a);
    text-align: center;
    padding: var(--sp-8);
  }

  .no-conflicts p { margin: 0; font-size: var(--fs-md); font-weight: 500; }
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

  .l-current { color: var(--accent); }
  .l-history { color: var(--success-text, #16a34a); }

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
    font-family: 'Menlo', 'Consolas', monospace;
    font-size: var(--fs-sm);
    color: var(--text-3);
    white-space: pre-wrap;
    word-break: break-all;
    border-radius: var(--radius-sm);
    background: var(--surface);
  }

  .conflict-block {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .conflict-block.resolved { border-color: var(--accent); }

  .conflict-side {
    padding: 0.4rem var(--sp-3);
    opacity: 0.5;
    transition: opacity 0.15s;
  }

  .conflict-side.chosen { opacity: 1; }
  .side-current { background: rgba(99, 102, 241, 0.05); border-bottom: 1px solid var(--border); }
  .side-history { background: rgba(16, 185, 129, 0.05); border-top: 1px solid var(--border); }

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

  .side-current .side-label { color: var(--accent); }
  .side-history .side-label { color: var(--success-text, #16a34a); }

  .side-content {
    margin: 0;
    font-family: 'Menlo', 'Consolas', monospace;
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
    background: var(--bg-1);
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
  .choice-btn:hover { background: var(--surface); color: var(--text); }
  .choice-btn.active { background: var(--accent-bg, rgba(99,102,241,0.1)); color: var(--accent); font-weight: 500; }
  .choice-both.active { background: rgba(16, 185, 129, 0.08); color: var(--success-text, #16a34a); }

  .footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
    padding: 0.6rem var(--sp-4);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
    background: var(--bg-1);
  }

</style>
