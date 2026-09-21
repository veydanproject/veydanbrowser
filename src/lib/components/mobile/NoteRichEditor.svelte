<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- WYSIWYG editor over the desktop tiptap extensions; Markdown stays the storage format. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { Editor } from '@tiptap/core';
  import { noteExtensions, WIKI_MARK, unclosedWikiAt, type ResolveSrc } from '$lib/tiptap-ext';

  interface Props {
    content: string;
    resolveSrc: ResolveSrc;
    placeholder: string;
    onchange: (md: string) => void;
    /** Text typed after an unclosed `@@`, or null when no wiki link is being typed. */
    onwiki: (query: string | null) => void;
    onwikilink: (target: string) => void;
  }

  let { content, resolveSrc, placeholder, onchange, onwiki, onwikilink }: Props = $props();

  let hostEl: HTMLElement | null = $state(null);
  let editor: Editor | null = null;
  let lastEmitted = '';
  let isEmpty = $state(false);
  let wikiFrom = 0;

  onMount(() => {
    editor = new Editor({
      element: hostEl!,
      extensions: noteExtensions(resolveSrc),
      content,
      contentType: 'markdown',
      editorProps: {
        attributes: { spellcheck: 'false' },
        handleClick: (_view, _pos, e) => onClick(e),
      },
      onUpdate: ({ editor: ed }) => {
        const md = ed.getMarkdown();
        isEmpty = ed.isEmpty;
        updateWikiState();
        if (md === lastEmitted) return;
        lastEmitted = md;
        onchange(md);
      },
      onSelectionUpdate: updateWikiState,
    });
    lastEmitted = content;
    isEmpty = editor.isEmpty;
    return () => editor?.destroy();
  });

  // Content replaced outside the editor (history restore, remote edit).
  $effect(() => {
    const md = content;
    if (!editor || md === lastEmitted) return;
    lastEmitted = md;
    editor.chain().setMeta('addToHistory', false).setContent(md, { contentType: 'markdown', emitUpdate: false }).run();
    isEmpty = editor.isEmpty;
  });

  function onClick(e: MouseEvent): boolean {
    const wiki = (e.target as HTMLElement).closest('a.wiki');
    if (!wiki) return false;
    onwikilink(wiki.getAttribute('data-target') ?? '');
    return true;
  }

  /** Track an unclosed `@@` before the caret inside the current text block. */
  function updateWikiState() {
    if (!editor) return;
    const { $from: caret, empty } = editor.state.selection;
    if (!empty || !caret.parent.isTextblock) { onwiki(null); return; }
    const before = caret.parent.textBetween(0, caret.parentOffset, undefined, '\ufffc');
    const open = unclosedWikiAt(before);
    if (open < 0) { onwiki(null); return; }
    wikiFrom = caret.pos - (before.length - open);
    onwiki(before.slice(open + WIKI_MARK.length));
  }

  /** Replace the partial `@@query` with a wiki link node. */
  export function pickWikiLink(title: string) {
    if (!editor) return;
    const to = editor.state.selection.from;
    editor.chain().focus()
      .insertContentAt({ from: wikiFrom, to }, [{ type: 'wikiLink', attrs: { target: title } }, { type: 'text', text: ' ' }])
      .run();
    onwiki(null);
  }

  export function insertMarkdown(md: string) {
    editor?.chain().focus().insertContent(md, { contentType: 'markdown' }).run();
  }

  /** Plain text at the caret, e.g. `@@` to start a wiki link. */
  export function insertText(text: string) {
    editor?.chain().focus().insertContent(text).run();
  }
</script>

<div class="rich">
  <div class="host" bind:this={hostEl}></div>
  {#if isEmpty && placeholder}
    <div class="placeholder">{placeholder}</div>
  {/if}
</div>

<style>
  .rich {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .host { flex: 1; display: flex; min-width: 0; min-height: 0; }
  .placeholder {
    position: absolute;
    top: var(--sp-2);
    left: 0;
    color: var(--text-3);
    pointer-events: none;
  }

  .host :global(.ProseMirror) {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    outline: none;
    padding: var(--sp-2) 0;
    font-size: 15px;
    line-height: 1.6;
    word-wrap: break-word;
    white-space: pre-wrap;
    user-select: text;
    -webkit-user-select: text;
  }
  .host :global(h1), .host :global(h2), .host :global(h3) { margin: 1em 0 0.4em; line-height: 1.3; }
  .host :global(h1) { font-size: 1.5em; }
  .host :global(h2) { font-size: 1.3em; }
  .host :global(h3) { font-size: 1.15em; }
  .host :global(.ProseMirror > :first-child) { margin-top: 0; }
  .host :global(p) { margin: 0 0 0.7em; }
  .host :global(a) { color: var(--accent-text); text-decoration: none; }
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
    overflow-x: auto;
    margin: 0 0 1em;
    padding: var(--sp-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md, 8px);
    font-family: var(--font-mono);
  }
  .host :global(pre code) { background: none; border: none; padding: 0; font-size: 13px; }
  .host :global(blockquote) {
    margin: 0 0 1em;
    padding-left: var(--sp-3);
    border-left: 3px solid var(--border-2);
    color: var(--text-2);
  }
  .host :global(ul), .host :global(ol) { margin: 0 0 0.7em; padding-left: 1.4em; }
  .host :global(li > p) { margin: 0; }
  .host :global(ul[data-type="taskList"]) { list-style: none; padding-left: 0; }
  .host :global(ul[data-type="taskList"] > li) { display: flex; align-items: flex-start; gap: 0.5em; }
  .host :global(ul[data-type="taskList"] > li > label) { flex-shrink: 0; margin-top: 0.2em; display: flex; }
  .host :global(ul[data-type="taskList"] > li > div) { flex: 1; min-width: 0; }
  .host :global(ul[data-type="taskList"] input[type="checkbox"]) {
    width: 20px;
    height: 20px;
    min-height: 0;
    padding: 0;
    margin: 0;
    accent-color: var(--accent);
  }
  .host :global(hr) { border: none; border-top: 1px solid var(--border); margin: 1em 0; }
  .host :global(img) { max-width: 100%; height: auto; border-radius: var(--radius-md, 8px); vertical-align: middle; }
  .host :global([data-resize-container]), .host :global([data-resize-wrapper]) { max-width: 100%; }
  .host :global([data-resize-handle]) { display: none; }
  .host :global(.ProseMirror-selectednode) { outline: 2px solid var(--accent); border-radius: 3px; }
  .host :global(.tableWrapper) { overflow-x: auto; margin: 0 0 1em; }
  .host :global(table) { border-collapse: collapse; font-size: 0.92em; }
  .host :global(th), .host :global(td) { border: 1px solid var(--border); padding: 4px 8px; vertical-align: top; }
  .host :global(th > p), .host :global(td > p) { margin: 0; }
  .host :global(th) { background: var(--surface-2); }
</style>
