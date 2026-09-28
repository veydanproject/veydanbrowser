// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Messenger command layer. Intentionally separate from `$lib/api` so the
// module can be lifted into a standalone app: this file is the only place
// that knows command names (docs/messenger-spec.md §4.6).

export interface MessengerIngressCounters {
  received: number;
  duplicates: number;
  dispatched: number;
  dm: number;
  ignored: number;
}

export interface MessengerRuntimeStatus {
  version: string;
  data_dir: string;
  schema_version: number;
  secrets_unlocked: boolean;
  identity_present: boolean;
  session_active: boolean;
  relays_total: number;
  relays_connected: number;
  silent_mode: boolean;
  manifest_serial: number | null;
  region: string;
  ingress: MessengerIngressCounters;
  outbox_pending: number;
}

export interface MessengerProfile {
  pubkey: string;
  npub: string;
  name: string | null;
  display_name: string | null;
  about: string | null;
  picture: string | null;
  banner: string | null;
  website: string | null;
  nip05: string | null;
  lud16: string | null;
  nip05_verified: boolean;
  event_created_at: number;
  fetched_at: number;
}

export interface MessengerProfileInput {
  name?: string | null;
  display_name?: string | null;
  about?: string | null;
  picture?: string | null;
  banner?: string | null;
  website?: string | null;
  nip05?: string | null;
  lud16?: string | null;
}

export interface MessengerContact {
  pubkey: string;
  npub: string;
  nickname: string | null;
  note: string | null;
  is_muted: boolean;
  notification_level: 'all' | 'mentions' | 'none';
  followed: boolean;
  profile: MessengerProfile | null;
  created_at: number;
  updated_at: number;
}

export interface MessengerContactPatch {
  nickname?: string | null;
  note?: string | null;
  is_muted?: boolean;
  notification_level?: 'all' | 'mentions' | 'none';
}

/** Display label: nickname → profile name → short npub. */
export function contactLabel(c: MessengerContact): string {
  return c.nickname?.trim() || profileLabel(c.profile) || `${c.npub.slice(0, 12)}…${c.npub.slice(-4)}`;
}

export function profileLabel(p: MessengerProfile | null): string {
  if (!p) return '';
  return p.display_name?.trim() || p.name?.trim() || p.nip05 || '';
}

/** Runtime UI event as forwarded by the host. */
export interface MessengerUiEvent {
  name: string;
  payload: unknown;
}

/** Event name: payload is `MessengerUiEvent`. */
export const RUNTIME_EVENT = 'messenger://event';

export type RelayState = 'disconnected' | 'connecting' | 'connected' | 'paused';

export interface MessengerRelay {
  url: string;
  relay_id: string | null;
  source: 'manifest' | 'user';
  regions: string[];
  read: boolean;
  write: boolean;
  enabled: boolean;
  /** 'nip42' | 'api_key' | null. The key itself never reaches the UI. */
  auth_type: string | null;
  state: RelayState;
}

export interface MessengerManifestInfo {
  serial: number | null;
  issued_at: number | null;
  region: string;
  regions: string[];
  silent_mode: boolean;
}

/** Event name: payload is `MessengerRelay[]`. */
export const RELAY_STATUS_EVENT = 'messenger://relay-status';

export interface MessengerStatus {
  /** False when the host binary was built without the `messenger` feature. */
  compiled: boolean;
  /** Host setting: the module is shown in navigation. */
  enabled: boolean;
  runtime: MessengerRuntimeStatus | null;
  error: string | null;
}

export interface MessengerIdentity {
  npub: string;
  pubkey: string;
  created_at: number;
}

export interface MessengerCreatedIdentity {
  identity: MessengerIdentity;
  /** NIP-49 backup string; shown once, never stored by the UI. */
  ncryptsec: string;
}

export type IdentityImportKind = 'nsec' | 'ncryptsec' | 'mnemonic';

/** Error shape forwarded by the host (`{ code, message }`). */
export interface MessengerError {
  code: string;
  message?: string;
}

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const NOT_COMPILED: MessengerStatus = { compiled: false, enabled: false, runtime: null, error: null };

let mockIdentity: MessengerIdentity | null = null;
let mockRelays: MessengerRelay[] = [
  { url: 'wss://relay.damus.io', relay_id: 'pub-damus', source: 'manifest', regions: ['default'], read: true, write: true, enabled: true, auth_type: 'api_key', state: 'connected' },
  { url: 'wss://nos.lol', relay_id: 'pub-nos', source: 'manifest', regions: ['default'], read: true, write: true, enabled: false, auth_type: null, state: 'disconnected' },
];
let mockSilent = false;
let mockRegion = 'default';
let mockContacts: MessengerContact[] = [];
let mockOwnProfile: MessengerProfile | null = null;
const emptyProfile = (pubkey: string): MessengerProfile => ({
  pubkey, npub: `npub1${pubkey.slice(0, 58)}`, name: null, display_name: null, about: null, picture: null, banner: null,
  website: null, nip05: null, lud16: null, nip05_verified: false, event_created_at: 0, fetched_at: 0,
});

const devMocks: Record<string, (args?: Record<string, unknown>) => unknown> = {
  messenger_status: () => ({
    compiled: true,
    enabled: true,
    runtime: {
      version: '0.1.0-dev',
      data_dir: '/home/dev/.local/share/net.veydan.space/VeydanSpace/messenger',
      schema_version: 2,
      secrets_unlocked: true,
      identity_present: mockIdentity !== null,
      session_active: mockIdentity !== null,
      relays_total: mockRelays.filter((r) => r.enabled).length,
      relays_connected: mockRelays.filter((r) => r.state === 'connected').length,
      silent_mode: mockSilent,
      manifest_serial: 2,
      region: mockRegion,
      ingress: { received: 0, duplicates: 0, dispatched: 0, dm: 0, ignored: 0 },
      outbox_pending: 0,
    },
    error: null,
  }),
  messenger_dm_send_text: () => `local-${Date.now().toString(16)}`,
  messenger_profile_get: () => null,
  messenger_profile_request: () => undefined,
  messenger_profile_own_get: () => mockOwnProfile,
  messenger_profile_own_set: (a) => {
    const i = (a?.input ?? {}) as MessengerProfileInput;
    mockOwnProfile = { ...(mockOwnProfile ?? emptyProfile('ab'.repeat(32))), ...i, event_created_at: Date.now() / 1000 } as MessengerProfile;
    return mockOwnProfile;
  },
  messenger_nip05_verify: () => false,
  messenger_contacts_list: () => mockContacts,
  messenger_contacts_add: (a) => {
    const c: MessengerContact = { pubkey: 'ef'.repeat(32), npub: 'npub1mockcontactmockcontactmockcontactmockcontactmockcontact0000', nickname: (a?.nickname as string) ?? null, note: null, is_muted: false, notification_level: 'all', followed: false, profile: null, created_at: Date.now() / 1000, updated_at: Date.now() / 1000 };
    mockContacts = [c, ...mockContacts];
    return c;
  },
  messenger_contacts_update: (a) => { mockContacts = mockContacts.map((c) => c.pubkey === a?.pubkey ? { ...c, ...(a?.patch as object) } : c); return mockContacts.find((c) => c.pubkey === a?.pubkey); },
  messenger_contacts_remove: (a) => { mockContacts = mockContacts.filter((c) => c.pubkey !== a?.pubkey); },
  messenger_contacts_set_followed: (a) => { mockContacts = mockContacts.map((c) => c.pubkey === a?.pubkey ? { ...c, followed: Boolean(a?.followed) } : c); },
  messenger_relays_list: () => mockRelays,
  messenger_relays_add: (a) => {
    const r: MessengerRelay = { url: String(a?.url), relay_id: null, source: 'user', regions: [], read: true, write: true, enabled: true, auth_type: null, state: 'connecting' };
    mockRelays = [...mockRelays, r];
    return r;
  },
  messenger_relays_remove: (a) => { mockRelays = mockRelays.filter((r) => r.url !== a?.url); },
  messenger_relays_set_enabled: (a) => { mockRelays = mockRelays.map((r) => r.url === a?.url ? { ...r, enabled: Boolean(a?.enabled) } : r); },
  messenger_relays_set_silent: (a) => { mockSilent = Boolean(a?.enabled); },
  messenger_manifest_info: () => ({ serial: 1, issued_at: 1759017600, region: mockRegion, regions: ['default', 'ru'], silent_mode: mockSilent }),
  messenger_manifest_set_region: (a) => { mockRegion = String(a?.region); },
  messenger_set_enabled: () => undefined,
  messenger_identity_get: () => mockIdentity,
  messenger_identity_create: () => {
    mockIdentity = { npub: 'npub1devdevdevdevdevdevdevdevdevdevdevdevdevdevdevdevdevdevdev0000', pubkey: 'ab'.repeat(32), created_at: Date.now() / 1000 };
    return { identity: mockIdentity, ncryptsec: 'ncryptsec1devbackupdevbackupdevbackup' };
  },
  messenger_identity_import: () => {
    mockIdentity = { npub: 'npub1importedimportedimportedimportedimportedimportedimported00', pubkey: 'cd'.repeat(32), created_at: Date.now() / 1000 };
    return mockIdentity;
  },
  messenger_identity_export: () => 'ncryptsec1devbackupdevbackupdevbackup',
  messenger_identity_delete: () => { mockIdentity = null; },
};

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }
  console.warn(`[dev-browser] messenger invoke('${cmd}')`, args ?? '');
  return devMocks[cmd]?.(args) as T;
}

/** True when Tauri reports the command does not exist (feature not compiled). */
function isUnknownCommand(e: unknown): boolean {
  const msg = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e ?? '');
  return /command .* not found/i.test(msg) || /not allowed/i.test(msg);
}

/** Human-readable text for a host error or anything else thrown. */
export function messengerError(e: unknown): string {
  if (e != null && typeof e === 'object' && 'code' in e) {
    const err = e as MessengerError;
    return err.message ?? err.code;
  }
  return typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
}

export function messengerErrorCode(e: unknown): string | null {
  if (e != null && typeof e === 'object' && 'code' in e) return String((e as MessengerError).code);
  return null;
}

export const messengerApi = {
  async status(): Promise<MessengerStatus> {
    try {
      return await invoke<MessengerStatus>('messenger_status');
    } catch (e) {
      if (isUnknownCommand(e)) return NOT_COMPILED;
      throw e;
    }
  },
  setEnabled: (enabled: boolean) => invoke<void>('messenger_set_enabled', { enabled }),

  relays: {
    list: () => invoke<MessengerRelay[]>('messenger_relays_list'),
    add: (url: string, apiKey?: string) =>
      invoke<MessengerRelay>('messenger_relays_add', { url, apiKey: apiKey && apiKey.trim() ? apiKey.trim() : null }),
    remove: (url: string) => invoke<void>('messenger_relays_remove', { url }),
    setEnabled: (url: string, enabled: boolean) => invoke<void>('messenger_relays_set_enabled', { url, enabled }),
    setSilent: (enabled: boolean) => invoke<void>('messenger_relays_set_silent', { enabled }),
    manifestInfo: () => invoke<MessengerManifestInfo>('messenger_manifest_info'),
    setRegion: (region: string) => invoke<void>('messenger_manifest_set_region', { region }),
  },

  dm: {
    sendText: (to: string, text: string) => invoke<string>('messenger_dm_send_text', { to, text }),
  },

  profiles: {
    get: (pubkey: string) => invoke<MessengerProfile | null>('messenger_profile_get', { pubkey }),
    request: (pubkey: string) => invoke<void>('messenger_profile_request', { pubkey }),
    ownGet: () => invoke<MessengerProfile | null>('messenger_profile_own_get'),
    ownSet: (input: MessengerProfileInput) => invoke<MessengerProfile>('messenger_profile_own_set', { input }),
    verifyNip05: (pubkey: string) => invoke<boolean>('messenger_nip05_verify', { pubkey }),
  },

  contacts: {
    list: () => invoke<MessengerContact[]>('messenger_contacts_list'),
    add: (key: string, nickname?: string) =>
      invoke<MessengerContact>('messenger_contacts_add', { key, nickname: nickname?.trim() || null }),
    update: (pubkey: string, patch: MessengerContactPatch) =>
      invoke<MessengerContact>('messenger_contacts_update', { pubkey, patch }),
    remove: (pubkey: string) => invoke<void>('messenger_contacts_remove', { pubkey }),
    setFollowed: (pubkey: string, followed: boolean) =>
      invoke<void>('messenger_contacts_set_followed', { pubkey, followed }),
  },

  identity: {
    get: () => invoke<MessengerIdentity | null>('messenger_identity_get'),
    create: (password: string) => invoke<MessengerCreatedIdentity>('messenger_identity_create', { password }),
    import: (kind: IdentityImportKind, secret: string, password?: string) =>
      invoke<MessengerIdentity>('messenger_identity_import', { kind, secret, password: password ?? null }),
    export: (password: string) => invoke<string>('messenger_identity_export', { password }),
    delete: () => invoke<void>('messenger_identity_delete'),
  },
};
