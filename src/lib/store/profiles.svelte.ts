// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { Profile } from '$lib/types';

class ProfilesStore {
  list = $state<Profile[]>([]);
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
      this.list = await api.profiles.list();
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  byWorkspace(id: string): Profile[] {
    return this.list.filter((p) => p.workspace_id === id);
  }
}

export const profilesStore = new ProfilesStore();
