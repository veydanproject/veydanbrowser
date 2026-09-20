// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { SyncStatus } from '$lib/api';
import { formatError } from '$lib/utils';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Vault sync status shared by the Notes sync button and Settings. */
class SyncStore {
  status = $state<SyncStatus | null>(null);
  busy = $state(false);
  error = $state('');
  private _unlisten: (() => void) | null = null;
  private _listeners = 0;

  async refresh() {
    if (!isTauri) return;
    try {
      this.status = await api.sync.status();
    } catch {}
  }

  /** Subscribe to backend status events; returns an unsubscribe fn. */
  async listen(): Promise<() => void> {
    if (!isTauri) return () => {};
    this._listeners++;
    void this.refresh();
    if (!this._unlisten) {
      const { listen } = await import('@tauri-apps/api/event');
      this._unlisten = await listen('sync://status', () => void this.refresh());
    }
    return () => {
      this._listeners--;
      if (this._listeners <= 0 && this._unlisten) {
        this._unlisten();
        this._unlisten = null;
      }
    };
  }

  /** Full vault cycle now; falls back to the local file reindex when no vault is joined. */
  async runNow() {
    if (this.busy) return;
    this.busy = true;
    this.error = '';
    try {
      if (this.status?.joined) {
        this.status = await api.sync.runNow();
      } else {
        await api.notes.sync();
      }
    } catch (e) {
      this.error = formatError(e);
    } finally {
      this.busy = false;
      await this.refresh();
    }
  }
}

export const syncStore = new SyncStore();
