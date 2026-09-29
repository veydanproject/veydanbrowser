// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Splits message text into plain parts and http(s) links. The result is
// rendered as text nodes and anchors; nothing is ever inserted as HTML.

export interface TextPart {
  text: string;
  href: string | null;
}

const URL_RE = /https?:\/\/[^\s<>"'`]+/g;

export function linkify(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let last = 0;
  for (const m of text.matchAll(URL_RE)) {
    let url = m[0];
    // Trailing punctuation belongs to the sentence, not to the link.
    const trimmed = url.replace(/[.,;:!?)\]}»]+$/, '');
    const start = m.index ?? 0;
    if (start > last) parts.push({ text: text.slice(last, start), href: null });
    url = trimmed;
    parts.push({ text: url, href: url });
    last = start + url.length;
  }
  if (last < text.length) parts.push({ text: text.slice(last), href: null });
  return parts;
}
