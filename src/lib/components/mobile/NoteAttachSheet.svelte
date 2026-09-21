<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { locale, t } from '$lib/mobile/i18n';
  import { fmtDateTimeLong, fmtSize } from '$lib/mobile/notes-editor';
  import type { NoteAttachment } from '$lib/mobile/api';
  import BottomSheet from './BottomSheet.svelte';

  interface Props {
    open: boolean;
    attachments: NoteAttachment[];
    thumbs: Record<string, string>;
    createdAt: string;
    updatedAt: string;
    folderLabel: string;
    tags: string[];
    onclose: () => void;
    onfile: () => void;
    onimage: () => void;
    oninsert: (a: NoteAttachment) => void;
    onremove: (a: NoteAttachment) => void;
  }

  let {
    open, attachments, thumbs, createdAt, updatedAt, folderLabel, tags,
    onclose, onfile, onimage, oninsert, onremove,
  }: Props = $props();
</script>

<BottomSheet {open} title={$t('notes_attachments')} {onclose}>
  <div class="actions">
    <button type="button" class="act" onclick={onimage}>
      <span class="m-doc lg"><Icon name="image" size={20} /></span>
      {$t('notes_attach_image')}
    </button>
    <button type="button" class="act" onclick={onfile}>
      <span class="m-doc lg"><Icon name="file" size={20} /></span>
      {$t('notes_attach_file')}
    </button>
  </div>

  {#if attachments.length}
    <h3>{$t('notes_attachments_count', { n: String(attachments.length) })}</h3>
    <div class="list">
      {#each attachments as a (a.name)}
        <div class="row">
          <button type="button" class="open" onclick={() => oninsert(a)} aria-label={$t('notes_insert')}>
            {#if a.is_image && thumbs[a.name]}
              <img src={thumbs[a.name]} alt="" />
            {:else}
              <span class="m-doc"><Icon name="file" size={16} /></span>
            {/if}
            <span class="meta">
              <span class="name">{a.name}</span>
              <span class="size">{fmtSize(a.size)}</span>
            </span>
          </button>
          <button type="button" class="x" onclick={() => onremove(a)} aria-label={$t('totp_delete')}>
            <Icon name="x" size={14} />
          </button>
        </div>
      {/each}
    </div>
  {/if}

  <dl class="m-meta">
    <dt>{$t('notes_meta_created')}</dt><dd>{createdAt ? fmtDateTimeLong(createdAt, $locale) : '—'}</dd>
    <dt>{$t('notes_meta_updated')}</dt><dd>{updatedAt ? fmtDateTimeLong(updatedAt, $locale) : '—'}</dd>
    <dt>{$t('notes_meta_folder')}</dt><dd>{folderLabel || '—'}</dd>
    <dt>{$t('notes_meta_tags')}</dt>
    <dd>
      {#if tags.length}
        <span class="m-chips">{#each tags as name (name)}<span class="m-chip small">#{name}</span>{/each}</span>
      {:else}—{/if}
    </dd>
  </dl>
</BottomSheet>

<style>
  .actions { display: flex; gap: var(--sp-2); }
  .act {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    min-height: 88px;
    padding: var(--sp-3);
    border: 0;
    border-radius: var(--m-radius);
    background: var(--m-field);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
  }
  h3 { margin: var(--sp-2) 0 0; font-size: 15px; font-weight: 700; }
  .list { display: flex; flex-direction: column; gap: 8px; }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 4px 4px 8px;
    border-radius: var(--m-radius);
    background: var(--m-card);
    border: 1px solid var(--m-card-border);
  }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
  }
  .open img { width: 36px; height: 36px; object-fit: cover; border-radius: 8px; flex-shrink: 0; }
  .meta { display: flex; flex-direction: column; min-width: 0; gap: 2px; }
  .name { font-size: 13px; font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .size { font-size: 11px; color: var(--text-3); }
  .x {
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    margin-right: 4px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: var(--m-seg);
    color: var(--text-2);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
</style>
