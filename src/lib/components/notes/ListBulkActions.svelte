<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/i18n';
  import { notesStore } from '$lib/store/notes.svelte';
  import type { NoteListItem } from '$lib/types';

  interface Props {
    /** Current list filter is the trash */
    isTrash: boolean;
    /** Notes shown in the list right now */
    notes: NoteListItem[];
  }

  let { isTrash, notes }: Props = $props();

  let confirming = $state(false);
  let busy = $state(false);

  async function run() {
    busy = true;
    try {
      if (isTrash) await notesStore.emptyTrash();
      else await notesStore.deleteMany(notes.map((n) => n.id));
      confirming = false;
    } finally {
      busy = false;
    }
  }
</script>

{#if isTrash}
  <button class="btn btn-ghost btn-sm danger" disabled={!notes.length} onclick={() => (confirming = true)}>
    <Icon name="trash" size={14} /> {$t('notes_btn_empty_trash')}
  </button>
{:else}
  <button
    class="icon-btn danger-soft"
    disabled={!notes.length}
    title={$t('notes_btn_delete_all')}
    aria-label={$t('notes_btn_delete_all')}
    onclick={() => (confirming = true)}
  >
    <Icon name="trash" size={14} />
  </button>
{/if}

{#if confirming}
  <div class="delete-overlay">
    <div class="delete-modal">
      <p class="delete-title">{isTrash ? $t('notes_btn_empty_trash') : $t('notes_btn_delete_all')}</p>
      <p class="delete-warn">
        {isTrash
          ? $t('notes_empty_trash_confirm', { n: String(notes.length) })
          : $t('notes_delete_all_confirm', { n: String(notes.length) })}
      </p>
      <div class="delete-actions">
        <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => (confirming = false)}>{$t('notes_btn_cancel')}</button>
        <button class="btn btn-danger btn-sm" disabled={busy} onclick={run}>
          {isTrash ? $t('notes_btn_empty_trash') : $t('notes_btn_delete_all')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .btn.danger { color: var(--danger-text); }
  .btn.danger:hover:not(:disabled) { background: var(--danger-bg); }
  .icon-btn { width: 32px; height: 32px; }

  .delete-overlay {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }
  .delete-modal {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: var(--sp-5) var(--sp-6);
    max-width: 320px;
    width: 90%;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .delete-title { margin: 0; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .delete-warn { margin: 0; font-size: var(--fs-sm); color: var(--text-2); line-height: 1.5; }
  .delete-actions { display: flex; gap: var(--sp-2); justify-content: flex-end; margin-top: var(--sp-1); }
</style>
