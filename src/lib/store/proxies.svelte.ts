// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { Proxy } from '$lib/types';

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
}

export const proxiesStore = new ProxiesStore();
