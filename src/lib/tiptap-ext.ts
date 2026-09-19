// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/** Tiptap extensions for the notes WYSIWYG editor (Markdown stays the storage format). */

import { Node, mergeAttributes, nodeInputRule, type Extensions, type JSONContent } from '@tiptap/core';
import { StarterKit } from '@tiptap/starter-kit';
import { Markdown } from '@tiptap/markdown';
import { HardBreak } from '@tiptap/extension-hard-break';
import { Paragraph } from '@tiptap/extension-paragraph';
import { Image, type ImageOptions } from '@tiptap/extension-image';
import { TaskList } from '@tiptap/extension-task-list';
import { TaskItem } from '@tiptap/extension-task-item';
import { TableKit } from '@tiptap/extension-table';

/** `[[Target]]` / `[[Target|label]]` at the start of a string. */
export const WIKI_RE = /^\[\[([^\]\n|]+)(?:\|([^\]\n]+))?\]\]/;

const wikiAttrs = (m: RegExpMatchArray) => ({ target: m[1].trim(), label: m[2]?.trim() ?? null });

/** Inline atom for wiki links; round-trips as `[[...]]` in Markdown. */
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
    };
  },

  parseHTML() {
    return [{ tag: 'a[data-target]' }];
  },

  renderHTML({ node, HTMLAttributes }) {
    return ['a', mergeAttributes(HTMLAttributes, { class: 'wiki' }), node.attrs.label ?? node.attrs.target];
  },

  renderText({ node }) {
    return `[[${node.attrs.target}]]`;
  },

  markdownTokenizer: {
    name: 'wikiLink',
    level: 'inline',
    start: (src) => src.indexOf('[['),
    tokenize(src) {
      const m = WIKI_RE.exec(src);
      if (m) return { type: 'wikiLink', raw: m[0], ...wikiAttrs(m) };
    },
  },

  parseMarkdown: (token, h) => h.createNode('wikiLink', { target: token.target, label: token.label }),

  renderMarkdown: (node: JSONContent) => {
    const { target, label } = node.attrs ?? {};
    return label && label !== target ? `[[${target}|${label}]]` : `[[${target}]]`;
  },

  // Typing `]]` after `[[Title` converts the text into a wiki link node
  addInputRules() {
    return [
      nodeInputRule({
        find: /\[\[([^\]\n|]+)(?:\|([^\]\n]+))?\]\]$/,
        type: this.type,
        getAttributes: wikiAttrs,
      }),
    ];
  },
});

export type ResolveSrc = (src: string) => string;

/** Inline image whose `src` attr stays a relative attachment path; display URL is resolved at render. */
export const NoteImage = Image.extend<ImageOptions & { resolveSrc: ResolveSrc }>({
  addOptions() {
    const base = this.parent?.() as ImageOptions;
    return { ...base, inline: true, resolveSrc: (s: string) => s };
  },

  renderHTML({ HTMLAttributes }) {
    const src = this.options.resolveSrc(HTMLAttributes.src ?? '');
    return ['img', mergeAttributes(this.options.HTMLAttributes, HTMLAttributes, { src })];
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
