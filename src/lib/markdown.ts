// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { marked } from 'marked';

const ALLOWED_TAGS = new Set([
  'p', 'br', 'hr', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'strong', 'em', 'b', 'i', 'u', 's', 'del', 'code', 'pre', 'blockquote',
  'ul', 'ol', 'li', 'a', 'img', 'input',
  'table', 'thead', 'tbody', 'tr', 'th', 'td', 'sup', 'sub', 'span', 'div',
]);

const ALLOWED_ATTRS: Record<string, Set<string>> = {
  a: new Set(['href', 'title', 'class']),
  img: new Set(['src', 'alt', 'title']),
  input: new Set(['type', 'checked', 'disabled']),
  th: new Set(['align']),
  td: new Set(['align']),
  code: new Set(['class']),
  pre: new Set(['class']),
};

const SAFE_URL = /^(https?:|mailto:|asset:|wiki:|data:image\/(png|jpe?g|gif|webp);|#|\.{0,2}\/|[^:]+$)/i;

/** `wiki:` href prefix produced for `[[links]]`; the preview intercepts clicks on it. */
export const WIKI_HREF = 'wiki:';

const escapeHtml = (s: string) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

// [[Target]] / [[Target|label]] -> <a class="wiki" href="wiki:Target">label</a>
marked.use({
  extensions: [
    {
      name: 'wikilink',
      level: 'inline',
      start: (src: string) => src.indexOf('[['),
      tokenizer(src: string) {
        const m = /^\[\[([^\]\n|]+)(?:\|([^\]\n]+))?\]\]/.exec(src);
        if (!m) return undefined;
        return { type: 'wikilink', raw: m[0], target: m[1].trim(), label: (m[2] ?? m[1]).trim() };
      },
      renderer(token) {
        const t = token as unknown as { target: string; label: string };
        return `<a class="wiki" href="${WIKI_HREF}${encodeURIComponent(t.target)}">${escapeHtml(t.label)}</a>`;
      },
    },
  ],
});

/** Regex matching GFM task list markers; order of matches equals render order. */
export const TASK_RE = /^(\s*(?:[-*+]|\d+\.)\s+\[)([ xX])(\])/gm;

/** Maps a relative link (e.g. `attachments/id/file.png`) to a loadable URL. */
export type ResolveSrc = (rel: string) => string;

const isRelative = (url: string) => !/^([a-z][a-z0-9+.-]*:|\/\/|#|\/)/i.test(url);

function sanitize(html: string, resolveSrc?: ResolveSrc): string {
  const doc = new DOMParser().parseFromString(`<body>${html}</body>`, 'text/html');
  const walk = (node: Element) => {
    for (const el of Array.from(node.children)) {
      const tag = el.tagName.toLowerCase();
      if (!ALLOWED_TAGS.has(tag)) {
        el.replaceWith(...Array.from(el.childNodes));
        continue;
      }
      const allowed = ALLOWED_ATTRS[tag] ?? new Set<string>();
      for (const attr of Array.from(el.attributes)) {
        const name = attr.name.toLowerCase();
        if (!allowed.has(name)) { el.removeAttribute(attr.name); continue; }
        if ((name === 'href' || name === 'src') && !SAFE_URL.test(attr.value.trim())) {
          el.removeAttribute(attr.name);
        }
      }
      if (resolveSrc && tag === 'img') {
        const src = el.getAttribute('src');
        if (src && isRelative(src)) el.setAttribute('src', resolveSrc(src));
      }
      if (tag === 'a') { el.setAttribute('target', '_blank'); el.setAttribute('rel', 'noopener noreferrer'); }
      walk(el);
    }
  };
  walk(doc.body);
  return doc.body.innerHTML;
}

/** Render Markdown to sanitized HTML. Task checkboxes are left enabled for click-to-toggle. */
export function renderMarkdown(md: string, resolveSrc?: ResolveSrc): string {
  const raw = marked.parse(md, { gfm: true, breaks: true, async: false }) as string;
  return sanitize(raw, resolveSrc).replaceAll('<input disabled="" type="checkbox"', '<input type="checkbox"')
    .replaceAll('<input checked="" disabled="" type="checkbox"', '<input type="checkbox" checked=""');
}

/** Toggle the n-th task checkbox in source (render order). */
export function toggleTask(md: string, index: number): string {
  let i = 0;
  return md.replace(TASK_RE, (_m, pre: string, mark: string, post: string) => {
    const out = i === index ? `${pre}${mark === ' ' ? 'x' : ' '}${post}` : `${pre}${mark}${post}`;
    i++;
    return out;
  });
}

export function wordCount(text: string): number {
  const t = text.trim();
  return t ? t.split(/\s+/).length : 0;
}
