// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import {
  messengerApi,
  RELAY_STATUS_EVENT,
  RUNTIME_EVENT,
  type IdentityImportKind,
  type MessengerIdentity,
  type MessengerManifestInfo,
  type MessengerRelay,
  type MessengerStatus,
  type MessengerUiEvent,
} from './api';

export interface FeedEntry extends MessengerUiEvent {
  at: number;
}

const FEED_LIMIT = 50;

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Module-level state: whether the module exists in this build, is enabled, and who we are. */
class MessengerStore {
  status = $state<MessengerStatus | null>(null);
  identity = $state<MessengerIdentity | null>(null);
  relays = $state<MessengerRelay[]>([]);
  manifest = $state<MessengerManifestInfo | null>(null);
  /** Newest first. Runtime events (inbound DMs, ignored events, errors). */
  feed = $state<FeedEntry[]>([]);
  loaded = $state(false);
  loading = $state(false);
  private _promise: Promise<void> | null = null;
  private _unlisten: (() => void)[] = [];

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
      if (this.visible) {
        const [identity, relays, manifest] = await Promise.all([
          messengerApi.identity.get(),
          messengerApi.relays.list(),
          messengerApi.relays.manifestInfo(),
        ]);
        this.identity = identity;
        this.relays = relays;
        this.manifest = manifest;
      } else {
        this.identity = null;
        this.relays = [];
        this.manifest = null;
      }
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  /** Subscribe to relay-state and runtime event pushes. Idempotent. */
  async startListeners() {
    if (this._unlisten.length || !isTauri) return;
    const { listen } = await import('@tauri-apps/api/event');
    this._unlisten.push(
      await listen<MessengerRelay[]>(RELAY_STATUS_EVENT, (e) => {
        this.relays = e.payload;
      }),
      await listen<MessengerUiEvent>(RUNTIME_EVENT, (e) => {
        this.feed = [{ ...e.payload, at: Date.now() }, ...this.feed].slice(0, FEED_LIMIT);
        if (e.payload.name === 'inbound.dm') messengerApi.status().then((s) => (this.status = s)).catch(() => {});
      }),
    );
  }

  clearFeed() {
    this.feed = [];
  }

  sendTextDm(to: string, text: string) {
    return messengerApi.dm.sendText(to, text);
  }

  async refreshRelays() {
    const [relays, manifest] = await Promise.all([messengerApi.relays.list(), messengerApi.relays.manifestInfo()]);
    this.relays = relays;
    this.manifest = manifest;
  }

  async addRelay(url: string, apiKey?: string) {
    await messengerApi.relays.add(url, apiKey);
    await this.refreshRelays();
  }

  async removeRelay(url: string) {
    await messengerApi.relays.remove(url);
    await this.refreshRelays();
  }

  async setRelayEnabled(url: string, enabled: boolean) {
    await messengerApi.relays.setEnabled(url, enabled);
    await this.refreshRelays();
  }

  async setSilent(enabled: boolean) {
    await messengerApi.relays.setSilent(enabled);
    await this.refresh();
  }

  async setRegion(region: string) {
    await messengerApi.relays.setRegion(region);
    await this.refresh();
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
