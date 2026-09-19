<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { Editor } from '@tiptap/core';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { api } from '$lib/api';
  import { noteExtensions } from '$lib/tiptap-ext';
  import type { EditAction } from '$lib/markdown-edit';
  import type { NoteListItem } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import WikiLinkPicker from './WikiLinkPicker.svelte';
  import { t } from '$lib/i18n';

  interface Props {
    content: string;
    /** Absolute dir the relative attachment links resolve against */
    baseDir?: string;
    readonly?: boolean;
    placeholder?: string;
    notes: NoteListItem[];
    excludeId?: string | null;
    onchange: (md: string) => void;
    onwikilink: (target: string) => void;
    onfiles: (files: File[]) => void;
    /** Hotkeys owned by the parent (save, find, link); return true when handled */
    onhotkey?: (e: KeyboardEvent) => boolean;
  }

  let {
    content, baseDir, readonly = false, placeholder = '', notes, excludeId = null,
    onchange, onwikilink, onfiles, onhotkey,
  }: Props = $props();

  let hostEl: HTMLElement | null = $state(null);
  let wrapEl: HTMLElement | null = $state(null);
  let editor: Editor | null = null;
  let lastEmitted = '';
  let isEmpty = $state(false);

  const isRelative = (url: string) => !/^([a-z][a-z0-9+.-]*:|\/\/|#|\/)/i.test(url);
  const resolveSrc = (src: string) =>
    baseDir && isRelative(src) ? convertFileSrc(`${baseDir}/${decodeURIComponent(src)}`) : src;

  onMount(() => {
    editor = new Editor({
      element: hostEl!,
      extensions: noteExtensions(resolveSrc),
      content,
      contentType: 'markdown',
      editable: !readonly,
      editorProps: {
        attributes: { spellcheck: 'false' },
        handleKeyDown: (_view, e) => onKeydown(e),
        handlePaste: (_view, e) => takeFiles(Array.from(e.clipboardData?.files ?? [])),
        handleDrop: (_view, e) => takeFiles(Array.from(e.dataTransfer?.files ?? [])),
        handleClick: (_view, _pos, e) => onClick(e),
      },
      onUpdate: ({ editor: ed }) => {
        lastEmitted = ed.getMarkdown();
        isEmpty = ed.isEmpty;
        onchange(lastEmitted);
        updateWikiState();
      },
      onSelectionUpdate: updateWikiState,
    });
    lastEmitted = content;
    isEmpty = editor.isEmpty;
    return () => editor?.destroy();
  });

  // Content changed outside the editor (history restore, merge, external file change, source mode)
  $effect(() => {
    const md = content;
    if (!editor || md === lastEmitted) return;
    lastEmitted = md;
    // Not undoable: Ctrl+Z must not bring back the replaced document
    editor.chain().setMeta('addToHistory', false).setContent(md, { contentType: 'markdown', emitUpdate: false }).run();
    isEmpty = editor.isEmpty;
  });

  $effect(() => { editor?.setEditable(!readonly); });

  function takeFiles(files: File[]): boolean {
    if (readonly || files.length === 0) return false;
    onfiles(files);
    return true;
  }

  function onKeydown(e: KeyboardEvent): boolean {
    if (wikiOpen) {
      if (e.key === 'Escape') { wikiOpen = false; return true; }
      if (wikiPicker?.handleKeydown(e)) return true;
    }
    return onhotkey?.(e) ?? false;
  }

  /** Wiki atoms open on click; http links open with Ctrl/Cmd or when read-only, else edit the URL. */
  function onClick(e: MouseEvent): boolean {
    const target = e.target as HTMLElement;
    const wiki = target.closest('a.wiki');
    if (wiki) { onwikilink(wiki.getAttribute('data-target') ?? ''); return true; }
    const anchor = target.closest('a[href]');
    if (!anchor) return false;
    if (e.ctrlKey || e.metaKey || readonly) {
      const href = anchor.getAttribute('href') ?? '';
      if (/^https?:/i.test(href)) void api.system.openUrl(href);
    } else {
      openLinkForm();
    }
    return true;
  }

  export function focus() {
    editor?.commands.focus();
  }

  export function runAction(action: EditAction) {
    if (!editor || readonly) return;
    const c = editor.chain().focus();
    switch (action) {
      case 'h1': case 'h2': case 'h3': c.toggleHeading({ level: Number(action[1]) as 1 | 2 | 3 }).run(); break;
      case 'bold': c.toggleBold().run(); break;
      case 'italic': c.toggleItalic().run(); break;
      case 'strike': c.toggleStrike().run(); break;
      case 'code': c.toggleCode().run(); break;
      case 'ul': c.toggleBulletList().run(); break;
      case 'ol': c.toggleOrderedList().run(); break;
      case 'task': c.toggleTaskList().run(); break;
      case 'quote': c.toggleBlockquote().run(); break;
      case 'codeblock': c.toggleCodeBlock().run(); break;
      case 'hr': c.setHorizontalRule().run(); break;
      case 'link': openLinkForm(); break;
    }
  }

  /** Insert a Markdown snippet at the cursor. */
  export function insertMarkdown(md: string) {
    if (!editor || readonly) return;
    editor.chain().focus().insertContent(md, { contentType: 'markdown' }).run();
  }

  /** Select the first occurrence of `query` (within one text node) and scroll to it. */
  export function selectText(query: string) {
    if (!editor || !query) return;
    const q = query.toLowerCase();
    let found = -1;
    editor.state.doc.descendants((node, pos) => {
      if (found >= 0) return false;
      const i = node.isText ? (node.text ?? '').toLowerCase().indexOf(q) : -1;
      if (i >= 0) found = pos + i;
      return found < 0;
    });
    if (found < 0) return;
    editor.chain().focus().setTextSelection({ from: found, to: found + query.length }).scrollIntoView().run();
  }

  // ── Link form: shown under the caret for Ctrl+K / toolbar / click on a link ──
  let linkOpen = $state(false);
  let linkHref = $state('');
  let linkPos = $state({ x: 0, y: 0 });
  let linkInput: HTMLInputElement | null = $state(null);
  let hasLink = $state(false);

  function openLinkForm() {
    if (!editor || !wrapEl) return;
    const { from, to, empty } = editor.state.selection;
    const sel = editor.state.doc.textBetween(from, to, ' ');
    if (!empty && /^https?:\/\/\S+$/i.test(sel)) {
      editor.chain().focus().setLink({ href: sel }).run();
      return;
    }
    hasLink = editor.isActive('link');
    linkHref = editor.getAttributes('link').href ?? '';
    const caret = editor.view.coordsAtPos(from);
    const box = wrapEl.getBoundingClientRect();
    linkPos = { x: Math.max(0, caret.left - box.left), y: caret.bottom - box.top + 4 };
    linkOpen = true;
    tick().then(() => linkInput?.select());
  }

  function applyLink(e: SubmitEvent) {
    e.preventDefault();
    if (!editor) return;
    const href = linkHref.trim();
    const chain = editor.chain().focus().extendMarkRange('link');
    if (!href) chain.unsetLink().run();
    else if (editor.state.selection.empty && !editor.isActive('link')) {
      chain.insertContent({ type: 'text', text: href, marks: [{ type: 'link', attrs: { href } }] }).run();
    } else chain.setLink({ href }).run();
    linkOpen = false;
  }

  function removeLink() {
    editor?.chain().focus().extendMarkRange('link').unsetLink().run();
    linkOpen = false;
  }

  function closeLinkForm() {
    linkOpen = false;
    editor?.commands.focus();
  }

  // ── Wiki links: `[[` autocomplete in the current text block ─────────────────
  let wikiOpen = $state(false);
  let wikiQuery = $state('');
  let wikiIndex = $state(0);
  let wikiFrom = 0;
  let wikiPicker: WikiLinkPicker | null = $state(null);

  /** Track an unclosed `[[` before the caret inside the current text block. */
  function updateWikiState() {
    if (!editor || readonly) return;
    const { $from: caret, empty } = editor.state.selection;
    if (!empty || !caret.parent.isTextblock) { wikiOpen = false; return; }
    const before = caret.parent.textBetween(0, caret.parentOffset, undefined, '\ufffc');
    const open = before.lastIndexOf('[[');
    if (open < 0 || before.indexOf(']]', open) >= 0) { wikiOpen = false; return; }
    wikiFrom = caret.pos - (before.length - open);
    wikiQuery = before.slice(open + 2);
    wikiIndex = 0;
    wikiOpen = true;
  }

  /** Replace the partial `[[query` with a wiki link node. */
  function pickWikiLink(title: string) {
    if (!editor) return;
    const to = editor.state.selection.from;
    editor.chain().focus()
      .insertContentAt({ from: wikiFrom, to }, [{ type: 'wikiLink', attrs: { target: title } }, { type: 'text', text: ' ' }])
      .run();
    wikiOpen = false;
  }
</script>

<div class="rich" bind:this={wrapEl}>
  <div class="host" bind:this={hostEl}></div>
  {#if isEmpty && !readonly && placeholder}
    <div class="placeholder">{placeholder}</div>
  {/if}
  {#if linkOpen}
    <form class="link-form" style="left: {linkPos.x}px; top: {linkPos.y}px" onsubmit={applyLink}>
      <Icon name="link" size={13} />
      <input
        bind:this={linkInput}
        bind:value={linkHref}
        type="text"
        placeholder="https://"
        onkeydown={(e) => { if (e.key === 'Escape') { e.preventDefault(); closeLinkForm(); } }}
      />
      <button type="submit" class="lf-btn" title={$t('note_tb_link')}><Icon name="check" size={13} /></button>
      {#if hasLink}
        <button type="button" class="lf-btn" title={$t('note_link_remove')} onclick={removeLink}><Icon name="x" size={13} /></button>
      {/if}
    </form>
  {/if}
  {#if wikiOpen}
    <WikiLinkPicker bind:this={wikiPicker} bind:index={wikiIndex} query={wikiQuery} {notes} {excludeId} onpick={pickWikiLink} />
  {/if}
</div>

<style>
  .rich {
    position: relative;
    flex: 1;
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .host {
    flex: 1;
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  .placeholder {
    position: absolute;
    top: var(--sp-3);
    left: var(--sp-5);
    color: var(--text-3);
    font-size: 0.95rem;
    line-height: 1.7;
    pointer-events: none;
  }

  .link-form {
    position: absolute;
    z-index: 6;
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.4rem;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
    color: var(--text-3);
  }
  /* Reset the global input chrome from base.css */
  .link-form input {
    width: 240px;
    height: 24px;
    padding: 0 0.3rem;
    background: none;
    border: none;
    outline: none;
    font-size: var(--fs-sm);
    color: var(--text);
  }
  .link-form input:focus { box-shadow: none; }
  .lf-btn {
    background: none;
    border: none;
    color: var(--text-2);
    cursor: pointer;
    border-radius: 4px;
    display: inline-flex;
    align-items: center;
    padding: 0.15rem 0.3rem;
  }
  .lf-btn:hover { background: var(--surface-2); color: var(--text); }

  /* ── Document styles (shared look with the former read-only preview) ── */
  .host :global(.ProseMirror) {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    outline: none;
    padding: var(--sp-3) var(--sp-5);
    font-size: 0.95rem;
    line-height: 1.7;
    color: var(--text-body);
    word-wrap: break-word;
    white-space: pre-wrap;
  }
  .host :global(h1), .host :global(h2), .host :global(h3),
  .host :global(h4), .host :global(h5), .host :global(h6) {
    color: var(--text);
    margin: 1.2em 0 0.5em;
    line-height: 1.3;
    font-weight: var(--fw-bold);
  }
  .host :global(h1) { font-size: 1.6em; }
  .host :global(h2) { font-size: 1.35em; }
  .host :global(h3) { font-size: 1.15em; }
  .host :global(.ProseMirror > :first-child) { margin-top: 0; }
  .host :global(p) { margin: 0 0 0.8em; }
  .host :global(a) { color: var(--accent); text-decoration: none; cursor: pointer; }
  .host :global(a:hover) { text-decoration: underline; }
  .host :global(a.wiki) {
    padding: 0 0.15em;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .host :global(code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.05em 0.35em;
  }
  .host :global(pre) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-3);
    overflow-x: auto;
    margin: 0 0 1em;
    font-family: var(--font-mono);
  }
  .host :global(pre code) { background: none; border: none; padding: 0; font-size: 0.85em; }
  .host :global(blockquote) {
    margin: 0 0 1em;
    padding: 0.2em 0 0.2em var(--sp-3);
    border-left: 3px solid var(--accent);
    color: var(--text-2);
  }
  .host :global(ul), .host :global(ol) { margin: 0 0 0.8em; padding-left: 1.5em; }
  .host :global(li) { margin: 0.15em 0; }
  .host :global(li > p) { margin: 0; }
  .host :global(ul[data-type="taskList"]) { list-style: none; padding-left: 0.2em; }
  .host :global(ul[data-type="taskList"] > li) { display: flex; align-items: flex-start; gap: 0.5em; }
  .host :global(ul[data-type="taskList"] > li > label) { flex-shrink: 0; margin-top: 0.3em; display: flex; }
  .host :global(ul[data-type="taskList"] > li > div) { flex: 1; min-width: 0; }
  .host :global(ul[data-type="taskList"] input[type="checkbox"]) {
    /* base.css gives every input width:100% + padding; keep the box inline */
    width: auto;
    padding: 0;
    margin: 0;
    cursor: pointer;
    accent-color: var(--accent);
  }
  .host :global(hr) { border: none; border-top: 1px solid var(--border); margin: 1.2em 0; }
  .host :global(img) { max-width: 100%; border-radius: var(--radius-sm); vertical-align: middle; }
  .host :global(img.ProseMirror-selectednode) { outline: 2px solid var(--accent); }
  .host :global(.ProseMirror-selectednode) { outline: 2px solid var(--accent); border-radius: 3px; }
  .host :global(.tableWrapper) { overflow-x: auto; margin: 0 0 1em; }
  .host :global(table) { border-collapse: collapse; font-size: 0.92em; table-layout: fixed; width: 100%; }
  .host :global(th), .host :global(td) {
    border: 1px solid var(--border);
    padding: 0.3em 0.6em;
    text-align: left;
    vertical-align: top;
    position: relative;
  }
  .host :global(th > p), .host :global(td > p) { margin: 0; }
  .host :global(th) { background: var(--surface-2); color: var(--text); }
  .host :global(.selectedCell::after) {
    content: '';
    position: absolute;
    inset: 0;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    pointer-events: none;
  }
</style>
