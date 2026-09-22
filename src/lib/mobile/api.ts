// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Mobile view of the shared API: same Rust commands as desktop, reshaped for
// the mobile screens (chips on cards, plain tag names, list filters by kind).

import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api as shared, downloadNoteAttachment } from '$lib/api';
import { pickNativeFiles } from '$lib/attachmentTransfer';
import { formatError } from '$lib/utils';
import type {
  ConflictView,
  MergeBlock,
  NavChild,
  Note as SharedNote,
  NoteAttachment,
  NoteFilter,
  NoteHistoryEntry,
  NoteLinks as SharedNoteLinks,
  NoteListItem as SharedNoteListItem,
  NoteNav,
  NoteSmartView,
  NoteSyncInfo,
  SmartViewInput,
} from '$lib/types';
import type { SyncConfig, SyncProgress, SyncStatus } from '$lib/api';

export { formatError };
export type {
  ConflictView,
  MergeBlock,
  NavChild,
  NoteAttachment,
  NoteFilter,
  NoteHistoryEntry,
  NoteNav,
  NoteSmartView,
  NoteSyncInfo,
  SmartViewInput,
  SyncConfig,
  SyncProgress,
  SyncStatus,
};

export type SyncProbe = 'empty' | 'vault' | 'foreign';

export interface NoteChip {
  kind: 'folder' | 'workspace' | 'profile' | 'domain' | 'tag' | string;
  id: string;
  label: string;
  color: string;
}

export interface NoteListItem {
  id: string;
  title: string;
  pinned: boolean;
  archived: boolean;
  created_at: string;
  updated_at: string;
  preview: string;
  tags: string[];
  chips: NoteChip[];
}

export interface Note extends Omit<NoteListItem, 'preview'> {
  content: string;
}

export interface NoteUpdateInput {
  title?: string;
  content?: string;
  pinned?: boolean;
  archived?: boolean;
  tags?: string[];
}

export interface NoteTextMatch {
  id: string;
  title: string;
  updated_at: string;
  snippet: string;
  chips: NoteChip[];
}

export interface NoteSearchResult {
  notes: NoteListItem[];
  text_matches: NoteTextMatch[];
}

export interface NoteTag {
  id: string;
  name: string;
  color: string;
  count: number;
}

export interface NoteListFilter {
  kind: string;
  id?: string;
}

export interface NoteFolder {
  id: string;
  name: string;
  parent_id: string | null;
  color: string;
}

export interface NoteLinks {
  outgoing: NoteListItem[];
  backlinks: NoteListItem[];
}

// ── Shape adapters ───────────────────────────────────────────────────────────

const PROFILE_COLOR = '#8b7bff';
const DOMAIN_COLOR = '#888';

/** Chips in desktop order: folder, workspace, profile, domain, tag. */
function chipsFor(item: SharedNoteListItem, nav: NoteNav): NoteChip[] {
  const out: NoteChip[] = [];
  for (const fid of item.folder_ids ?? []) {
    const f = nav.folders.find((x) => x.id === fid);
    if (f) out.push({ kind: 'folder', id: f.id, label: f.name, color: f.color });
  }
  for (const b of item.bindings ?? []) {
    if (b.startsWith('workspace:')) {
      const id = b.slice('workspace:'.length);
      const w = nav.all_workspaces.find((x) => x.id === id);
      if (w) out.push({ kind: 'workspace', id, label: w.name, color: w.color });
    } else if (b.startsWith('profile:')) {
      const id = b.slice('profile:'.length);
      const p = nav.all_profiles.find((x) => x.id === id);
      if (p) out.push({ kind: 'profile', id, label: p.name, color: PROFILE_COLOR });
    } else if (b.startsWith('domain:')) {
      const domain = b.slice('domain:'.length);
      if (domain) out.push({ kind: 'domain', id: domain, label: domain, color: DOMAIN_COLOR });
    }
  }
  for (const t of item.tags ?? []) out.push({ kind: 'tag', id: t.name, label: t.name, color: t.color });
  return out;
}

function toItem(d: SharedNoteListItem, nav: NoteNav): NoteListItem {
  return {
    id: d.id,
    title: d.title,
    pinned: d.pinned,
    archived: d.archived,
    created_at: d.created_at,
    updated_at: d.updated_at,
    preview: d.preview,
    tags: (d.tags ?? []).map((t) => t.name),
    chips: chipsFor(d, nav),
  };
}

function toNote(d: SharedNote, nav: NoteNav): Note {
  const { preview: _preview, ...item } = toItem(d, nav);
  return { ...item, content: d.content ?? '' };
}

function toTag(t: { id: string; name: string; color: string; count?: number }): NoteTag {
  return { id: t.id, name: t.name, color: t.color, count: t.count ?? 0 };
}

/** Mobile list kinds mapped onto the shared `NoteFilter`. */
async function filterFor(q: NoteListFilter): Promise<NoteFilter> {
  const id = q.id ?? '';
  switch (q.kind) {
    case 'global': return { archived: false, global_only: true };
    case 'workspace': return { archived: false, binding: `workspace:${id}` };
    case 'profile': return { archived: false, binding: `profile:${id}` };
    case 'domain': return { archived: false, binding: `domain:${id}` };
    case 'tag': return { archived: false, tag_name: id };
    case 'folder': return { archived: false, folder_id: id };
    case 'pinned': return { archived: false, pinned: true };
    case 'archived': return { archived: true };
    case 'trash': return { deleted: true };
    case 'smart': {
      const view = (await shared.notes.smartViewList()).find((s) => s.id === id);
      const f: NoteFilter = { ...(view?.conditions ?? {}) };
      if (f.archived === undefined) f.archived = false;
      return f;
    }
    default: return { archived: false };
  }
}

const stripMarks = (s: string) => s.replace(/<\/?mark>/g, '');

// ── Events ───────────────────────────────────────────────────────────────────

/** Runs `cb` when sync applied remote changes to one of `entities` ("note", "totp", ...). */
export async function onSyncChanged(entities: string[], cb: () => void): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = [
    await listen<string[]>('sync://data-changed', (e) => {
      if (e.payload.some((x) => entities.includes(x))) cb();
    }),
  ];
  // Note bodies and attachments are reported on their own channels.
  if (entities.includes('note')) unlisteners.push(await listen('notes://external-change', cb));
  if (entities.includes('note_attachment')) unlisteners.push(await listen('notes://attachments-changed', cb));
  return () => unlisteners.forEach((f) => f());
}

/** Remote apply of one note body. Does not start a sync cycle. */
export function onNoteRemote(cb: (id: string) => void): Promise<UnlistenFn> {
  return listen<string>('notes://external-change', (e) => cb(e.payload));
}

export function onSyncStatus(cb: () => void): Promise<UnlistenFn> {
  return listen('sync://status', cb);
}

export function onSyncProgress(cb: (p: SyncProgress) => void): Promise<UnlistenFn> {
  return listen<SyncProgress>('sync://progress', (e) => cb(e.payload));
}

export function syncProgressText(
  t: (key: string, vars?: Record<string, string>) => string,
  p: SyncProgress,
): string {
  const phase = t(`settings_sync_phase_${p.phase}`);
  if (p.total > 0) {
    return t('settings_sync_progress_files', {
      phase,
      current: String(p.current),
      total: String(p.total),
      percent: String(p.percent),
    });
  }
  return t('settings_sync_progress', { phase, percent: String(p.percent) });
}

// ── API ──────────────────────────────────────────────────────────────────────

export const api = {
  sync: {
    getConfig: () => shared.sync.getConfig(),
    setConfig: (cfg: SyncConfig) => shared.sync.setConfig(cfg),
    probe: () => shared.sync.probe(),
    createVault: (passphrase: string) => shared.sync.createVault(passphrase),
    joinVault: (passphrase: string) => shared.sync.joinVault(passphrase),
    leave: () => shared.sync.leave(),
    status: () => shared.sync.status(),
    runNow: () => shared.sync.runNow(),
    trigger: () => shared.sync.trigger(),
    conflictGet: (noteId: string) => shared.sync.conflictGet(noteId),
    conflictResolve: (noteId: string, token: string, content: string) =>
      shared.sync.conflictResolve(noteId, token, content),
    attachmentCancel: (noteId: string, name: string) => shared.sync.attachmentCancel(noteId, name),
  },
  notes: {
    nav: () => shared.notes.nav(),
    list: async (search = '', filter: NoteListFilter = { kind: 'all' }): Promise<NoteListItem[]> => {
      const [items, nav] = await Promise.all([filterFor(filter).then((f) => shared.notes.list(f)), shared.notes.nav()]);
      const q = search.trim().toLowerCase();
      return items
        .filter((n) => !q || n.title.toLowerCase().includes(q) || n.preview.toLowerCase().includes(q))
        .map((n) => toItem(n, nav));
    },
    get: async (id: string): Promise<Note> => {
      const [n, nav] = await Promise.all([shared.notes.get(id), shared.notes.nav()]);
      return toNote(n, nav);
    },
    create: async (title: string, content = '', tags: string[] = [], bindings: string[] = []): Promise<Note> => {
      const n = await shared.notes.create({ title, content, tag_names: tags, bindings });
      return toNote(n, await shared.notes.nav());
    },
    update: async (id: string, input: NoteUpdateInput): Promise<Note> => {
      if (input.tags) await shared.notes.setTags(id, input.tags);
      if (input.title !== undefined || input.content !== undefined || input.pinned !== undefined) {
        await shared.notes.update(id, { title: input.title, content: input.content, pinned: input.pinned });
      }
      if (input.archived === true) await shared.notes.archive(id);
      else if (input.archived === false) await shared.notes.restore(id);
      const [n, nav] = await Promise.all([shared.notes.get(id), shared.notes.nav()]);
      return toNote(n, nav);
    },
    delete: (id: string) => shared.notes.delete(id),
    restore: async (id: string): Promise<Note> => {
      await shared.notes.restore(id);
      const [n, nav] = await Promise.all([shared.notes.get(id), shared.notes.nav()]);
      return toNote(n, nav);
    },
    purge: (id: string) => shared.notes.delete(id, true),
    emptyTrash: () => shared.notes.emptyTrash(),
    setFolder: (noteId: string, folderId: string | null) => shared.notes.noteSetFolder(noteId, folderId),
    addFolder: (noteId: string, folderId: string) => shared.notes.noteAddFolder(noteId, folderId),
    removeFolder: (noteId: string, folderId: string) => shared.notes.noteRemoveFolder(noteId, folderId),
    addBinding: (noteId: string, binding: string) => shared.notes.noteAddBinding(noteId, binding),
    removeBinding: (noteId: string, binding: string) => shared.notes.noteRemoveBinding(noteId, binding),
    /** Title hits first, body-only hits with an excerpt after. */
    search: async (query: string): Promise<NoteSearchResult> => {
      const q = query.trim().toLowerCase();
      const out: NoteSearchResult = { notes: [], text_matches: [] };
      if (!q) return out;
      const [items, nav] = await Promise.all([shared.notes.search(query), shared.notes.nav()]);
      for (const d of items) {
        if (d.deleted) continue;
        if (d.title.toLowerCase().includes(q)) {
          out.notes.push(toItem(d, nav));
          continue;
        }
        out.text_matches.push({
          id: d.id,
          title: d.title,
          updated_at: d.updated_at,
          snippet: stripMarks(d.snippet ?? d.preview),
          chips: chipsFor(d, nav),
        });
      }
      return out;
    },
    tags: async (): Promise<NoteTag[]> => (await shared.notes.nav()).tags.map(toTag),
    tagCreate: async (name: string, color?: string) => toTag(await shared.notes.tagCreate(name, color)),
    tagUpdate: async (id: string, name?: string, color?: string) => toTag(await shared.notes.tagUpdate(id, name, color)),
    tagDelete: (id: string) => shared.notes.tagDelete(id),
    folderCreate: (name: string, parentId?: string, color?: string): Promise<NoteFolder> =>
      shared.notes.folderCreate(name, parentId, color),
    folderUpdate: (id: string, name?: string, color?: string): Promise<NoteFolder> =>
      shared.notes.folderUpdate(id, name, color),
    folderDelete: (id: string) => shared.notes.folderDelete(id),
    smartViewGet: async (id: string): Promise<NoteSmartView> => {
      const view = (await shared.notes.smartViewList()).find((s) => s.id === id);
      if (!view) throw new Error(`smart view ${id} not found`);
      return view;
    },
    smartViewCreate: (input: SmartViewInput) => shared.notes.smartViewCreate(input),
    smartViewUpdate: (id: string, input: SmartViewInput) => shared.notes.smartViewUpdate(id, input),
    smartViewDelete: (id: string) => shared.notes.smartViewDelete(id),
    historyList: (noteId: string) => shared.notes.historyList(noteId),
    historyGet: (id: string) => shared.notes.historyGet(id),
    historyRestore: async (noteId: string, historyId: string): Promise<Note> => {
      const n = await shared.notes.historyRestore(noteId, historyId);
      return toNote(n, await shared.notes.nav());
    },
    links: async (id: string): Promise<NoteLinks> => {
      const [l, nav]: [SharedNoteLinks, NoteNav] = await Promise.all([shared.notes.links(id), shared.notes.nav()]);
      return { outgoing: l.outgoing.map((n) => toItem(n, nav)), backlinks: l.backlinks.map((n) => toItem(n, nav)) };
    },
    resolveLink: (target: string) => shared.notes.resolveLink(target),
    syncInfo: (id: string) => shared.notes.syncInfo(id),
  },
  attachments: {
    list: (noteId: string) => shared.notes.attachmentList(noteId),
    fetch: (noteId: string, name: string) => shared.notes.attachmentFetch(noteId, name),
    /** System save dialog; on Android the `content://` target is written by Rust. */
    save: (noteId: string, name: string) => downloadNoteAttachment(noteId, name),
    /** Native picker; the `content://` URIs are streamed by Rust, nothing crosses IPC as bytes. */
    pick: async (noteId: string, imagesOnly: boolean): Promise<NoteAttachment[]> => {
      const out: NoteAttachment[] = [];
      for (const uri of await pickNativeFiles(imagesOnly)) out.push(await shared.notes.attachmentAddFromPath(noteId, uri));
      return out;
    },
    read: (noteId: string, name: string) => shared.notes.attachmentRead(noteId, name),
    delete: (noteId: string, name: string) => shared.notes.attachmentDelete(noteId, name),
  },
  totp: shared.totp,
};
