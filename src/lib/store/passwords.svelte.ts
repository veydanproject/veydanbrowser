// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import { notesStore } from '$lib/store/notes.svelte';
import type { PasswordEntry } from '$lib/types';

class PasswordStore {
  list = $state<PasswordEntry[]>([]);
  loading = $state(false);
  loaded = $state(false);
  /** Set by the command palette; the password drawer opens the create form. */
  createRequest = $state(false);
  /** Password to show when the drawer opens. */
  openId = $state<string | null>(null);
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
      this.list = await api.passwords.list();
      this.loaded = true;
    } finally {
      this.loading = false;
    }
  }

  byProfile(profileId: string): PasswordEntry[] {
    const tag = `profile:${profileId}`;
    return this.list.filter((e) => e.tags.includes(tag));
  }

  countForProfile(profileId: string): number {
    return this.byProfile(profileId).length;
  }

  byWorkspace(workspaceId: string, profileIds: string[]): PasswordEntry[] {
    const wsTag = `workspace:${workspaceId}`;
    const profileTags = new Set(profileIds.map((id) => `profile:${id}`));
    return this.list.filter(
      (e) => e.tags.includes(wsTag) || e.tags.some((t) => profileTags.has(t)),
    );
  }

  /** Move a linked TOTP to the front; the first one shows in the password list. */
  async makePrimaryTotp(passwordId: string, totpId: string) {
    const entry = this.list.find((e) => e.id === passwordId);
    if (!entry || entry.totp_ids[0] === totpId || !entry.totp_ids.includes(totpId)) return;
    const next = [totpId, ...entry.totp_ids.filter((id) => id !== totpId)];
    await api.passwords.update(passwordId, { totp_ids: next });
    await this.refresh();
  }

  /** Remove the soft note tag and the leftover password binding on that note. */
  async unlinkNote(passwordId: string, noteId: string) {
    const tag = `note:${noteId}`;
    const entry = this.list.find((e) => e.id === passwordId);
    if (entry?.tags.includes(tag)) {
      await api.passwords.update(passwordId, { tags: entry.tags.filter((t) => t !== tag) });
      await this.refresh();
    }
    const binding = `password:${passwordId}`;
    const note = notesStore.activeNote?.id === noteId
      ? notesStore.activeNote
      : notesStore.list.find((n) => n.id === noteId);
    if (note?.bindings.includes(binding)) await notesStore.removeNoteBinding(noteId, binding);
  }

  /** Notes keep a password binding only while the password still tags them. */
  async dropStaleNoteBindings(passwordId: string, tags: string[]) {
    await notesStore.ensureLoaded();
    const linked = new Set(tags.filter((t) => t.startsWith('note:')).map((t) => t.slice(5)));
    const binding = `password:${passwordId}`;
    const stale = notesStore.list.filter((n) => n.bindings.includes(binding) && !linked.has(n.id));
    if (stale.length === 0) return;
    await Promise.all(stale.map((n) => api.notes.noteRemoveBinding(n.id, binding)));
    const active = notesStore.activeNote;
    if (active && stale.some((n) => n.id === active.id)) {
      notesStore.activeNote = { ...active, bindings: active.bindings.filter((b) => b !== binding) };
    }
    await notesStore.refresh();
  }
}

export const passwordStore = new PasswordStore();
