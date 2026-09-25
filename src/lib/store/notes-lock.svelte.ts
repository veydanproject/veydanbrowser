// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { NoteLockStatus } from '$lib/types';

const TOUCH_INTERVAL_MS = 20_000;

/** Lock state shared by every notes surface; the backend is the source of truth. */
class NotesLockStore {
  status = $state<NoteLockStatus>({ enabled: false, locked: false, timeout_min: 5, vault: 'none' });
  ready = $state(false);

  private _lastTouch = 0;
  private _unlisten: (() => void) | null = null;
  /** Bumped on unlock/lock so an older status read cannot overwrite it. */
  private _epoch = 0;

  get locked() {
    return this.status.enabled && this.status.locked;
  }

  async refresh() {
    const epoch = this._epoch;
    try {
      const status = await api.notes.lockStatus();
      if (epoch !== this._epoch) return;
      this.status = status;
    } catch {}
    this.ready = true;
  }

  async unlock(password: string) {
    const epoch = ++this._epoch;
    const status = await api.notes.lockUnlock(password);
    if (epoch !== this._epoch) return;
    this.status = status;
  }

  async lock() {
    const epoch = ++this._epoch;
    const status = await api.notes.lockNow();
    if (epoch !== this._epoch) return;
    this.status = status;
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

  /** Subscribe to lock events and user activity from any window; idempotent. */
  async listen() {
    if (this._unlisten || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
    const touch = () => this.touch();
    window.addEventListener('keydown', touch, true);
    window.addEventListener('pointerdown', touch, true);
    this._unlisten = () => {
      window.removeEventListener('keydown', touch, true);
      window.removeEventListener('pointerdown', touch, true);
    };
    const { listen } = await import('@tauri-apps/api/event');
    const a = await listen('notes://locked', () => void this.refresh());
    const b = await listen('notes://unlocked', () => void this.refresh());
    const c = await listen('passwords://vault-changed', () => void this.refresh());
    const dom = this._unlisten;
    this._unlisten = () => { dom(); a(); b(); c(); };
  }
}

export const notesLock = new NotesLockStore();
