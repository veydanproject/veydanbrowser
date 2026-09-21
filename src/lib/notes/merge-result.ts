// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Assembles the final text from 3-way merge blocks and per-block choices.
// Shared by the desktop merge panel and the mobile conflict sheet.

import type { MergeBlock } from '$lib/types';

export type MergeSide = 'ours' | 'theirs';
export type MergeChoice = MergeSide | 'both';
/** One entry per block; normal blocks stay null. */
export type MergeSelections = (MergeChoice | null)[];

export function emptySelections(blocks: MergeBlock[]): MergeSelections {
  return blocks.map(() => null);
}

/** Picks `side` for every conflict block. */
export function selectAll(blocks: MergeBlock[], side: MergeSide): MergeSelections {
  return blocks.map((b) => (b.kind === 'conflict' ? side : null));
}

export function unresolvedCount(blocks: MergeBlock[], selections: MergeSelections): number {
  return blocks.filter((b, i) => b.kind === 'conflict' && !selections[i]).length;
}

export function isComplete(blocks: MergeBlock[], selections: MergeSelections): boolean {
  return unresolvedCount(blocks, selections) === 0;
}

/** Block texts keep their trailing newlines, so plain concatenation rebuilds the file. */
export function buildMergeContent(blocks: MergeBlock[], selections: MergeSelections): string {
  return blocks
    .map((b, i) => {
      if (b.kind === 'normal') return b.text;
      switch (selections[i]) {
        case 'theirs': return b.theirs;
        case 'both': return b.ours + b.theirs;
        default: return b.ours;
      }
    })
    .join('');
}
