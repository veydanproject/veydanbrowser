// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import type { SyncStatus } from '$lib/api';
import { formatError } from '$lib/utils';
import { profilesStore } from './profiles.svelte';
import { proxiesStore } from './proxies.svelte';
import { workspacesStore } from './workspaces.svelte';
import { sshStore } from './ssh.svelte';
import { totpStore } from './totp.svelte';
import { notesStore } from './notes.svelte';
import { locale } from '$lib/i18n';
import { get } from 'svelte/store';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** Follow the UI language another device saved. */
async function applyRemoteLocale() {
  const remote = await api.settings.getLocale();
  if ((remote === 'en' || remote === 'ru') && remote !== get(locale)) locale.set(remote);
}

/** Store reload per sync entity applied from another device. */
const reloaders: Record<string, () => Promise<unknown>> = {
  workspace: () => workspacesStore.loaded ? workspacesStore.refresh() : Promise.resolve(),
  workspace_column: () => workspacesStore.loaded ? workspacesStore.refresh() : Promise.resolve(),
  profile: () => profilesStore.loaded ? profilesStore.refresh() : Promise.resolve(),
  proxy: () => proxiesStore.loaded ? proxiesStore.refresh() : Promise.resolve(),
  ssh_connection: () => sshStore.loadConnections(),
  ssh_key: () => sshStore.loadConnections(),
  totp: () => totpStore.loaded ? totpStore.refresh() : Promise.resolve(),
  note_tag: () => notesStore.loaded ? notesStore.refreshTags() : Promise.resolve(),
  note_folder: () => notesStore.loaded ? notesStore.refreshFolders() : Promise.resolve(),
  note_smart_view: () => notesStore.loaded ? notesStore.refreshSmartViews() : Promise.resolve(),
  note_meta: () => notesStore.loaded ? notesStore.refresh() : Promise.resolve(),
  setting: applyRemoteLocale,
};

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

  /** Reload stores whose rows another device changed; returns an unsubscribe fn. */
  async listenDataChanged(): Promise<() => void> {
    if (!isTauri) return () => {};
    const { listen } = await import('@tauri-apps/api/event');
    return listen<string[]>('sync://data-changed', (e) => {
      for (const entity of e.payload) {
        reloaders[entity]?.().catch(() => {});
      }
    });
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
