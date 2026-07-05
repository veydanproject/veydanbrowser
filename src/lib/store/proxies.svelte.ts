// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { Proxy, ProxyCheckResult } from '$lib/types';

class ProxiesStore {
  list = $state<Proxy[]>([]);
  loading = $state(false);
  loaded = $state(false);
  private _promise: Promise<void> | null = null;

  async ensureLoaded() {
    if (this.loaded) return;
    if (this._promise) return this._promise;
    this._promise = this.refresh().finally(() => { this._promise = null; });
    return this._promise;
  }

  async refresh() {
    this.loading = true;
    try {
      this.list = await api.proxies.list();
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  byWorkspace(id: string): Proxy[] {
    return this.list.filter((p) => p.tags.includes(`workspace:${id}`));
  }

  private patch(id: string, fields: Partial<Proxy>) {
    this.list = this.list.map((p) => (p.id === id ? { ...p, ...fields } : p));
  }

  /** Insert a new proxy or replace the existing one with the same id. */
  upsert(proxy: Proxy) {
    const exists = this.list.some((p) => p.id === proxy.id);
    this.list = exists
      ? this.list.map((p) => (p.id === proxy.id ? proxy : p))
      : [proxy, ...this.list];
  }

  /** Delete a proxy and drop it from the cached list. */
  async remove(id: string): Promise<void> {
    await api.proxies.delete(id);
    this.list = this.list.filter((p) => p.id !== id);
  }

  /**
   * Run a connectivity check. On success (and when no new SSH host key needs
   * trusting) the cached proxy is marked active with the resolved IP/geo.
   * Returns the raw result so callers can handle the SSH fingerprint prompt.
   */
  async check(id: string): Promise<ProxyCheckResult> {
    const result = await api.proxies.check(id);
    if (!(result.ssh_fingerprint_is_new && result.ssh_fingerprint)) {
      this.patch(id, {
        status: 'active',
        last_ip: result.ip,
        country: this.list.find((p) => p.id === id)?.country || result.country,
        city: this.list.find((p) => p.id === id)?.city || result.city,
      });
    }
    return result;
  }

  /** Mark a proxy as failed after a check error. */
  markFailed(id: string) {
    this.patch(id, { status: 'failed' });
  }

  /** Pin an SSH host key (TOFU) and mark the proxy active. */
  async trustFingerprint(
    id: string,
    fingerprint: string,
    ip: string,
    country: string | null,
    city: string | null,
  ): Promise<void> {
    await api.proxies.trustFingerprint(id, fingerprint, ip, country, city);
    const current = this.list.find((p) => p.id === id);
    this.patch(id, {
      status: 'active',
      last_ip: ip,
      server_fingerprint: fingerprint,
      country: current?.country || country,
      city: current?.city || city,
    });
  }
}

export const proxiesStore = new ProxiesStore();
