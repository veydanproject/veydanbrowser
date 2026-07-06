// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { writable } from 'svelte/store';

export type Theme = 'dark' | 'light';

function loadTheme(): Theme {
  if (typeof localStorage !== 'undefined') {
    const saved = localStorage.getItem('rb_theme');
    // Light theme is disabled while the redesign is dark-only: migrate any
    // persisted 'light' back to 'dark' so users don't land on the stale palette.
    if (saved === 'dark') return saved;
  }
  return 'dark';
}

export const theme = writable<Theme>(loadTheme());

theme.subscribe((val) => {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('rb_theme', val);
  }
  if (typeof document !== 'undefined') {
    document.body.dataset.theme = val;
  }
});

export function toggleTheme() {
  theme.update((t) => (t === 'dark' ? 'light' : 'dark'));
}
