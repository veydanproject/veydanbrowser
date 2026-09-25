// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { api } from '$lib/api';
import { notesStore } from '$lib/store/notes.svelte';

interface NoteRef {
  id: string;
  bindings: string[];
  deleted?: boolean;
}

/** Note ids that already store `totp:id`. */
export function notesLinkedToTotp(totpId: string, notes: NoteRef[]): string[] {
  const binding = `totp:${totpId}`;
  return notes.filter((n) => !n.deleted && n.bindings.includes(binding)).map((n) => n.id);
}

/** Write `totp:id` onto notes in `next` and drop it from notes no longer selected. */
export async function syncTotpNotes(totpId: string, next: string[], prev: string[]): Promise<void> {
  const binding = `totp:${totpId}`;
  const want = new Set(next);
  const had = new Set(prev);
  let changed = false;
  for (const id of next) {
    if (had.has(id)) continue;
    await api.notes.noteAddBinding(id, binding);
    changed = true;
  }
  for (const id of prev) {
    if (want.has(id)) continue;
    await api.notes.noteRemoveBinding(id, binding);
    changed = true;
  }
  if (!changed) return;
  if (notesStore.loaded) await notesStore.refresh();
  const active = notesStore.activeNote;
  if (!active) return;
  let bindings = active.bindings;
  if (want.has(active.id) && !bindings.includes(binding)) bindings = [...bindings, binding];
  if (had.has(active.id) && !want.has(active.id)) bindings = bindings.filter((b) => b !== binding);
  if (bindings !== active.bindings) notesStore.activeNote = { ...active, bindings };
}
