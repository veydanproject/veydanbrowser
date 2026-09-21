<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onDestroy, onMount, untrack } from 'svelte';
  import { goto, replaceState } from '$app/navigation';
  import { page } from '$app/state';
  import Icon from '$lib/Icon.svelte';
  import {
    api, formatError, onSyncChanged,
    type NavChild, type Note, type NoteAttachment, type NoteChip, type NoteListItem, type NoteTag,
  } from '$lib/mobile/api';
  import { locale, t } from '$lib/mobile/i18n';
  import { AttachmentUrls, attachmentHref } from '$lib/mobile/markdown';
  import { fmtDateTime, loadEditorMode, saveEditorMode, type EditorMode } from '$lib/mobile/notes-editor';
  import NoteRichEditor from '$lib/components/mobile/NoteRichEditor.svelte';
  import WikiLinkSheet from '$lib/components/mobile/WikiLinkSheet.svelte';
  import NoteActionsSheet from '$lib/components/mobile/NoteActionsSheet.svelte';
  import NoteHistorySheet from '$lib/components/mobile/NoteHistorySheet.svelte';
  import NoteLinksSheet from '$lib/components/mobile/NoteLinksSheet.svelte';
  import MoveFolderSheet from '$lib/components/mobile/MoveFolderSheet.svelte';
  import NoteAttachSheet from '$lib/components/mobile/NoteAttachSheet.svelte';
  import { WIKI_MARK, unclosedWikiAt, wikiMarkup } from '$lib/tiptap-ext';

  // "new" means nothing is stored yet; the note is created on the first edit.
  let id = $state(page.params.id ?? 'new');
  let title = $state('');
  let content = $state('');
  let tags = $state<string[]>([]);
  let chips = $state<NoteChip[]>([]);
  let pinned = $state(false);
  let archived = $state(false);
  let createdAt = $state('');
  let updatedAt = $state('');
  let error = $state('');
  let status = $state<'idle' | 'dirty' | 'saving' | 'saved' | 'failed'>('idle');
  let editingTitle = $state(false);
  // Attachments are loaded before the editor mounts so image sources resolve.
  let ready = $state(untrack(() => id) === 'new');

  let mode = $state<EditorMode>(loadEditorMode());
  let sheet = $state<'none' | 'actions' | 'links' | 'history' | 'move' | 'attach'>('none');
  let allTags = $state<NoteTag[]>([]);
  let allNotes = $state<NoteListItem[]>([]);
  let folders = $state<NavChild[]>([]);
  let tagInput = $state('');
  let addingTag = $state(false);
  let attachments = $state<NoteAttachment[]>([]);
  let thumbs = $state<Record<string, string>>({});
  let urls = $state(new AttachmentUrls(untrack(() => id)));
  let wikiQuery = $state('');
  let wikiOpen = $state(false);
  let wikiStart = 0;
  let busy = $state(false);

  let rich = $state<NoteRichEditor>();
  let body = $state<HTMLTextAreaElement>();
  let fileInput = $state<HTMLInputElement>();
  let imageInput = $state<HTMLInputElement>();

  let saveTimer: ReturnType<typeof setTimeout>;
  let saving: Promise<void> | null = null;

  const folderChip = $derived(chips.find((c) => c.kind === 'folder'));
  const isNew = $derived(id === 'new');

  function applyNote(n: Note) {
    title = n.title;
    content = n.content;
    tags = n.tags;
    chips = n.chips;
    pinned = n.pinned;
    archived = n.archived;
    createdAt = n.created_at;
    updatedAt = n.updated_at;
  }

  async function load() {
    if (id === 'new') return;
    try {
      const n = await api.notes.get(id);
      await loadAttachments();
      applyNote(n);
      if (status !== 'dirty' && status !== 'saving') status = 'saved';
      ready = true;
    } catch (e) {
      error = formatError(e);
    }
  }

  async function loadAttachments() {
    if (id === 'new') return;
    attachments = await api.attachments.list(id);
    const names = new Set(attachments.map((a) => a.name));
    for (const name of Object.keys(thumbs)) {
      if (!names.has(name)) {
        urls.forget(name);
        delete thumbs[name];
      }
    }
    for (const a of attachments) {
      if (a.is_image && !thumbs[a.name]) {
        const url = await urls.get(a.name);
        if (url) thumbs[a.name] = url;
      }
    }
  }

  function loadCatalogs() {
    api.notes.tags().then((x) => (allTags = x)).catch(() => {});
    api.notes.list().then((x) => (allNotes = x)).catch(() => {});
    api.notes.nav().then((x) => (folders = x.folders)).catch(() => {});
  }

  onMount(() => {
    load();
    loadCatalogs();
    // Remote edit arrived while open: reload unless local edits are pending.
    const unlisten = onSyncChanged(['note', 'note_attachment', 'note_tag', 'workspace', 'profile', 'note_folder'], () => {
      if (status !== 'dirty' && status !== 'saving') load();
      loadCatalogs();
    });
    return () => unlisten.then((f) => f());
  });

  // Following a wiki link keeps this component; reload when the route param changes.
  let seenParam = page.params.id;
  $effect(() => {
    const next = page.params.id ?? 'new';
    if (next === seenParam) return;
    seenParam = next;
    // The first save rewrites the URL from /new to the real id; nothing to reload then.
    if (next !== untrack(() => id)) void switchNote(next);
  });

  async function switchNote(next: string) {
    await save();
    urls.release();
    urls = new AttachmentUrls(next);
    id = next;
    title = '';
    content = '';
    tags = [];
    chips = [];
    pinned = false;
    archived = false;
    attachments = [];
    thumbs = {};
    wikiQuery = '';
    wikiOpen = false;
    sheet = 'none';
    ready = next === 'new';
    status = 'idle';
    await load();
  }

  // ── Editor ──

  /** Attachment images render from blob URLs read through the backend. */
  function resolveSrc(src: string): string {
    const prefix = `attachments/${id}/`;
    let decoded = src;
    try { decoded = decodeURIComponent(src); } catch { /* keep raw */ }
    if (!decoded.startsWith(prefix)) return src;
    return thumbs[decoded.slice(prefix.length)] ?? src;
  }

  function toggleMode() {
    mode = mode === 'rich' ? 'md' : 'rich';
    wikiQuery = '';
    wikiOpen = false;
    saveEditorMode(mode);
  }

  function onRichChange(md: string) {
    content = md;
    onEdit();
  }

  /** Track an unclosed `@@` before the caret in source mode. */
  function updateWikiMd() {
    const pos = body?.selectionStart ?? content.length;
    const before = content.slice(0, pos);
    const open = unclosedWikiAt(before);
    if (open < 0) return;
    wikiStart = open;
    wikiQuery = before.slice(open + WIKI_MARK.length);
    wikiOpen = true;
  }

  function pickWiki(name: string) {
    if (mode === 'rich') {
      rich?.pickWikiLink(name);
      wikiQuery = '';
      wikiOpen = false;
      return;
    }
    const pos = body?.selectionStart ?? content.length;
    const link = wikiMarkup(name);
    const rest = content.slice(pos);
    const skip = rest.startsWith(WIKI_MARK) ? WIKI_MARK.length : 0;
    content = content.slice(0, wikiStart) + link + rest.slice(skip);
    wikiQuery = '';
    wikiOpen = false;
    onEdit();
    const caret = wikiStart + link.length;
    requestAnimationFrame(() => body?.setSelectionRange(caret, caret));
  }

  /** Open the linked note; create it when nothing matches. */
  async function openWikiLink(target: string) {
    const name = target.trim();
    if (!name) return;
    try {
      await save();
      let hit = await api.notes.resolveLink(name);
      if (!hit) hit = (await api.notes.create(name)).id;
      await goto(`/notes/${hit}`);
    } catch (e) {
      error = formatError(e);
    }
  }

  function openNote(noteId: string) {
    sheet = 'none';
    void goto(`/notes/${noteId}`);
  }

  // ── Tags ──

  const tagCloud = $derived.by(() => {
    const q = tagInput.trim().toLowerCase().replace(/^#/, '');
    return allTags
      .filter((x) => !tags.includes(x.name) && (!q || x.name.toLowerCase().includes(q)))
      .map((x) => x.name);
  });
  const canCreateTag = $derived.by(() => {
    const name = tagInput.trim().replace(/^#/, '');
    if (!name || tags.includes(name)) return false;
    return !allTags.some((x) => x.name.toLowerCase() === name.toLowerCase());
  });

  function pickTag(name: string) {
    const n = name.trim().replace(/^#/, '');
    if (!n || tags.includes(n)) return;
    tags = [...tags, n];
    tagInput = '';
    onEdit();
  }

  function addTag() {
    pickTag(tagInput);
  }

  function closeTagInput() {
    tagInput = '';
    addingTag = false;
  }

  function removeTag(name: string) {
    tags = tags.filter((x) => x !== name);
    onEdit();
  }

  // ── Attachments ──

  async function attach(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = '';
    if (!files.length) return;
    try {
      if (id === 'new') {
        status = 'dirty';
        await save();
        if (id === 'new') return;
      }
      const added: NoteAttachment[] = [];
      for (const f of files) added.push(await api.attachments.add(id, f));
      await loadAttachments();
      for (const a of added) if (a.is_image) insertAttachment(a);
    } catch (err) {
      error = formatError(err);
    }
  }

  function insertText(text: string) {
    if (mode === 'rich' && rich) {
      rich.insertMarkdown(text);
      return;
    }
    const pos = body?.selectionStart ?? content.length;
    const before = content.slice(0, pos);
    const sep = before && !before.endsWith('\n') ? '\n' : '';
    content = `${before}${sep}${text}\n${content.slice(pos)}`;
    onEdit();
  }

  function insertAttachment(a: NoteAttachment) {
    insertText(a.is_image ? `![${a.name}](${attachmentHref(a.rel_path)})` : `[${a.name}](${attachmentHref(a.rel_path)})`);
  }

  async function removeAttachment(a: NoteAttachment) {
    if (!confirm($t('notes_attach_delete_confirm', { name: a.name }))) return;
    try {
      await api.attachments.delete(id, a.name);
      await loadAttachments();
    } catch (err) {
      error = formatError(err);
    }
  }

  // ── Save ──

  function onEdit() {
    status = 'dirty';
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 700);
  }

  async function save() {
    clearTimeout(saveTimer);
    if (status !== 'dirty') return saving ?? undefined;
    if (saving) {
      // Let the in-flight write finish, then persist the latest state.
      await saving;
      return save();
    }
    status = 'saving';
    saving = (async () => {
      try {
        if (id === 'new') {
          if (!title.trim() && !content.trim()) {
            status = 'idle';
            return;
          }
          const n = await api.notes.create(title, content, tags);
          id = n.id;
          urls = new AttachmentUrls(id);
          createdAt = n.created_at;
          updatedAt = n.updated_at;
          replaceState(`/notes/${id}`, {});
        } else {
          const n = await api.notes.update(id, { title, content, tags });
          updatedAt = n.updated_at;
          chips = n.chips;
        }
        status = 'saved';
      } catch (e) {
        error = formatError(e);
        status = 'failed';
      } finally {
        saving = null;
      }
    })();
    return saving;
  }

  async function setFlags(input: { pinned?: boolean; archived?: boolean }) {
    sheet = 'none';
    if (id === 'new') return;
    try {
      const n = await api.notes.update(id, input);
      pinned = n.pinned;
      archived = n.archived;
      api.sync.trigger().catch(() => {});
    } catch (e) {
      error = formatError(e);
    }
  }

  async function moveTo(folderId: string | null) {
    busy = true;
    try {
      await api.notes.setFolder(id, folderId);
      const n = await api.notes.get(id);
      chips = n.chips;
      sheet = 'none';
      api.sync.trigger().catch(() => {});
    } catch (e) {
      error = formatError(e);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    sheet = 'none';
    if (id === 'new') return goto('/notes');
    if (!confirm($t('notes_delete_confirm', { title: title || $t('notes_untitled') }))) return;
    clearTimeout(saveTimer);
    status = 'idle';
    try {
      await api.notes.delete(id);
      goto('/notes', { replaceState: true });
    } catch (e) {
      error = formatError(e);
    }
  }

  function back() {
    if (history.length > 1) history.back();
    else void goto('/notes');
  }

  // Flush a pending edit when the user navigates away.
  onDestroy(() => {
    if (status === 'dirty') save();
    urls.release();
  });
</script>

<div class="editor">
  <div class="m-header">
    <button class="m-ibtn" onclick={back} aria-label={$t('common_back')}>
      <Icon name="chevron-left" size={24} />
    </button>
    {#if editingTitle}
      <input
        class="crumb-edit"
        bind:value={title}
        oninput={onEdit}
        onblur={() => (editingTitle = false)}
        onkeydown={(e) => e.key === 'Enter' && (editingTitle = false)}
        placeholder={$t('notes_title_placeholder')}
        {@attach (el: HTMLInputElement) => el.focus()}
      />
    {:else}
      <button type="button" class="crumb" onclick={() => (editingTitle = true)}>
        <span class="name">{title.trim() || $t('notes_untitled')}</span>
        <Icon name="pencil" size={14} />
      </button>
    {/if}
    <div class="acts">
      <button type="button" class="act" class:on={pinned} disabled={isNew} onclick={() => setFlags({ pinned: !pinned })} aria-label={pinned ? $t('notes_unpin') : $t('notes_pin')}>
        <Icon name="pin" size={14} />
      </button>
      {#if status !== 'idle'}
        <span
          class="act save"
          class:ok={status === 'saved'}
          class:busy={status === 'saving' || status === 'dirty'}
          class:bad={status === 'failed'}
          class:spin={status === 'saving'}
          title={status === 'saved' ? $t('notes_saved') : status === 'failed' ? error : $t('notes_saving')}
        >
          <Icon name={status === 'saved' ? 'check-circle' : status === 'failed' ? 'alert-triangle' : status === 'saving' ? 'loader' : 'circle'} size={14} />
        </span>
      {/if}
      <button type="button" class="act" onclick={() => (sheet = 'actions')} aria-label={$t('notes_actions')}>
        <Icon name="more-vertical" size={14} />
      </button>
    </div>
  </div>
  <input class="hidden" type="file" multiple bind:this={fileInput} onchange={attach} />
  <input class="hidden" type="file" accept="image/*" multiple bind:this={imageInput} onchange={attach} />

  <div class="meta">
    {#if error}
      <div class="m-error">{error}</div>
    {/if}
    <div class="m-chips tags">
      {#each chips.filter((c) => c.kind !== 'tag' && c.kind !== 'folder') as c (c.kind + c.id)}
        <span class="m-chip small" class:ws={c.kind === 'workspace'} style:--chip={c.color}>{c.label}</span>
      {/each}
      {#each tags as name (name)}
        <button type="button" class="m-chip" onclick={() => removeTag(name)} aria-label={$t('notes_tag_add')}>#{name}</button>
      {/each}
      {#if addingTag}
        <input
          class="tag-input"
          bind:value={tagInput}
          onblur={closeTagInput}
          onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addTag(); } if (e.key === 'Escape') closeTagInput(); }}
          placeholder={$t('notes_tag_add')}
          {@attach (el: HTMLInputElement) => el.focus()}
        />
      {:else}
        <button class="m-chip add" onclick={() => (addingTag = true)} aria-label={$t('notes_tag_add')}>
          <Icon name="plus" size={14} />
        </button>
      {/if}
    </div>
    {#if addingTag}
      <div class="m-chips cloud">
        {#each tagCloud as name (name)}
          <button type="button" class="m-chip" onpointerdown={(e) => { e.preventDefault(); pickTag(name); }}>#{name}</button>
        {/each}
        {#if canCreateTag}
          <button type="button" class="m-chip add-new" onpointerdown={(e) => { e.preventDefault(); addTag(); }}>
            <Icon name="plus" size={14} /> #{tagInput.trim().replace(/^#/, '')}
          </button>
        {/if}
      </div>
    {/if}
    {#if updatedAt}
      <span class="when">{fmtDateTime(updatedAt, $locale)}</span>
    {/if}
  </div>

  <div class="scroll">
  {#if !ready}
    <div class="body"></div>
  {:else if mode === 'rich'}
    <NoteRichEditor
      bind:this={rich}
      {content}
      {resolveSrc}
      placeholder={$t('notes_body_placeholder')}
      onchange={onRichChange}
      onwiki={(q) => { if (q !== null) { wikiQuery = q; wikiOpen = true; } }}
      onwikilink={openWikiLink}
    />
  {:else}
    <textarea
      class="body"
      bind:this={body}
      bind:value={content}
      oninput={() => { onEdit(); updateWikiMd(); }}
      onclick={updateWikiMd}
      onkeyup={updateWikiMd}
      placeholder={$t('notes_body_placeholder')}
    ></textarea>
  {/if}
  </div>

</div>

<div class="toolbar" class:hide={wikiOpen}>
  <button type="button" onclick={() => (sheet = 'attach')} aria-label={$t('notes_attach')}>
    <Icon name="paperclip" size={22} />
  </button>
  <button type="button" class:on={mode === 'md'} onclick={toggleMode} aria-label={$t('notes_mode')}>
    <Icon name="type" size={22} />
  </button>
  <button type="button" disabled={isNew} onclick={() => (sheet = 'links')} aria-label={$t('notes_links')}>
    <Icon name="link" size={22} />
  </button>
</div>

<NoteActionsSheet
  open={sheet === 'actions'}
  noteId={id}
  {pinned}
  {archived}
  onclose={() => (sheet = 'none')}
  onhistory={() => (sheet = 'history')}
  onpin={() => setFlags({ pinned: !pinned })}
  onarchive={() => setFlags({ archived: !archived })}
  onmove={() => (sheet = 'move')}
  ondelete={remove}
  onsynced={() => { if (status !== 'dirty' && status !== 'saving') load(); }}
/>
<NoteLinksSheet open={sheet === 'links'} noteId={id} onclose={() => (sheet = 'none')} onopen={openNote} />
<NoteHistorySheet
  open={sheet === 'history'}
  noteId={id}
  {urls}
  onclose={() => (sheet = 'none')}
  onrestored={(n) => { applyNote(n); status = 'saved'; }}
/>
<MoveFolderSheet
  open={sheet === 'move'}
  {folders}
  current={folderChip?.id ?? null}
  {busy}
  onclose={() => (sheet = 'none')}
  onmove={moveTo}
/>
<NoteAttachSheet
  open={sheet === 'attach'}
  {attachments}
  {thumbs}
  {createdAt}
  {updatedAt}
  folderLabel={folderChip?.label ?? ''}
  {tags}
  onclose={() => (sheet = 'none')}
  onfile={() => fileInput?.click()}
  onimage={() => imageInput?.click()}
  oninsert={(a) => { insertAttachment(a); sheet = 'none'; }}
  onremove={removeAttachment}
/>
<WikiLinkSheet
  open={wikiOpen}
  seed={wikiQuery}
  notes={allNotes}
  excludeId={id}
  onclose={() => { wikiOpen = false; wikiQuery = ''; }}
  onpick={pickWiki}
/>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0 0 calc(72px + env(safe-area-inset-bottom));
    animation: vfade var(--dur-base) ease-out;
  }
  .m-header {
    flex-shrink: 0;
    position: relative;
    top: auto;
    margin: 0;
  }
  .meta {
    flex-shrink: 0;
    padding: 0 var(--sp-4) var(--sp-2);
    background: var(--bg);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    padding: 0 var(--sp-4);
  }
  .crumb {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .crumb .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 17px; font-weight: 700; }
  .crumb :global(svg) { flex-shrink: 0; color: var(--text-3); }
  .crumb-edit {
    flex: 1;
    min-width: 0;
    min-height: 40px;
    padding: 0 4px;
    border: 0;
    border-radius: 8px;
    background: var(--m-field);
    font-size: 17px;
    font-weight: 700;
  }
  .crumb-edit:focus { box-shadow: none; }
  .m-header { padding-right: 6px; }
  .acts { display: flex; align-items: center; gap: 4px; flex-shrink: 0; }
  .act {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--text-3);
  }
  .act.on { color: var(--accent); background: var(--accent-tint); }
  .act:disabled { opacity: 0.35; }
  .save.ok { color: var(--success-text); }
  .save.busy { color: var(--warn-text); }
  .save.bad { color: var(--danger-text); }
  .save.spin :global(svg) { animation: spin 1s linear infinite; }
  .when { display: block; margin-top: 2px; font-size: 11px; color: var(--text-3); }
  .body {
    border: 0;
    background: transparent;
    padding: var(--sp-2) 0;
    resize: none;
    min-height: 200px;
    field-sizing: content;
    font-family: var(--font-mono);
    font-size: 15px;
    line-height: 1.55;
  }
  .body:focus { box-shadow: none; }
  .editor :global(.rich) { flex: none; }
  .editor :global(.ProseMirror) { overflow: visible; min-height: 200px; font-size: 16px; line-height: 1.55; }
  .hidden { display: none; }

  .tags { margin-bottom: var(--sp-2); }
  .cloud { margin-bottom: var(--sp-2); }
  .m-chip.add-new { width: auto; padding: 0 12px; }
  .tag-input {
    width: 8em;
    min-height: 28px;
    padding: 0 10px;
    font-size: 13px;
    border: 0;
    border-radius: 999px;
    background: var(--m-field);
  }

  .toolbar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 20;
    display: flex;
    justify-content: space-around;
    align-items: center;
    height: calc(56px + env(safe-area-inset-bottom));
    padding-bottom: env(safe-area-inset-bottom);
    background: var(--m-nav);
    border-top: 1px solid var(--border);
  }
  .toolbar button {
    width: 48px;
    height: 48px;
    padding: 0;
    border: 0;
    border-radius: 12px;
    background: transparent;
    color: var(--text-2);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .toolbar button.on { color: var(--accent); }
  .toolbar button:active { background: var(--m-seg); }
  .toolbar button:disabled { opacity: 0.35; }
  .toolbar.hide { display: none; }
</style>
