// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Module registry for the super-app shell. Home grid, hub sheet and the
// default-app setting are all built from this list.

import type { Key } from './i18n';

export interface AppModule {
  id: string;
  title: Key;
  icon: string;
  route: string;
  /** Shown only while this optional module is switched on. */
  optional?: 'messenger';
}

export const APPS: AppModule[] = [
  { id: 'notes', title: 'app_notes', icon: 'file-text', route: '/notes' },
  { id: 'passwords', title: 'app_passwords', icon: 'lock', route: '/passwords' },
  { id: 'totp', title: 'app_totp', icon: 'shield', route: '/totp' },
  { id: 'tools', title: 'app_tools', icon: 'dices', route: '/tools' },
  { id: 'messenger', title: 'nav_messenger', icon: 'message-circle', route: '/messenger', optional: 'messenger' },
  { id: 'settings', title: 'app_settings', icon: 'settings', route: '/settings' },
];

/** Modules to offer: optional ones only when they are available. */
export function availableApps(messenger: boolean): AppModule[] {
  return APPS.filter((a) => a.optional !== 'messenger' || messenger);
}

export function appForPath(pathname: string): AppModule | undefined {
  return APPS.find((a) => pathname === a.route || pathname.startsWith(a.route + '/'));
}

const DEFAULT_APP_KEY = 'm_default_app';

/** Module opened at launch instead of Home; empty means Home. */
export function loadDefaultApp(): string {
  if (typeof localStorage === 'undefined') return '';
  const id = localStorage.getItem(DEFAULT_APP_KEY) ?? '';
  return APPS.some((a) => a.id === id) ? id : '';
}

export function saveDefaultApp(id: string) {
  if (typeof localStorage === 'undefined') return;
  if (id) localStorage.setItem(DEFAULT_APP_KEY, id);
  else localStorage.removeItem(DEFAULT_APP_KEY);
}
