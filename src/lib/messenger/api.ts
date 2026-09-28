// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Messenger command layer. Intentionally separate from `$lib/api` so the
// module can be lifted into a standalone app: this file is the only place
// that knows command names (docs/messenger-spec.md §4.6).

export interface MessengerRuntimeStatus {
  version: string;
  data_dir: string;
  schema_version: number;
  secrets_unlocked: boolean;
  identity_present: boolean;
}

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

const devMocks: Record<string, (args?: Record<string, unknown>) => unknown> = {
  messenger_status: () => ({
    compiled: true,
    enabled: true,
    runtime: {
      version: '0.1.0-dev',
      data_dir: '/home/dev/.local/share/net.veydan.space/VeydanSpace/messenger',
      schema_version: 1,
      secrets_unlocked: true,
      identity_present: mockIdentity !== null,
    },
    error: null,
  }),
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

  identity: {
    get: () => invoke<MessengerIdentity | null>('messenger_identity_get'),
    create: (password: string) => invoke<MessengerCreatedIdentity>('messenger_identity_create', { password }),
    import: (kind: IdentityImportKind, secret: string, password?: string) =>
      invoke<MessengerIdentity>('messenger_identity_import', { kind, secret, password: password ?? null }),
    export: (password: string) => invoke<string>('messenger_identity_export', { password }),
    delete: () => invoke<void>('messenger_identity_delete'),
  },
};
