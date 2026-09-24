// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/** Tiptap extensions for the notes WYSIWYG editor (Markdown stays the storage format). */

import { Node, mergeAttributes, nodeInputRule, type Extensions, type JSONContent } from '@tiptap/core';
import { StarterKit } from '@tiptap/starter-kit';
import { Markdown } from '@tiptap/markdown';
import { markdownDestination } from '$lib/markdown';
import { isEntityBinding } from '$lib/bindings';
import { HardBreak } from '@tiptap/extension-hard-break';
import { Paragraph } from '@tiptap/extension-paragraph';
import { Image, type ImageOptions } from '@tiptap/extension-image';
import { TaskList } from '@tiptap/extension-task-list';
import { TaskItem } from '@tiptap/extension-task-item';
import { TableKit } from '@tiptap/extension-table';

/** Standard wiki link delimiters; new links are always written this way. */
export const WIKI_OPEN = '[[';
export const WIKI_CLOSE = ']]';
/** Legacy delimiter (open and close), still parsed. */
export const WIKI_LEGACY = '@@';

/** `[[Target]]` / `[[Target|alias]]` or legacy `@@Target@@` at the start of a string. */
export const WIKI_RE = /^(?:\[\[([^|\]\n]+?)(?:\|([^|\]\n]+?))?\]\]|@@([^|\n]+?)(?:\|([^|\n]+?))?@@)/;

/** Same as `WIKI_RE` but anchored at the end, for the input rule. */
const WIKI_END_RE = /(?:\[\[([^|\]\n]+?)(?:\|([^|\]\n]+?))?\]\]|@@([^|\n]+?)(?:\|([^|\n]+?))?@@)$/;

/** Node attrs from a `WIKI_RE` / `WIKI_END_RE` match; `legacy` keeps the `@@` form on save. */
const wikiAttrs = (m: RegExpMatchArray) => {
  const legacy = m[1] == null;
  const target = (legacy ? m[3] : m[1]).trim();
  const label = (legacy ? m[4] : m[2])?.trim() ?? null;
  return { target, label, legacy };
};

/** Template placeholder delimiters `{{name}}`. */
export const TPL_OPEN = '{{';
export const TPL_CLOSE = '}}';

/** Index of an unclosed `open` (no `close` after it, same line) before the caret, or -1. */
export function unclosedMarkAt(before: string, open: string, close: string): number {
  const at = before.lastIndexOf(open);
  if (at < 0) return -1;
  if (before.indexOf(close, at + open.length) >= 0) return -1;
  if (before.includes('\n', at)) return -1;
  return at;
}

/** Position and delimiter of an unclosed wiki link opener before the caret, or null. */
export function unclosedWikiAt(before: string): { start: number; mark: string } | null {
  const candidates = [
    { mark: WIKI_OPEN, close: WIKI_CLOSE },
    { mark: WIKI_LEGACY, close: WIKI_LEGACY },
  ];
  let best: { start: number; mark: string } | null = null;
  for (const c of candidates) {
    const open = unclosedMarkAt(before, c.mark, c.close);
    if (open < 0) continue;
    if (!best || open > best.start) best = { start: open, mark: c.mark };
  }
  return best;
}

/** Distinct link targets (text before `|`) in a Markdown body, both syntaxes. */
export function extractWikiTargets(text: string): string[] {
  const re = /\[\[([^|\]\n]+?)(?:\|[^\]\n]*?)?\]\]|@@([^|\n]+?)(?:\|[^\n]*?)?@@/g;
  const out: string[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(re)) {
    const target = (m[1] ?? m[2] ?? '').trim();
    const key = target.toLowerCase();
    if (target && !seen.has(key)) {
      seen.add(key);
      out.push(target);
    }
  }
  return out;
}

/** Closing delimiter for an opening one. */
export function wikiCloseOf(mark: string): string {
  return mark === WIKI_LEGACY ? WIKI_LEGACY : WIKI_CLOSE;
}

export function wikiMarkup(target: string, label?: string | null, legacy = false): string {
  const open = legacy ? WIKI_LEGACY : WIKI_OPEN;
  const close = legacy ? WIKI_LEGACY : WIKI_CLOSE;
  const inner = label && label !== target ? `${target}|${label}` : target;
  return `${open}${inner}${close}`;
}

/** Inline atom for wiki links; round-trips as `[[...]]` (or the original `@@...@@`) in Markdown. */
export const WikiLink = Node.create({
  name: 'wikiLink',
  group: 'inline',
  inline: true,
  atom: true,

  addAttributes() {
    return {
      target: {
        default: '',
        parseHTML: (el) => el.getAttribute('data-target'),
        renderHTML: (attrs) => ({ 'data-target': attrs.target }),
      },
      label: {
        default: null,
        parseHTML: (el) => el.textContent,
        renderHTML: () => ({}),
      },
      legacy: {
        default: false,
        parseHTML: (el) => el.hasAttribute('data-legacy'),
        renderHTML: (attrs) => (attrs.legacy ? { 'data-legacy': '' } : {}),
      },
    };
  },

  parseHTML() {
    return [{ tag: 'a[data-target]' }];
  },

  renderHTML({ node, HTMLAttributes }) {
    // `kind:id` targets are Veydan entity mentions; styled as chips by the editor
    const entity = isEntityBinding(String(node.attrs.target ?? ''));
    return ['a', mergeAttributes(HTMLAttributes, { class: entity ? 'wiki wiki-entity' : 'wiki' }), node.attrs.label ?? node.attrs.target];
  },

  renderText({ node }) {
    return wikiMarkup(node.attrs.target, null, node.attrs.legacy);
  },

  markdownTokenizer: {
    name: 'wikiLink',
    level: 'inline',
    start: (src) => {
      const a = src.indexOf(WIKI_OPEN);
      const b = src.indexOf(WIKI_LEGACY);
      if (a < 0) return b;
      if (b < 0) return a;
      return Math.min(a, b);
    },
    tokenize(src) {
      const m = WIKI_RE.exec(src);
      if (m) return { type: 'wikiLink', raw: m[0], ...wikiAttrs(m) };
    },
  },

  parseMarkdown: (token, h) =>
    h.createNode('wikiLink', { target: token.target, label: token.label, legacy: token.legacy }),

  renderMarkdown: (node: JSONContent) => {
    const { target, label, legacy } = node.attrs ?? {};
    return wikiMarkup(String(target ?? ''), label == null ? null : String(label), !!legacy);
  },

  // Typing the closing `]]` after `[[Title` converts the text into a wiki link node
  addInputRules() {
    return [
      nodeInputRule({
        find: WIKI_END_RE,
        type: this.type,
        getAttributes: wikiAttrs,
      }),
    ];
  },
});

export type ResolveSrc = (src: string) => string;

function escAttr(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

/** Inline image whose `src` attr stays a relative attachment path; display URL is resolved at render. */
export const NoteImage = Image.extend<ImageOptions & { resolveSrc: ResolveSrc }>({
  addOptions() {
    const base = this.parent?.() as ImageOptions;
    return {
      ...base,
      inline: true,
      resolveSrc: (s: string) => s,
      resize: {
        enabled: true,
        directions: ['top-left', 'top-right', 'bottom-left', 'bottom-right'],
        minWidth: 40,
        minHeight: 40,
        alwaysPreserveAspectRatio: true,
      },
    };
  },

  addAttributes() {
    const parent = this.parent?.() ?? {};
    return {
      ...parent,
      src: {
        default: null,
        renderHTML: (attrs: { src?: string | null }) => {
          if (!attrs.src) return {};
          return { src: this.options.resolveSrc(attrs.src) };
        },
      },
    };
  },

  renderHTML({ HTMLAttributes }) {
    const src = this.options.resolveSrc(HTMLAttributes.src ?? '');
    return ['img', mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { src })];
  },

  renderMarkdown: (node: JSONContent) => {
    const src = String(node.attrs?.src ?? '');
    const alt = String(node.attrs?.alt ?? '');
    const title = String(node.attrs?.title ?? '');
    const width = Number(node.attrs?.width);
    if (width > 0) {
      return `<img src="${escAttr(src)}" alt="${escAttr(alt)}" width="${Math.round(width)}">`;
    }
    const dest = markdownDestination(src);
    return title ? `![${alt}](${dest} "${title}")` : `![${alt}](${dest})`;
  },
});

/** Single newline in Markdown (`breaks: true`) instead of the two-space hard break. */
const SoftBreak = HardBreak.extend({
  renderMarkdown: () => '\n',
});

const EMPTY_PARAGRAPH = new Set(['&nbsp;', '\u00a0']);

/** Paragraph that keeps images inline (stock version lifts a lone image to block level). */
const NoteParagraph = Paragraph.extend({
  parseMarkdown: (token, h) => {
    const content = h.parseInline(token.tokens ?? []);
    const blank = content.length === 1 && content[0].type === 'text' && EMPTY_PARAGRAPH.has(content[0].text ?? '');
    return h.createNode('paragraph', undefined, blank ? [] : content);
  },
});

export function noteExtensions(resolveSrc: ResolveSrc): Extensions {
  return [
    StarterKit.configure({ hardBreak: false, paragraph: false, link: { openOnClick: false } }),
    SoftBreak,
    NoteParagraph,
    NoteImage.configure({ resolveSrc }),
    TaskList,
    TaskItem.configure({ nested: true }),
    TableKit,
    WikiLink,
    Markdown.configure({ markedOptions: { gfm: true, breaks: true } }),
  ];
}
