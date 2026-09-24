// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/** Notes and entity commands for the palette. Registered once from the desktop shell. */

import { get } from 'svelte/store';
import { goto } from '$app/navigation';
import { api } from '$lib/api';
import { t } from '$lib/i18n';
import { commands, type Command } from '$lib/commands';
import { notesStore } from '$lib/store/notes.svelte';
import { templateNotes } from '$lib/notes-filter';
import { ENTITY_DEFS } from '$lib/notes-context';
import { ENTITY_KINDS } from '$lib/bindings';

const tr = () => get(t);

async function toNotes(): Promise<void> {
  await notesStore.ensureLoaded();
  await goto('/notes');
}

let registered = false;

export function registerNotesCommands(): void {
  // The shell may remount (HMR, window reopen); providers must not stack up
  if (registered) return;
  registered = true;
  // Provider rather than static registration so titles follow the current locale
  commands.addProvider(() => [
    {
      id: 'notes.create',
      title: tr()('cmd_notes_create'),
      keywords: 'new note',
      icon: 'plus',
      group: tr()('cmd_group_notes'),
      run: async () => {
        await toNotes();
        notesStore.uiRequest = { kind: 'create' };
      },
    },
    {
      id: 'notes.search',
      title: tr()('cmd_notes_search'),
      keywords: 'find',
      icon: 'search',
      group: tr()('cmd_group_notes'),
      run: async () => {
        await toNotes();
        notesStore.uiRequest = { kind: 'search' };
      },
    },
    {
      id: 'notes.insertLink',
      title: tr()('cmd_notes_insert_link'),
      keywords: 'wiki [[',
      icon: 'link',
      group: tr()('cmd_group_notes'),
      when: () => notesStore.activeNote !== null && !notesStore.activeNote.deleted,
      run: () => {
        notesStore.uiRequest = { kind: 'insertLink' };
      },
    },
    {
      id: 'notes.quickCapture',
      title: tr()('cmd_notes_quick_capture'),
      icon: 'zap',
      group: tr()('cmd_group_notes'),
      run: () => api.notes.openQuickCapture(),
    },
  ]);

  // One command per smart view
  commands.addProvider(() =>
    notesStore.smartViews.map((v): Command => ({
      id: `notes.smartView.${v.id}`,
      title: tr()('cmd_notes_smart_view', { name: v.name }),
      icon: 'filter',
      group: tr()('cmd_group_notes'),
      run: async () => {
        await toNotes();
        notesStore.uiRequest = { kind: 'filter', filter: { type: 'smart', id: v.id } };
      },
    })),
  );

  // New note from each template
  commands.addProvider(() =>
    templateNotes(notesStore.list, notesStore.folders).map((tpl): Command => ({
      id: `notes.fromTemplate.${tpl.id}`,
      title: tr()('cmd_notes_from_template', { name: tpl.title }),
      icon: 'file-text',
      group: tr()('cmd_group_notes'),
      run: async () => {
        await toNotes();
        const note = await notesStore.createNote({ title: tpl.title, template_id: tpl.id });
        notesStore.openRequestId = note.id;
      },
    })),
  );

  // Recently edited notes
  commands.addProvider(() =>
    [...notesStore.list]
      .filter((n) => !n.deleted && !n.archived)
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
      .slice(0, 12)
      .map((n): Command => ({
        id: `notes.open.${n.id}`,
        title: tr()('cmd_notes_open', { title: n.title || tr()('notes_untitled') }),
        keywords: n.tags.map((x) => x.name).join(' '),
        icon: 'file-text',
        group: tr()('cmd_group_notes'),
        run: async () => {
          await toNotes();
          notesStore.openRequestId = n.id;
        },
      })),
  );

  // Entity actions: `Connect: Production SG`, `Check: SG Proxy`, ...
  commands.addProvider(() => {
    const out: Command[] = [];
    for (const kind of ENTITY_KINDS) {
      const def = ENTITY_DEFS[kind];
      for (const entity of def.list()) {
        for (const action of def.actions) {
          out.push({
            id: `${kind}.${action.id}.${entity.id}`,
            title: `${tr()(action.label)}: ${entity.name}`,
            keywords: `${kind} ${entity.subtitle}`,
            icon: action.icon,
            group: tr()(def.label),
            run: () => action.run(entity.id),
          });
        }
      }
    }
    return out;
  });
}
