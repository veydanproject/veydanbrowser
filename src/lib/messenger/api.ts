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


export type ChatMode =
  | 'full_chat' | 'first_contact' | 'request_sent' | 'request_received' | 'request_declined'
  | 'request_declined_by_me' | 'request_revoked_by_peer' | 'removed_by_peer' | 'both_removed'
  | 'mutual_reconnect' | 'blocked' | 'blocked_by_peer';

export interface MessengerChat {
  id: string;
  kind: 'dm' | string;
  peer_pubkey: string | null;
  peer_npub: string | null;
  title: string;
  picture: string | null;
  is_contact: boolean;
  is_muted: boolean;
  unread: number;
  last_message_at: number | null;
  last_preview: string | null;
  pinned: boolean;
  archived: boolean;
  mode: ChatMode;
  can_send: boolean;
}

export interface MessengerReplyPreview {
  id: string;
  sender_pubkey: string;
  text: string | null;
}

export type MessageStatus = 'queued' | 'sent' | 'failed' | 'received' | 'uploading' | 'paused';

export interface MessengerMessage {
  id: string;
  chat_id: string;
  direction: 'in' | 'out';
  status: MessageStatus;
  content_type: string;
  text: string | null;
  sender_pubkey: string;
  reply_to: MessengerReplyPreview | null;
  created_at: number;
  edited_at: number | null;
  deleted: boolean;
  failure_reason: string | null;
  media: Record<string, unknown> | null;
}


export type DmAction = 'request' | 'accept' | 'decline' | 'block' | 'unblock' | 'remove';

export interface MessengerRelation {
  peer_pubkey: string;
  mode: ChatMode;
  my_contact: 'none' | 'approved' | 'declined';
  blocked: boolean;
  peer_signal: 'none' | 'approved' | 'blocked' | 'left' | 'declined' | 'revoked';
  was_ever_mutual: boolean;
  can_send: boolean;
}

/** Stable refusal code inside an error (`dm_waiting_approval`, …), if any. */
export function dmErrorCode(e: unknown): string | null {
  const m = /\bdm_[a-z_]+\b/.exec(messengerError(e));
  return m ? m[0] : null;
}


export type MediaKind = 'image' | 'video' | 'audio' | 'file' | 'voice' | 'circle';

/** Attachment fields of a message as the UI needs them. */
export interface MessengerMedia {
  name: string;
  mime: string;
  size: number;
  kind: MediaKind;
  /** Present when the file is on this device. */
  local_path?: string;
  transfer_id?: string;
  /** Recordings: length and loudness outline (0..255 per bar). */
  duration_ms?: number;
  waveform?: number[];
}

export function mediaOf(m: MessengerMessage): MessengerMedia | null {
  const f = m.media;
  if (!f || typeof f.name !== 'string') return null;
  return {
    name: f.name,
    mime: typeof f.mime === 'string' ? f.mime : 'application/octet-stream',
    size: typeof f.size === 'number' ? f.size : 0,
    kind: (["image", "video", "audio", "file", "voice", "circle"] as const).includes(f.kind as MediaKind) ? (f.kind as MediaKind) : "file",
    local_path: typeof f.local_path === 'string' ? f.local_path : undefined,
    transfer_id: typeof f.transfer_id === 'string' ? f.transfer_id : undefined,
    duration_ms: typeof f.duration_ms === "number" ? f.duration_ms : undefined,
    waveform: Array.isArray(f.waveform) ? (f.waveform as unknown[]).filter((x): x is number => typeof x === "number") : undefined,
  };
}

export type TransferStatus = 'queued' | 'running' | 'paused' | 'done' | 'failed' | 'cancelled';

export interface MessengerTransfer {
  id: string;
  direction: 'up' | 'down';
  message_id: string | null;
  chat_id: string | null;
  file_name: string;
  mime: string;
  size: number;
  status: TransferStatus;
  done_bytes: number;
  attempts: number;
  failure_reason: string | null;
  local_path: string | null;
}

/** Payload of the `transfer.progress` runtime event. */
export interface MessengerTransferProgress {
  transfer_id: string;
  message_id: string | null;
  chat_id: string | null;
  direction: 'up' | 'down';
  status: TransferStatus;
  done_bytes: number;
  total_bytes: number;
  failure_reason: string | null;
  local_path: string | null;
}

export interface MessengerMediaServer {
  id: string;
  kind: 's3' | 'blossom';
  url: string;
  bucket: string | null;
  region: string | null;
  access_key: string | null;
  has_secret: boolean;
  priority: number;
  enabled: boolean;
  source: 'manifest' | 'user';
  public_base: string;
}

/** A voice message or a video circle as the recorder produced it. */
export interface MessengerRecording {
  kind: "voice" | "circle";
  mime: string;
  duration_ms: number;
  waveform?: number[];
  blob: Blob;
}

async function blobToBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let bin = "";
  const step = 0x8000;
  for (let i = 0; i < bytes.length; i += step) bin += String.fromCharCode(...bytes.subarray(i, i + step));
  return btoa(bin);
}

export interface MessengerMediaServerInput {
  id?: string | null;
  kind: 's3' | 'blossom';
  url: string;
  bucket?: string | null;
  region?: string | null;
  access_key?: string | null;
  secret_key?: string | null;
}

/** Stable failure code inside an error (`err.rate_limited`, …), if any. */
export function mediaErrorCode(e: unknown): string | null {
  const m = /\berr\.[a-z_]+\b/.exec(typeof e === 'string' ? e : messengerError(e));
  return m ? m[0] : null;
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

import { buildDemo, demoEnabled } from './devDemo';

const demo = !isTauri && demoEnabled() ? buildDemo() : null;
let mockIdentity: MessengerIdentity | null = demo?.identity ?? null;
let mockRelays: MessengerRelay[] = [
  { url: 'wss://relay.damus.io', relay_id: 'pub-damus', source: 'manifest', regions: ['default'], read: true, write: true, enabled: true, auth_type: 'api_key', state: 'connected' },
  { url: 'wss://nos.lol', relay_id: 'pub-nos', source: 'manifest', regions: ['default'], read: true, write: true, enabled: false, auth_type: null, state: 'disconnected' },
];
let mockSilent = false;
let mockRegion = 'default';
let mockContacts: MessengerContact[] = demo?.contacts ?? [];
let mockOwnProfile: MessengerProfile | null = demo?.ownProfile ?? null;
const emptyProfile = (pubkey: string): MessengerProfile => ({
  pubkey, npub: `npub1${pubkey.slice(0, 58)}`, name: null, display_name: null, about: null, picture: null, banner: null,
  website: null, nip05: null, lud16: null, nip05_verified: false, event_created_at: 0, fetched_at: 0,
});

let mockChats: MessengerChat[] = demo?.chats ?? [];
let mockMediaServers: MessengerMediaServer[] = [
  { id: 'veydan-node-1-s3', kind: 's3', url: 'https://node-1.veydan.net:9000', bucket: 'veydan-media', region: 'us-east-1', access_key: null, has_secret: false, priority: 10, enabled: true, source: 'manifest', public_base: 'https://node-1.veydan.net:9000/veydan-media' },
];
const mockMessages: Record<string, MessengerMessage[]> = demo?.messages ?? {};
function mockChat(peer: string): MessengerChat {
  const hex = peer.startsWith('npub') ? 'ef'.repeat(32) : peer;
  const id = `dm:${hex}`;
  let c = mockChats.find((x) => x.id === id);
  if (!c) {
    const contact = mockContacts.find((x) => x.pubkey === hex);
    c = { id, kind: 'dm', peer_pubkey: hex, peer_npub: `npub1${hex.slice(0, 58)}`, title: contact ? contactLabel(contact) : `npub1${hex.slice(0, 7)}…`, picture: null, is_contact: !!contact, is_muted: false, unread: 0, last_message_at: null, last_preview: null, pinned: false, archived: false, mode: 'full_chat', can_send: true };
    mockChats = [c, ...mockChats];
    mockMessages[id] = [];
  }
  return c;
}
function mockFind(id: string): MessengerMessage | undefined {
  return Object.values(mockMessages).flat().find((m) => m.id === id);
}

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
  messenger_chats_list: () => mockChats,
  messenger_chat_open: (a) => mockChat(String(a?.peer)),
  messenger_chat_messages: (a) => {
    const all = mockMessages[String(a?.chatId)] ?? [];
    const before = (a?.before as number | null) ?? Number.MAX_SAFE_INTEGER;
    return all.filter((m) => m.created_at < before).slice(-((a?.limit as number) ?? 50));
  },
  messenger_chat_mark_read: (a) => { mockChats = mockChats.map((c) => c.id === a?.chatId ? { ...c, unread: 0 } : c); },
  messenger_chat_set_pinned: (a) => { mockChats = mockChats.map((c) => c.id === a?.chatId ? { ...c, pinned: Boolean(a?.pinned) } : c); },
  messenger_chat_set_archived: (a) => { mockChats = mockChats.map((c) => c.id === a?.chatId ? { ...c, archived: Boolean(a?.archived) } : c); },
  messenger_chat_delete: (a) => { mockChats = mockChats.filter((c) => c.id !== a?.chatId); },
  messenger_dm_send_text: (a) => {
    const c = mockChat(String(a?.to));
    const now = Math.floor(Date.now() / 1000);
    const target = a?.replyTo ? mockFind(String(a.replyTo)) : undefined;
    const m: MessengerMessage = { id: `${Date.now().toString(16)}${Math.random().toString(16).slice(2)}`, chat_id: c.id, direction: 'out', status: 'sent', content_type: 'text', text: String(a?.text), sender_pubkey: 'ab'.repeat(32), reply_to: target ? { id: target.id, sender_pubkey: target.sender_pubkey, text: target.text } : null, created_at: now, edited_at: null, deleted: false, failure_reason: null, media: null };
    mockMessages[c.id] = [...(mockMessages[c.id] ?? []), m];
    mockChats = mockChats.map((x) => x.id === c.id ? { ...x, last_message_at: now, last_preview: m.text } : x);
    return m;
  },
  messenger_dm_edit: (a) => { const m = mockFind(String(a?.messageId)); if (m) { m.text = String(a?.text); m.edited_at = Math.floor(Date.now() / 1000); } return m; },
  messenger_dm_delete: (a) => { const m = mockFind(String(a?.messageId)); if (m) { m.deleted = true; m.text = null; } },
  messenger_dm_retry: () => undefined,
  messenger_media_servers: () => mockMediaServers,
  messenger_media_server_put: (a) => {
    const i = (a?.input ?? {}) as MessengerMediaServerInput;
    const id = i.id ?? `${i.kind}-${mockMediaServers.length + 1}`;
    const s: MessengerMediaServer = { id, kind: i.kind, url: i.url, bucket: i.bucket ?? null, region: i.region ?? (i.kind === 's3' ? 'us-east-1' : null), access_key: i.access_key ?? null, has_secret: !!i.secret_key, priority: 10, enabled: true, source: 'user', public_base: i.kind === 's3' ? `${i.url}/${i.bucket}` : i.url };
    mockMediaServers = [...mockMediaServers.filter((x) => x.id !== id), s];
    return s;
  },
  messenger_media_server_remove: (a) => { mockMediaServers = mockMediaServers.filter((x) => x.id !== a?.id); },
  messenger_media_server_set_enabled: (a) => { mockMediaServers = mockMediaServers.map((x) => (x.id === a?.id ? { ...x, enabled: Boolean(a?.enabled) } : x)); },
  messenger_media_server_check: () => undefined,
  messenger_dm_send_file: (a) => {
    const c = mockChat(String(a?.to));
    const now = Math.floor(Date.now() / 1000);
    const name = String(a?.path).split(/[\\/]/).pop() ?? 'file';
    const ext = name.split('.').pop()?.toLowerCase() ?? '';
    const kind = ['jpg', 'jpeg', 'png', 'gif', 'webp'].includes(ext) ? 'image' : ['mp4', 'webm'].includes(ext) ? 'video' : ['mp3', 'ogg', 'wav'].includes(ext) ? 'audio' : 'file';
    const m: MessengerMessage = { id: `local:${Date.now().toString(16)}`, chat_id: c.id, direction: 'out', status: 'sent', content_type: 'media', text: (a?.caption as string) ?? null, sender_pubkey: 'ab'.repeat(32), reply_to: null, created_at: now, edited_at: null, deleted: false, failure_reason: null, media: { name, mime: 'application/octet-stream', size: 1_234_567, kind, local_path: String(a?.path) } };
    mockMessages[c.id] = [...(mockMessages[c.id] ?? []), m];
    mockChats = mockChats.map((x) => (x.id === c.id ? { ...x, last_message_at: now, last_preview: `📎 ${name}` } : x));
    return m;
  },
  messenger_media_grant_access: () => undefined,
  messenger_dm_send_recording: (a) => {
    const c = mockChat(String(a?.to));
    const r = (a?.recording ?? {}) as { kind: MediaKind; mime: string; duration_ms: number; waveform: number[] | null; data_base64: string };
    const now = Math.floor(Date.now() / 1000);
    const m: MessengerMessage = { id: `local:${Date.now().toString(16)}`, chat_id: c.id, direction: "out", status: "sent", content_type: "media", text: null, sender_pubkey: "ab".repeat(32), reply_to: null, created_at: now, edited_at: null, deleted: false, failure_reason: null, media: { name: `${r.kind}.webm`, mime: r.mime, size: Math.round((r.data_base64.length * 3) / 4), kind: r.kind, duration_ms: r.duration_ms, waveform: r.waveform ?? undefined, local_path: "/dev/mock", mock_data: `data:${r.mime.split(";")[0]};base64,${r.data_base64}` } };
    mockMessages[c.id] = [...(mockMessages[c.id] ?? []), m];
    return m;
  },
  messenger_media_download: () => null,
  messenger_media_transfer: () => null,
  messenger_media_pause: () => undefined,
  messenger_media_resume: () => undefined,
  messenger_media_cancel: () => undefined,
  messenger_media_save_as: () => undefined,
  messenger_media_data_url: (a) => (mockFind(String(a?.messageId))?.media?.mock_data as string | undefined) ?? null,
  messenger_media_local_path: () => null,
  messenger_media_open: () => undefined,
  messenger_open_url: (a) => { window.open(String(a?.url), '_blank', 'noopener'); },
  messenger_dm_relation: (a) => {
    const c = mockChat(String(a?.peer));
    return { peer_pubkey: c.peer_pubkey, mode: c.mode, my_contact: 'approved', blocked: c.mode === 'blocked', peer_signal: 'approved', was_ever_mutual: true, can_send: c.can_send };
  },
  messenger_dm_action: (a) => {
    const c = mockChat(String(a?.peer));
    const next: Record<string, [ChatMode, boolean]> = {
      request: ['request_sent', false], accept: ['full_chat', true], decline: ['request_declined_by_me', false],
      block: ['blocked', false], unblock: ['full_chat', true], remove: ['mutual_reconnect', true],
    };
    const [mode, can_send] = next[String(a?.action)] ?? ['full_chat', true];
    mockChats = mockChats.map((x) => (x.id === c.id ? { ...x, mode, can_send } : x));
    return { peer_pubkey: c.peer_pubkey, mode, my_contact: 'approved', blocked: mode === 'blocked', peer_signal: 'approved', was_ever_mutual: true, can_send };
  },
  messenger_dm_blocked: () => mockChats.filter((c) => c.mode === 'blocked').map((c) => c.peer_pubkey),
  /** Dev only: put the open mock chat into a given screen mode. */
  messenger_dev_set_mode: (a) => {
    mockChats = mockChats.map((x) => (x.id === a?.chatId ? { ...x, mode: a?.mode as ChatMode, can_send: Boolean(a?.canSend) } : x));
  },
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
  /** Open an http(s) link in the system browser. */
  openUrl: (url: string) => invoke<void>('messenger_open_url', { url }),

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

  chats: {
    list: (includeArchived = false) => invoke<MessengerChat[]>('messenger_chats_list', { includeArchived }),
    open: (peer: string) => invoke<MessengerChat>('messenger_chat_open', { peer }),
    messages: (chatId: string, before?: number, limit = 50) =>
      invoke<MessengerMessage[]>('messenger_chat_messages', { chatId, before: before ?? null, limit }),
    markRead: (chatId: string) => invoke<void>('messenger_chat_mark_read', { chatId }),
    setPinned: (chatId: string, pinned: boolean) => invoke<void>('messenger_chat_set_pinned', { chatId, pinned }),
    setArchived: (chatId: string, archived: boolean) => invoke<void>('messenger_chat_set_archived', { chatId, archived }),
    delete: (chatId: string) => invoke<void>('messenger_chat_delete', { chatId }),
  },

  dm: {
    sendText: (to: string, text: string, replyTo?: string) =>
      invoke<MessengerMessage>('messenger_dm_send_text', { to, text, replyTo: replyTo ?? null }),
    edit: (messageId: string, text: string) => invoke<MessengerMessage>('messenger_dm_edit', { messageId, text }),
    delete: (messageId: string, forEveryone: boolean) => invoke<void>('messenger_dm_delete', { messageId, forEveryone }),
    retry: (messageId: string) => invoke<void>('messenger_dm_retry', { messageId }),
    relation: (peer: string) => invoke<MessengerRelation>('messenger_dm_relation', { peer }),
    action: (peer: string, action: DmAction) => invoke<MessengerRelation>('messenger_dm_action', { peer, action }),
    blocked: () => invoke<string[]>('messenger_dm_blocked'),
  },

  media: {
    servers: () => invoke<MessengerMediaServer[]>('messenger_media_servers'),
    putServer: (input: MessengerMediaServerInput) => invoke<MessengerMediaServer>('messenger_media_server_put', { input }),
    removeServer: (id: string) => invoke<void>('messenger_media_server_remove', { id }),
    setServerEnabled: (id: string, enabled: boolean) => invoke<void>('messenger_media_server_set_enabled', { id, enabled }),
    checkServer: (id: string) => invoke<void>('messenger_media_server_check', { id }),
    sendRecording: async (to: string, rec: MessengerRecording) =>
      invoke<MessengerMessage>('messenger_dm_send_recording', {
        to,
        recording: {
          kind: rec.kind, mime: rec.mime, duration_ms: Math.round(rec.duration_ms),
          waveform: rec.waveform ?? null, data_base64: await blobToBase64(rec.blob),
        },
      }),
    /** Must run before the first `getUserMedia` (desktop webviews deny otherwise). */
    grantAccess: () => invoke<void>('messenger_media_grant_access'),
    sendFile: (to: string, path: string, caption?: string) =>
      invoke<MessengerMessage>('messenger_dm_send_file', { to, path, caption: caption?.trim() || null }),
    download: (messageId: string, manual: boolean) => invoke<string | null>('messenger_media_download', { messageId, manual }),
    transfer: (messageId: string) => invoke<MessengerTransfer | null>('messenger_media_transfer', { messageId }),
    pause: (transferId: string) => invoke<void>('messenger_media_pause', { transferId }),
    resume: (transferId: string) => invoke<void>('messenger_media_resume', { transferId }),
    cancel: (transferId: string) => invoke<void>('messenger_media_cancel', { transferId }),
    saveAs: (messageId: string, dest: string) => invoke<void>('messenger_media_save_as', { messageId, dest }),
    dataUrl: (messageId: string) => invoke<string | null>('messenger_media_data_url', { messageId }),
    localPath: (messageId: string) => invoke<string | null>('messenger_media_local_path', { messageId }),
    /** Opens with the default application; runnable files are only revealed in their folder. */
    open: (messageId: string) => invoke<void>('messenger_media_open', { messageId }),
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
export const isTauriHost = isTauri;
