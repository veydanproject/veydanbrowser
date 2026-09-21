// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Compile-time platform facts. `__TAURI_PLATFORM__` is injected by vite.config.js
// from TAURI_ENV_PLATFORM; UI code asks for capabilities, not the OS.

export type Platform = 'linux' | 'windows' | 'darwin' | 'android' | 'ios' | 'unknown';

const KNOWN: Platform[] = ['linux', 'windows', 'darwin', 'android', 'ios'];

function detect(): Platform {
  const raw = typeof __TAURI_PLATFORM__ === 'string' ? __TAURI_PLATFORM__ : 'unknown';
  return (KNOWN as string[]).includes(raw) ? (raw as Platform) : 'unknown';
}

export const platform: Platform = detect();

export const isMobile = platform === 'android' || platform === 'ios';
/** Plain `vite dev` in a browser counts as desktop. */
export const isDesktop = !isMobile;

export const capabilities = {
  hasBrowserProfiles: isDesktop,
  hasWindowManagement: isDesktop,
  hasTray: isDesktop,
  hasUpdater: isDesktop,
  hasSsh: isDesktop,
  hasFileBrowser: isDesktop,
  /** Tauri IPC accepts raw byte bodies; mobile sends base64 instead. */
  hasRawIpc: isDesktop,
  hasBarcodeScanner: isMobile,
} as const;

/** Route prefixes that exist only in the desktop UI. */
export const DESKTOP_ONLY_ROUTES = ['/workspace', '/proxies', '/terminal', '/files', '/notes/quick'];

/** Route prefixes that exist only in the mobile UI. */
export const MOBILE_ONLY_ROUTES = ['/search', '/tools', '/totp', '/notes/tags', '/settings/sync'];

function startsWithAny(pathname: string, prefixes: string[]): boolean {
  return prefixes.some((p) => pathname === p || pathname.startsWith(`${p}/`));
}

/** Whether a route belongs to this platform's UI. */
export function routeAllowed(pathname: string): boolean {
  if (isMobile) {
    if (startsWithAny(pathname, DESKTOP_ONLY_ROUTES)) return false;
    return true;
  }
  // Desktop: mobile-only pages and the mobile note editor `/notes/<id>` are off.
  if (startsWithAny(pathname, MOBILE_ONLY_ROUTES)) return false;
  if (/^\/notes\/(?!quick$)[^/]+$/.test(pathname)) return false;
  return true;
}
