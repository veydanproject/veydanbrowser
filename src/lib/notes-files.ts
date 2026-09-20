// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/** Local file paths carried by paste/drop as file:// URIs or absolute paths.
 *  WebKitGTK/WKWebView expose copied or dropped OS files this way rather than
 *  through DataTransfer.files. */
export function localFilePaths(dt: DataTransfer | null): string[] {
  if (!dt) return [];
  const out: string[] = [];
  const push = (raw: string) => {
    const s = raw.trim();
    if (!s || s.startsWith('#')) return;
    if (s.startsWith('file://')) {
      try { out.push(decodeURIComponent(new URL(s).pathname)); } catch { /* skip */ }
    } else if (/^(\/|[a-zA-Z]:[\\/])/.test(s)) {
      out.push(s);
    }
  };
  dt.getData('text/uri-list').split(/\r?\n/).forEach(push);
  if (out.length === 0) {
    const text = dt.getData('text/plain');
    if (text && !/\r?\n/.test(text.trim())) push(text);
  }
  return out;
}
