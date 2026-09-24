<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { t, type TranslationKey } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import {
    HOTKEY_GROUPS,
    assignChord,
    captureEvent,
    clearCommand,
    formatCommand,
    isOverridden,
    keybindingOverrides,
    resetCommand,
    shortcutCapture,
    type CommandId,
  } from '$lib/keybindings';

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

  const GROUP_LABEL: Record<(typeof HOTKEY_GROUPS)[number]['id'], TranslationKey> = {
    edit: 'hotkey_group_edit',
    editor: 'hotkey_group_editor',
    app: 'hotkey_group_app',
  };

  const COMMAND_LABEL: Record<CommandId, TranslationKey> = {
    'edit.undo': 'hotkey_edit_undo',
    'edit.redo': 'hotkey_edit_redo',
    'editor.bold': 'hotkey_editor_bold',
    'editor.italic': 'hotkey_editor_italic',
    'editor.link': 'hotkey_editor_link',
    'editor.find': 'hotkey_editor_find',
    'editor.save': 'hotkey_editor_save',
    'app.palette': 'hotkey_app_palette',
    'app.inspector': 'hotkey_app_inspector',
  };

  let recording = $state<CommandId | null>(null);
  let rowError = $state<{ id: CommandId; text: string } | null>(null);

  let quickShortcut = $state('');
  let quickShortcutSaving = $state(false);
  let quickShortcutError = $state('');

  function keysOf(id: CommandId): string {
    return formatCommand(id, $keybindingOverrides) || $t('hotkey_unbound');
  }

  function start(id: CommandId) {
    recording = id;
    rowError = null;
  }

  $effect(() => {
    const id = recording;
    shortcutCapture.set(id !== null);
    if (!id) return;
    const translate = $t;
    const onKey = (e: KeyboardEvent) => {
      const result = captureEvent(e);
      if (result.kind === 'ignore') return;
      e.preventDefault();
      e.stopImmediatePropagation();
      if (result.kind === 'cancel') { recording = null; return; }
      if (result.kind === 'clear') { clearCommand(id); recording = null; return; }
      if (result.kind === 'reject') {
        rowError = { id, text: translate('hotkey_need_modifier') };
        return;
      }
      const other = assignChord(id, result.chord);
      if (other) {
        rowError = { id, text: translate('hotkey_conflict', { name: translate(COMMAND_LABEL[other]) }) };
        return;
      }
      rowError = null;
      recording = null;
    };
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      shortcutCapture.set(false);
    };
  });

  async function saveQuickShortcut() {
    quickShortcutSaving = true;
    quickShortcutError = '';
    try {
      quickShortcut = await api.notes.quickCaptureShortcutSet(quickShortcut);
    } catch (e) {
      quickShortcutError = `${$t('settings_quick_capture_invalid')}: ${formatError(e)}`;
    } finally {
      quickShortcutSaving = false;
    }
  }

  onMount(() => {
    api.notes.quickCaptureShortcutGet().then((v) => { quickShortcut = v; }).catch(() => {});
  });
</script>

<div class="card">
  <div class="card-title">{$t('hotkey_section')}</div>
  <p class="muted">{$t('hotkey_hint')}</p>

  {#each HOTKEY_GROUPS as group (group.id)}
    <div class="group">{$t(GROUP_LABEL[group.id])}</div>
    {#each group.commands as id (id)}
      <div class="row">
        <span class="name">{$t(COMMAND_LABEL[id])}</span>
        <span class="keys" class:unbound={recording !== id && !formatCommand(id, $keybindingOverrides)}>
          {#if recording === id}
            {$t('hotkey_record')}
          {:else}
            {keysOf(id)}
          {/if}
        </span>
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => start(id)}>
          {$t('hotkey_change')}
        </button>
        <button
          type="button"
          class="btn btn-ghost btn-sm"
          disabled={!isOverridden(id, $keybindingOverrides)}
          onclick={() => { resetCommand(id); if (recording === id) recording = null; rowError = null; }}
        >
          {$t('hotkey_reset')}
        </button>
      </div>
      {#if rowError?.id === id}
        <div class="error-msg">{rowError.text}</div>
      {/if}
    {/each}
  {/each}

  <div class="group">{$t('settings_quick_capture_section')}</div>
  <p class="muted">{$t('settings_quick_capture_hint')}</p>
  <div class="dir-row">
    <input
      class="dir-input"
      type="text"
      bind:value={quickShortcut}
      placeholder="CmdOrCtrl+Shift+N"
      readonly={!isTauri}
    />
  </div>
  <div class="btn-row">
    <button class="btn btn-primary btn-sm" disabled={quickShortcutSaving || !isTauri} onclick={saveQuickShortcut}>
      {quickShortcutSaving ? $t('settings_notes_saving') : $t('settings_notes_save')}
    </button>
  </div>
  {#if quickShortcutError}
    <div class="error-msg">{quickShortcutError}</div>
  {/if}
</div>

<style>
  .card {
    padding: var(--sp-6);
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .card-title {
    font-size: var(--fs-2xs);
    font-weight: var(--fw-bold);
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 1px;
  }
  .muted { font-size: var(--fs-base); margin: 0; }
  .group {
    margin-top: var(--sp-2);
    font-size: var(--fs-2xs);
    font-weight: var(--fw-bold);
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 1px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 36px;
  }
  .name { flex: 1; font-size: var(--fs-sm); color: var(--text); }
  .keys {
    min-width: 9rem;
    text-align: right;
    font-family: var(--font-mono);
    font-size: var(--fs-xs);
    color: var(--text-body);
  }
  .keys.unbound { color: var(--text-faint); }
  .btn-sm { padding: 0.35rem var(--sp-3); font-size: var(--fs-sm); }
  .error-msg { font-size: var(--fs-sm); color: var(--danger-text); margin-top: -0.25rem; }
  .dir-row { display: flex; align-items: center; gap: 0.4rem; }
  .dir-input {
    flex: 1; height: 44px; background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 0 var(--sp-3);
    font-size: 0.82rem; color: var(--text-body); font-family: var(--font-mono);
    outline: none;
  }
  .dir-input:focus { border-color: var(--accent-border); }
  .btn-row { display: flex; gap: var(--sp-2); align-items: center; }
</style>
