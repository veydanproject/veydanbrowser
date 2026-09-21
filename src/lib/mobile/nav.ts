// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Bottom navigation: the root shell has one bar, the notes module its own.

import { APPS, loadDefaultApp } from './apps';
import type { Key } from './i18n';

export interface NavItem {
  id: string;
  title: Key;
  icon: string;
  /** Route to open; omit when `onclick` handles it. */
  href?: string;
  onclick?: () => void;
}

export const ROOT_NAV: NavItem[] = [
  { id: 'home', title: 'nav_home', icon: 'home', href: '/' },
  { id: 'settings', title: 'app_settings', icon: 'settings', href: '/settings' },
];

/** Home tab opens the default module when one is set. */
export function rootNav(): NavItem[] {
  const id = loadDefaultApp();
  const app = APPS.find((a) => a.id === id && a.id !== 'settings');
  return [
    { id: 'home', title: 'nav_home', icon: 'home', href: app?.route ?? '/' },
    { id: 'settings', title: 'app_settings', icon: 'settings', href: '/settings' },
  ];
}

export function notesNav(onmore: () => void): NavItem[] {
  return [
    { id: 'notes', title: 'app_notes', icon: 'file-text', href: '/notes' },
    { id: 'new', title: 'nav_new', icon: 'file-plus', href: '/notes/new' },
    { id: 'tags', title: 'notes_tags', icon: 'tag', href: '/notes/tags' },
    { id: 'more', title: 'nav_more', icon: 'more-horizontal', onclick: onmore },
  ];
}

/** Which bar a route belongs to; `null` hides the bar (note editor, scanner). */
export function navKind(pathname: string): 'root' | 'notes' | null {
  if (/^\/notes\/(?!tags$)[^/]+/.test(pathname)) return null;
  if (pathname.startsWith('/totp/scan')) return null;
  if (pathname.startsWith('/notes') || pathname.startsWith('/search')) return 'notes';
  return 'root';
}

/** Active item id for the bar. */
export function navActive(pathname: string): string {
  if (pathname === '/') return 'home';
  if (pathname.startsWith('/settings')) return 'settings';
  if (pathname.startsWith('/search')) return 'search';
  if (pathname.startsWith('/notes/tags')) return 'tags';
  if (pathname.startsWith('/notes')) return 'notes';
  return '';
}
