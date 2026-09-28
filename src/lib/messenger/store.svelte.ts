// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { messengerApi, type MessengerStatus } from './api';

/** Module-level state: whether the module exists in this build and is enabled. */
class MessengerStore {
  status = $state<MessengerStatus | null>(null);
  loaded = $state(false);
  loading = $state(false);
  private _promise: Promise<void> | null = null;

  /** Show the nav entry only when compiled, enabled and the runtime started. */
  get visible(): boolean {
    const s = this.status;
    return !!s && s.compiled && s.enabled && s.runtime !== null;
  }

  get compiled(): boolean {
    return this.status?.compiled ?? false;
  }

  async ensureLoaded() {
    if (this.loaded) return;
    if (this._promise) return this._promise;
    this._promise = this.refresh().finally(() => { this._promise = null; });
    return this._promise;
  }

  async refresh() {
    this.loading = true;
    try {
      this.status = await messengerApi.status();
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  async setEnabled(enabled: boolean) {
    await messengerApi.setEnabled(enabled);
    await this.refresh();
  }
}

export const messengerStore = new MessengerStore();
