// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { binding, isScopeBinding } from '$lib/bindings';

/** Profile and workspace bindings share the tags array with free labels. */
export function isSystemTag(tag: string): boolean {
  return isScopeBinding(tag);
}

/** Free labels. These are the ones that can show up under a note tag. */
export function userLabels(tags: string[]): string[] {
  return tags.filter((tag) => !isSystemTag(tag));
}

/** `profile:{id}` and `workspace:{id}` bindings. */
export function systemTags(tags: string[]): string[] {
  return tags.filter(isSystemTag);
}

/** Locked context bindings, chosen bindings, and labels, without duplicates. */
export function mergeTags(locked: string[], bindings: string[], labels: string[]): string[] {
  const out: string[] = [];
  const push = (tag: string) => {
    const value = tag.trim();
    if (!value || out.includes(value)) return;
    out.push(value);
  };
  for (const tag of locked) if (isSystemTag(tag)) push(tag);
  for (const tag of bindings) if (isSystemTag(tag)) push(tag);
  for (const tag of labels) if (!isSystemTag(tag)) push(tag);
  return out;
}

/**
 * A code belongs in the notes list only for the open tag, profile, or workspace.
 * Empty tags never match. All / pinned / folder / trash do not match.
 */
export function totpMatchesFilter(tags: string[], kind: string, id?: string | null): boolean {
  if (!id || tags.length === 0) return false;
  if (kind === 'tag') return userLabels(tags).includes(id);
  if (kind === 'profile' || kind === 'workspace') return tags.includes(binding(kind, id));
  return false;
}
