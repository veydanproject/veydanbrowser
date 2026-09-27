<!--
  SPDX-FileCopyrightText: 2026 Veydan Project
  SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1
-->
<!-- Legacy Veydan Browser data migration. Remove in 4.0. -->
<script lang="ts">
  import '@fontsource-variable/manrope/index.css';
  import '$lib/styles/tokens.css';
  import '$lib/styles/base.css';
  import { onMount } from 'svelte';
  import { api, type MigrationPhase, type MigrationProgress } from '$lib/api';
  import { t } from '$lib/i18n';
  import { theme } from '$lib/theme';
  import { formatBytes } from '$lib/utils';
  import Icon from '$lib/Icon.svelte';

  let { initial, onfinished }: { initial: MigrationPhase; onfinished: () => void } = $props();

  // Initial snapshot only; later phases arrive via migration://state.
  // svelte-ignore state_referenced_locally
  let phase = $state<MigrationPhase>(initial);
  let progress = $state<MigrationProgress>({
    step: 'backup',
    percent: 0,
    done_bytes: 0,
    total_bytes: 0,
  });
  let startError = $state('');

  const stepLabel = $derived(
    ({
      backup: $t('migration_step_backup'),
      verify_backup: $t('migration_step_verify_backup'),
      move: $t('migration_step_move'),
      verify: $t('migration_step_verify'),
      starting: $t('migration_step_starting'),
    })[progress.step]
  );

  onMount(() => {
    // Tokens are scoped to body[data-theme]; the shell is not mounted yet.
    const unsubTheme = theme.subscribe((val) => {
      document.body.dataset.theme = val;
    });
    const unlisteners: Array<() => void> = [unsubTheme];
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      unlisteners.push(
        await listen<MigrationProgress>('migration://progress', (e) => {
          progress = e.payload;
        })
      );
      unlisteners.push(
        await listen<MigrationPhase>('migration://state', (e) => {
          phase = e.payload;
        })
      );
    })();
    return () => unlisteners.forEach((u) => u());
  });

  async function start() {
    startError = '';
    try {
      await api.migration.start();
    } catch (e) {
      startError = String(e);
    }
  }
</script>

<div class="migration">
  <div class="card">
    <div class="brand">
      <img src="/logo.png" alt="" class="logo" />
      <h1>{$t('migration_title')}</h1>
    </div>

    {#if phase.phase === 'pending'}
      <p class="intro">{$t('migration_intro')}</p>
      <p class="warn"><Icon name="alert-triangle" size={14} /> {$t('migration_close_old')}</p>

      <dl class="facts">
        <dt>{$t('migration_found')}</dt>
        <dd>{$t('migration_files', { n: String(phase.files), size: formatBytes(phase.bytes) })}</dd>
        <dt>{$t('migration_from')}</dt>
        <dd class="path">{phase.old_dir}</dd>
        <dt>{$t('migration_to')}</dt>
        <dd class="path">{phase.new_dir}</dd>
        <dt>{$t('migration_backup')}</dt>
        <dd class="path">{phase.backup_dir}</dd>
      </dl>

      {#if startError}
        <p class="error">{startError}</p>
      {/if}
      <div class="actions">
        <button class="btn btn-primary" onclick={start}>{$t('migration_start')}</button>
      </div>

    {:else if phase.phase === 'running'}
      <p class="step">{stepLabel}</p>
      <div class="bar"><div class="fill" style="width: {progress.percent}%"></div></div>
      <p class="bytes">
        {progress.percent}% · {formatBytes(progress.done_bytes)} / {formatBytes(progress.total_bytes)}
      </p>

    {:else if phase.phase === 'done'}
      <h2 class="ok"><Icon name="check" size={18} /> {$t('migration_done_title')}</h2>
      <ul class="checks">
        <li><Icon name="check" size={14} /> {$t('migration_files', { n: String(phase.files), size: formatBytes(phase.bytes) })}</li>
        <li><Icon name="check" size={14} /> {$t('migration_check_files')}</li>
        <li><Icon name="check" size={14} /> {$t('migration_check_db')}</li>
        <li><Icon name="check" size={14} /> {$t('migration_duration', { s: (phase.duration_ms / 1000).toFixed(1) })}</li>
      </ul>
      <dl class="facts">
        <dt>{$t('migration_to')}</dt>
        <dd class="path">{phase.new_dir}</dd>
        <dt>{$t('migration_backup')}</dt>
        <dd class="path">{phase.backup_dir}</dd>
      </dl>
      <p class="hint">{$t('migration_backup_hint')}</p>
      <div class="actions">
        <button class="btn btn-primary" onclick={onfinished}>{$t('migration_continue')}</button>
      </div>

    {:else if phase.phase === 'failed'}
      <h2 class="bad"><Icon name="alert-triangle" size={18} /> {$t('migration_failed_title')}</h2>
      <p class="error">{phase.message}</p>
      <p class="hint">{phase.data_intact ? $t('migration_intact') : $t('migration_restore_hint')}</p>
      {#if phase.backup_dir}
        <dl class="facts">
          <dt>{$t('migration_backup')}</dt>
          <dd class="path">{phase.backup_dir}</dd>
        </dl>
      {/if}
      <div class="actions">
        <button class="btn btn-ghost" onclick={() => api.migration.quit()}>{$t('migration_quit')}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .migration {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
    color: var(--text);
    padding: 24px;
  }

  .card {
    width: min(640px, 100%);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg, 12px);
    padding: 28px 32px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .logo {
    width: 36px;
    height: 36px;
  }

  h1 {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
  }

  h2 {
    font-size: 16px;
    font-weight: 600;
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .ok { color: var(--success-text); }
  .bad { color: var(--danger-text); }

  .intro, .hint {
    color: var(--text-body);
    font-size: var(--fs-sm, 13px);
    line-height: 1.5;
    margin: 0;
  }

  .warn {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--danger-text);
    font-size: var(--fs-sm, 13px);
    margin: 0;
  }

  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 0;
    font-size: var(--fs-sm, 13px);
  }

  dt { color: var(--text-faint); }
  dd { margin: 0; }

  .path {
    font-family: var(--font-mono, monospace);
    font-size: 12px;
    word-break: break-all;
    color: var(--text-soft);
  }

  .step {
    margin: 0;
    font-size: var(--fs-sm, 13px);
  }

  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s ease;
  }

  .bytes {
    margin: 0;
    color: var(--text-2);
    font-size: 12px;
    font-family: var(--font-mono, monospace);
  }

  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--fs-sm, 13px);
  }

  .checks li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--success-text);
  }

  .error {
    margin: 0;
    color: var(--danger-text);
    font-size: var(--fs-sm, 13px);
    word-break: break-word;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 6px;
  }
</style>
