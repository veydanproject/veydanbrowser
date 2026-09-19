<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { renderMarkdown, toggleTask, WIKI_HREF } from '$lib/markdown';
  import { api } from '$lib/api';
  import { convertFileSrc } from '@tauri-apps/api/core';

  interface Props {
    content: string;
    /** Absolute dir the relative attachment links resolve against */
    baseDir?: string;
    readonly?: boolean;
    onchange?: (content: string) => void;
    /** `[[Target]]` clicked; target is the raw link text */
    onwikilink?: (target: string) => void;
  }

  let { content, baseDir, readonly = false, onchange, onwikilink }: Props = $props();

  const html = $derived(
    renderMarkdown(content, baseDir ? (rel) => convertFileSrc(`${baseDir}/${decodeURIComponent(rel)}`) : undefined)
  );

  function onClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    const anchor = target.closest('a');
    if (anchor) {
      e.preventDefault();
      const href = anchor.getAttribute('href') ?? '';
      if (href.startsWith(WIKI_HREF)) onwikilink?.(decodeURIComponent(href.slice(WIKI_HREF.length)));
      else if (/^https?:/i.test(href)) void api.system.openUrl(href);
      return;
    }
    if (target instanceof HTMLInputElement && target.type === 'checkbox') {
      e.preventDefault();
      if (readonly || !onchange) return;
      const root = target.closest('.md-preview');
      if (!root) return;
      const boxes = Array.from(root.querySelectorAll('input[type="checkbox"]'));
      onchange(toggleTask(content, boxes.indexOf(target)));
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="md-preview" role="presentation" onclick={onClick}>
  {@html html}
</div>

<style>
  .md-preview {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-3) var(--sp-5);
    font-size: 0.95rem;
    line-height: 1.7;
    color: var(--text-body);
    word-wrap: break-word;
  }
  .md-preview :global(h1), .md-preview :global(h2), .md-preview :global(h3),
  .md-preview :global(h4), .md-preview :global(h5), .md-preview :global(h6) {
    color: var(--text);
    margin: 1.2em 0 0.5em;
    line-height: 1.3;
    font-weight: var(--fw-bold);
  }
  .md-preview :global(h1) { font-size: 1.6em; }
  .md-preview :global(h2) { font-size: 1.35em; }
  .md-preview :global(h3) { font-size: 1.15em; }
  .md-preview :global(h1:first-child), .md-preview :global(h2:first-child), .md-preview :global(p:first-child) { margin-top: 0; }
  .md-preview :global(p) { margin: 0 0 0.8em; }
  .md-preview :global(a) { color: var(--accent); text-decoration: none; }
  .md-preview :global(a:hover) { text-decoration: underline; }
  .md-preview :global(a.wiki) {
    padding: 0 0.15em;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .md-preview :global(code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.05em 0.35em;
  }
  .md-preview :global(pre) {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-3);
    overflow-x: auto;
    margin: 0 0 1em;
  }
  .md-preview :global(pre code) { background: none; border: none; padding: 0; font-size: 0.85em; }
  .md-preview :global(blockquote) {
    margin: 0 0 1em;
    padding: 0.2em 0 0.2em var(--sp-3);
    border-left: 3px solid var(--accent);
    color: var(--text-2);
  }
  .md-preview :global(ul), .md-preview :global(ol) { margin: 0 0 0.8em; padding-left: 1.5em; }
  .md-preview :global(li) { margin: 0.15em 0; }
  .md-preview :global(li > input[type="checkbox"]) {
    /* base.css gives every input width:100% + padding; keep the box inline */
    width: auto;
    padding: 0;
    margin: 0 0.5em 0 -1.4em;
    cursor: pointer;
    accent-color: var(--accent);
    vertical-align: middle;
  }
  .md-preview :global(li:has(> input[type="checkbox"])) { list-style: none; }
  .md-preview :global(hr) { border: none; border-top: 1px solid var(--border); margin: 1.2em 0; }
  .md-preview :global(img) { max-width: 100%; border-radius: var(--radius-sm); }
  .md-preview :global(table) { border-collapse: collapse; margin: 0 0 1em; font-size: 0.92em; }
  .md-preview :global(th), .md-preview :global(td) {
    border: 1px solid var(--border);
    padding: 0.3em 0.6em;
    text-align: left;
  }
  .md-preview :global(th) { background: var(--surface-2); color: var(--text); }
</style>
