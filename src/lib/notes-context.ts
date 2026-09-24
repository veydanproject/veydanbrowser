// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/**
 * Desktop registry of entity kinds a note can be bound to: how to load, name,
 * describe and act on each kind. Adding a kind here is the only change needed
 * for it to appear in note chips, context cards, the `[[kind:` picker and suggestions.
 */

import { goto } from '$app/navigation';
import { api } from '$lib/api';
import { bindingValue, isEntityKind, parseBinding, type EntityKind } from '$lib/bindings';
import type { TranslationKey } from '$lib/i18n';
import { workspacesStore } from '$lib/store/workspaces.svelte';
import { profilesStore } from '$lib/store/profiles.svelte';
import { proxiesStore } from '$lib/store/proxies.svelte';
import { sshStore } from '$lib/store/ssh.svelte';
import { totpStore } from '$lib/store/totp.svelte';

export type EntityStatus = 'ok' | 'bad' | 'unknown';

export interface EntitySummary {
  id: string;
  name: string;
  /** Short second line, e.g. `SOCKS5 · Singapore` */
  subtitle: string;
  status: EntityStatus | null;
  color: string;
}

export interface EntityAction {
  id: string;
  label: TranslationKey;
  icon: string;
  run: (id: string) => Promise<void>;
}

export interface EntityKindDef {
  kind: EntityKind;
  icon: string;
  /** Kind name */
  label: TranslationKey;
  color: string;
  ensureLoaded: () => Promise<void>;
  list: () => EntitySummary[];
  actions: EntityAction[];
}

/** Backend clipboard: actions run after awaited calls, where `navigator.clipboard` is denied. */
async function copyText(text: string): Promise<void> {
  await api.system.clipboardWriteText(text);
}

const region = (country: string | null, city: string | null) =>
  [country, city].filter((s): s is string => !!s).join(', ');

export const ENTITY_DEFS: Record<EntityKind, EntityKindDef> = {
  workspace: {
    kind: 'workspace',
    icon: 'layers',
    label: 'ctx_kind_workspace',
    color: 'var(--success)',
    ensureLoaded: () => workspacesStore.ensureLoaded(),
    list: () =>
      workspacesStore.list.map((w) => ({
        id: w.id,
        name: w.name,
        subtitle: w.description ?? '',
        status: null,
        color: w.color,
      })),
    actions: [
      { id: 'open', label: 'ctx_action_open', icon: 'external-link', run: async (id) => { await goto(`/workspace/${id}`); } },
    ],
  },
  profile: {
    kind: 'profile',
    icon: 'globe',
    label: 'ctx_kind_profile',
    color: 'var(--accent)',
    ensureLoaded: () => profilesStore.ensureLoaded(),
    list: () =>
      profilesStore.list.map((p) => ({
        id: p.id,
        name: p.name,
        subtitle: p.browser_type,
        status: p.status === 'running' ? 'ok' : 'unknown',
        color: 'var(--accent)',
      })),
    actions: [
      {
        id: 'launch',
        label: 'ctx_action_launch',
        icon: 'play',
        run: async (id) => {
          await api.profiles.launch(id);
          await profilesStore.refresh();
        },
      },
    ],
  },
  proxy: {
    kind: 'proxy',
    icon: 'shield',
    label: 'ctx_kind_proxy',
    color: 'var(--warn-text)',
    ensureLoaded: () => proxiesStore.ensureLoaded(),
    list: () =>
      proxiesStore.list.map((p) => ({
        id: p.id,
        name: p.name,
        subtitle: [p.proxy_type.toUpperCase(), region(p.country, p.city)].filter(Boolean).join(' · '),
        status: p.status === 'active' ? 'ok' : p.status === 'failed' ? 'bad' : 'unknown',
        color: 'var(--warn-text)',
      })),
    actions: [
      {
        id: 'check',
        label: 'ctx_action_check',
        icon: 'zap',
        run: async (id) => {
          try {
            await proxiesStore.check(id);
          } catch (e) {
            proxiesStore.markFailed(id);
            throw e;
          }
        },
      },
      {
        id: 'copy',
        label: 'ctx_action_copy',
        icon: 'copy',
        run: async (id) => { await copyText(await api.proxies.exportUrl(id)); },
      },
    ],
  },
  ssh: {
    kind: 'ssh',
    icon: 'terminal',
    label: 'ctx_kind_ssh',
    color: 'var(--text-2)',
    ensureLoaded: () => sshStore.ensureLoaded(),
    list: () =>
      sshStore.connections.map((c) => ({
        id: c.id,
        name: c.name,
        subtitle: `${c.username}@${c.host}${c.port !== 22 ? `:${c.port}` : ''}`,
        status: sshStore.activeSession(c.id) ? 'ok' : null,
        color: 'var(--text-2)',
      })),
    actions: [
      { id: 'connect', label: 'ctx_action_connect', icon: 'terminal', run: async (id) => { await sshStore.connect(id); } },
      {
        id: 'copy',
        label: 'ctx_action_copy_host',
        icon: 'copy',
        run: async (id) => {
          const c = sshStore.connections.find((x) => x.id === id);
          if (c) await copyText(c.host);
        },
      },
    ],
  },
  totp: {
    kind: 'totp',
    icon: 'key',
    label: 'ctx_kind_totp',
    color: 'var(--text-2)',
    ensureLoaded: () => totpStore.ensureLoaded(),
    list: () =>
      totpStore.list.map((e) => ({
        id: e.id,
        name: e.issuer ? `${e.issuer} · ${e.name}` : e.name,
        subtitle: '',
        status: null,
        color: 'var(--text-2)',
      })),
    // Used by the command palette; the context card shows the live code chip instead
    actions: [
      {
        id: 'copy-code',
        label: 'ctx_action_copy_code',
        icon: 'copy',
        run: async (id) => {
          const code = await api.totp.generateCode(id);
          await copyText(code.code);
        },
      },
    ],
  },
};

export function entitySummary(kind: EntityKind, id: string): EntitySummary | undefined {
  return ENTITY_DEFS[kind].list().find((e) => e.id === id);
}

/** Entities behind a note's bindings, as the table's context column shows them. */
export function noteContextEntities(note: { bindings: string[] }): { name: string; icon: string; color: string }[] {
  const out: { name: string; icon: string; color: string }[] = [];
  for (const b of note.bindings) {
    const p = parseBinding(b);
    if (!p || !isEntityKind(p.kind)) continue;
    const e = entitySummary(p.kind, p.value);
    if (e) out.push({ name: e.name, icon: ENTITY_DEFS[p.kind].icon, color: e.color });
  }
  return out;
}

/** Entities of `kind` whose name or subtitle contains `query` (case-insensitive). */
export function searchEntities(kind: EntityKind, query: string, max = 8): EntitySummary[] {
  const q = query.trim().toLowerCase();
  const all = ENTITY_DEFS[kind].list();
  const hits = q ? all.filter((e) => `${e.name} ${e.subtitle}`.toLowerCase().includes(q)) : all;
  return hits.slice(0, max);
}

/**
 * Current value of a template placeholder for a note; '' when nothing is bound.
 * Mirrors `templates.rs` so a placeholder picked in a regular note can be expanded in place.
 */
export function resolvePlaceholder(name: string, title: string, bindings: string[]): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}:${pad(now.getMinutes())}`;
  const id = (kind: EntityKind) => bindingValue(bindings, kind);
  const proxy = proxiesStore.list.find((p) => p.id === id('proxy'));
  const ssh = sshStore.connections.find((c) => c.id === id('ssh'));
  const name_ = (kind: EntityKind) => {
    const v = id(kind);
    return v ? entitySummary(kind, v)?.name ?? '' : '';
  };
  switch (name) {
    case 'date': return date;
    case 'time': return time;
    case 'datetime': return `${date} ${time}`;
    case 'title': return title;
    case 'workspace': return name_('workspace');
    case 'profile': return name_('profile');
    case 'totp': return name_('totp');
    case 'url': return bindingValue(bindings, 'url') ?? '';
    case 'domain': return bindingValue(bindings, 'domain') ?? '';
    case 'proxy': return proxy?.name ?? '';
    case 'proxy_type': return proxy?.proxy_type ?? '';
    case 'proxy_host': return proxy?.host ?? '';
    case 'proxy_port': return proxy ? String(proxy.port) : '';
    case 'proxy_region': return proxy ? region(proxy.country, proxy.city) : '';
    case 'ssh': return ssh?.name ?? '';
    case 'ssh_host': return ssh?.host ?? '';
    case 'ssh_port': return ssh ? String(ssh.port) : '';
    case 'ssh_user': return ssh?.username ?? '';
    default: return '';
  }
}

/** Load every entity store the cards and pickers read from. */
export function ensureEntitiesLoaded(): Promise<unknown> {
  return Promise.all(Object.values(ENTITY_DEFS).map((d) => d.ensureLoaded().catch(() => {})));
}
