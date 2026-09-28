// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { messengerApi, type IdentityImportKind, type MessengerIdentity, type MessengerStatus } from './api';

/** Module-level state: whether the module exists in this build, is enabled, and who we are. */
class MessengerStore {
  status = $state<MessengerStatus | null>(null);
  identity = $state<MessengerIdentity | null>(null);
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

  get secretsUnlocked(): boolean {
    return this.status?.runtime?.secrets_unlocked ?? false;
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
      this.identity = this.visible ? await messengerApi.identity.get() : null;
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  async setEnabled(enabled: boolean) {
    await messengerApi.setEnabled(enabled);
    await this.refresh();
  }

  async createIdentity(password: string) {
    const created = await messengerApi.identity.create(password);
    this.identity = created.identity;
    await this.refresh();
    return created;
  }

  async importIdentity(kind: IdentityImportKind, secret: string, password?: string) {
    this.identity = await messengerApi.identity.import(kind, secret, password);
    await this.refresh();
    return this.identity;
  }

  exportIdentity(password: string) {
    return messengerApi.identity.export(password);
  }

  async deleteIdentity() {
    await messengerApi.identity.delete();
    this.identity = null;
    await this.refresh();
  }
}

export const messengerStore = new MessengerStore();
