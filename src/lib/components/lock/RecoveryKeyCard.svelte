<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- One-time view of a recovery key: copy, optional file save, acknowledge, done. -->
<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';
  import { isMobile } from '$lib/platform';

  interface Props {
    code: string;
    title?: string;
    intro?: string;
    ondone: () => void;
  }

  let { code, title, intro, ondone }: Props = $props();

  let copied = $state(false);
  let acknowledged = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    await navigator.clipboard.writeText(code);
    copied = true;
    clearTimeout(copyTimer);
    copyTimer = setTimeout(() => (copied = false), 2000);
  }

  async function saveToFile() {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const dest = await save({ defaultPath: 'veydan-recovery-key.txt' });
    if (!dest) return;
    const { writeTextFile } = await import('@tauri-apps/plugin-fs');
    await writeTextFile(dest, `Veydan recovery key\n${code}\n`);
  }
</script>

<div class="recovery">
  <div class="head">
    <Icon name="key" size={24} />
    <h3>{title ?? $t('lock_recovery_title')}</h3>
  </div>
  <p class="intro">{intro ?? $t('lock_recovery_intro')}</p>
  <div class="code">{code}</div>
  <button class="btn btn-primary wide" type="button" onclick={copy}>
    <Icon name={copied ? 'check' : 'copy'} size={16} />
    {copied ? $t('lock_recovery_copied') : $t('lock_recovery_copy')}
  </button>
  {#if !isMobile}
    <button class="btn btn-ghost wide" type="button" onclick={saveToFile}>
      <Icon name="download" size={16} />
      {$t('lock_recovery_save_file')}
    </button>
  {/if}
  <label class="ack">
    <input type="checkbox" bind:checked={acknowledged} />
    <span>{$t('lock_recovery_ack')}</span>
  </label>
  <button class="btn btn-primary wide" type="button" disabled={!acknowledged} onclick={ondone}>
    {$t('lock_recovery_done')}
  </button>
</div>

<style>
  .recovery { display: flex; flex-direction: column; gap: var(--sp-3); }
  .head { display: flex; align-items: center; gap: var(--sp-2); color: var(--text); }
  .head h3 { margin: 0; font-size: var(--fs-md); font-weight: 600; }
  .intro { margin: 0; font-size: var(--fs-sm); color: var(--text-2); line-height: 1.45; }
  .code {
    padding: var(--sp-3);
    border: 1px dashed var(--accent-border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    font-family: var(--font-mono);
    font-size: 1.05rem;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-align: center;
    word-break: break-all;
    user-select: all;
    color: var(--text);
  }
  .wide { width: 100%; justify-content: center; }
  .ack { display: flex; align-items: flex-start; gap: var(--sp-2); font-size: var(--fs-sm); color: var(--text); cursor: pointer; }
  /* Global input rules set width 100% and a touch min-height, which turns the box into a full-width slab. */
  .ack input[type="checkbox"] {
    width: 1.15rem;
    height: 1.15rem;
    min-height: 0;
    margin: 2px 0 0;
    padding: 0;
    flex: 0 0 auto;
    background: transparent;
    border: 0;
    box-shadow: none;
    accent-color: var(--accent);
  }
  .ack span { flex: 1; min-width: 0; line-height: 1.4; }
</style>
