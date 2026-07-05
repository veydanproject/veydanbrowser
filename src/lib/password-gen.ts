// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { writable } from 'svelte/store';

export interface PwSettings {
  length: number;
  uppercase: boolean;
  lowercase: boolean;
  numbers: boolean;
  symbols: boolean;
  excludeSimilar: boolean;
  historyEnabled: boolean;
  historyLimit: number | null;
}

export interface PwEntry {
  id: string;
  password: string;
  created_at: string;
}

const SETTINGS_KEY = 'rb_pwgen_settings';

const defaults: PwSettings = {
  length: 16,
  uppercase: true,
  lowercase: true,
  numbers: true,
  symbols: true,
  excludeSimilar: false,
  historyEnabled: false,
  historyLimit: null,
};

function loadSettings(): PwSettings {
  if (typeof localStorage === 'undefined') return { ...defaults };
  try {
    const saved = localStorage.getItem(SETTINGS_KEY);
    if (saved) return { ...defaults, ...JSON.parse(saved) };
  } catch {}
  return { ...defaults };
}

export const pwSettings = writable<PwSettings>(loadSettings());

pwSettings.subscribe((val) => {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(val));
  }
});

const SIMILAR = new Set(['0', 'O', '1', 'l', 'I']);

export function generatePassword(s: PwSettings): string {
  let chars = '';

  if (s.uppercase) {
    for (const c of 'ABCDEFGHIJKLMNOPQRSTUVWXYZ') {
      if (!s.excludeSimilar || !SIMILAR.has(c)) chars += c;
    }
  }
  if (s.lowercase) {
    for (const c of 'abcdefghijklmnopqrstuvwxyz') {
      if (!s.excludeSimilar || !SIMILAR.has(c)) chars += c;
    }
  }
  if (s.numbers) {
    for (const c of '0123456789') {
      if (!s.excludeSimilar || !SIMILAR.has(c)) chars += c;
    }
  }
  if (s.symbols) {
    chars += '!@#$%^&*()-_=+[]{}|;:,.<>?';
  }

  if (chars.length === 0) {
    throw new Error('pwgen_error_no_charset');
  }

  const buf = new Uint32Array(s.length);
  crypto.getRandomValues(buf);
  return Array.from(buf, (n) => chars[n % chars.length]).join('');
}
