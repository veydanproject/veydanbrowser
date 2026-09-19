// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { NoteLockStatus } from '$lib/types';

const TOUCH_INTERVAL_MS = 20_000;

/** Lock state shared by every notes surface; the backend is the source of truth. */
class NotesLockStore {
  status = $state<NoteLockStatus>({ enabled: false, locked: false, timeout_min: 5 });
  ready = $state(false);

  private _lastTouch = 0;
  private _unlisten: (() => void) | null = null;

  get locked() {
    return this.status.enabled && this.status.locked;
  }

  async refresh() {
    try {
      this.status = await api.notes.lockStatus();
    } catch {}
    this.ready = true;
  }

  async unlock(password: string) {
    this.status = await api.notes.lockUnlock(password);
  }

  async lock() {
    this.status = await api.notes.lockNow();
  }

  async setPassword(password: string | null, current?: string) {
    this.status = await api.notes.lockSet(password, current);
  }

  async setTimeout(minutes: number) {
    this.status = await api.notes.lockTimeoutSet(minutes);
  }

  /** Throttled activity ping so the inactivity timer restarts. */
  touch() {
    if (!this.status.enabled || this.status.locked) return;
    const now = Date.now();
    if (now - this._lastTouch < TOUCH_INTERVAL_MS) return;
    this._lastTouch = now;
    void api.notes.lockTouch();
  }

  /** Subscribe to lock events from any window; idempotent. */
  async listen() {
    if (this._unlisten || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    const { listen } = await import('@tauri-apps/api/event');
    const a = await listen('notes://locked', () => void this.refresh());
    const b = await listen('notes://unlocked', () => void this.refresh());
    this._unlisten = () => { a(); b(); };
  }
}

export const notesLock = new NotesLockStore();
