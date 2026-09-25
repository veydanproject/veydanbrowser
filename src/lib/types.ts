// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

export type AppError = {
  code: 'db' | 'io' | 'browser' | 'proxy' | 'not_found' | 'conflict_changed' | 'vault_locked' | 'vault_mismatch' | 'decrypt_failed' | 'recovery_invalid' | 'other';
  message: string;
};

export interface Workspace {
  id: string;
  name: string;
  description: string | null;
  color: string;
  icon: string;
  notes: string | null;
  is_default: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateWorkspaceRequest {
  name: string;
  description?: string | null;
  color?: string;
  icon?: string;
}

export interface UpdateWorkspaceRequest {
  name?: string;
  description?: string | null;
  color?: string;
  icon?: string;
  notes?: string | null;
}

export interface WorkspaceStats {
  id: string;
  profile_count: number;
  proxy_count: number;
  active_count: number;
}

export interface WorkspaceColumn {
  id: string;
  workspace_id: string;
  name: string;
  tag_name: string;
  color: string;
  position: number;
  created_at: string;
}

export interface CreateWorkspaceColumnRequest {
  name: string;
  tag_name: string;
  color?: string;
}

export interface UpdateWorkspaceColumnRequest {
  name?: string;
  color?: string;
  position?: number;
}

export interface Profile {
  id: string;
  name: string;
  status: 'stopped' | 'running';
  profile_path: string;
  browser_type: string;
  proxy_id: string | null;
  fingerprint_preset: string;
  user_agent: string | null;
  platform: string | null;
  timezone: string | null;
  locale: string;
  languages: string;
  screen_width: number;
  screen_height: number;
  webrtc_mode: string;
  geolocation_enabled: boolean;
  latitude: number | null;
  longitude: number | null;
  webgl_vendor: string | null;
  webgl_renderer: string | null;
  notes: string | null;
  workspace_id: string | null;
  kanban_status: string;
  kanban_order: number;
  tags: string[];
  default_search_engine: string;
  history_enabled: boolean;
  created_at: string;
  updated_at: string;
  last_launch_at: string | null;
}

export interface CreateProfileRequest {
  name: string;
  workspace_id?: string;
  browser_type?: string;
  proxy_id?: string | null;
  fingerprint_preset?: string;
  user_agent?: string | null;
  platform?: string | null;
  timezone?: string | null;
  locale?: string;
  languages?: string;
  screen_width?: number;
  screen_height?: number;
  webrtc_mode?: string;
  geolocation_enabled?: boolean;
  latitude?: number | null;
  longitude?: number | null;
  webgl_vendor?: string | null;
  webgl_renderer?: string | null;
  notes?: string | null;
  default_search_engine?: string;
  history_enabled?: boolean;
}

export interface UpdateProfileRequest extends Partial<Omit<CreateProfileRequest, 'workspace_id'>> {}

export interface Proxy {
  id: string;
  name: string;
  proxy_type: string;
  host: string;
  port: number;
  username: string | null;
  /** Secrets stay in the backend; only their presence is reported. */
  has_password: boolean;
  country: string | null;
  city: string | null;
  status: 'unknown' | 'active' | 'failed';
  last_ip: string | null;
  last_check_at: string | null;
  tags: string[];
  has_private_key: boolean;
  server_fingerprint: string | null;
  created_at: string;
}

export interface CreateProxyRequest {
  name: string;
  proxy_type: string;
  host: string;
  port: number;
  tags?: string[];
  username?: string | null;
  password?: string | null;
  country?: string | null;
  city?: string | null;
  private_key?: string | null;
}

export interface BulkProxyItem {
  line_number: number;
  proxy_type: string;
  host: string;
  port: number;
  username?: string | null;
  password?: string | null;
}

export interface BulkImportRowResult {
  line_number: number;
  status: 'imported' | 'duplicate' | 'error';
  message?: string;
  id?: string;
}

export interface BulkImportResult {
  rows: BulkImportRowResult[];
  imported: Proxy[];
}

export interface ProxyCheckResult {
  ip: string;
  country: string | null;
  city: string | null;
  ok: boolean;
  ssh_fingerprint?: string | null;
  ssh_fingerprint_is_new?: boolean | null;
}

export interface PresetInfo {
  id: string;
  label: string;
}

export interface CookieEntry {
  host: string;
  name: string;
  value: string;
  path: string;
  expiry: number | null;
  secure: boolean;
  http_only: boolean;
}

export interface ProfileRawData {
  user_agent: string;
  platform: string;
  locale: string;
  languages: string;
  timezone: string | null;
  screen_width: number;
  screen_height: number;
  webrtc_mode: string;
  webgl_vendor: string | null;
  webgl_renderer: string | null;
  canvas_seed: number;
  audio_seed: number;
  fonts_seed: number;
  geolocation_enabled: boolean;
  latitude: number | null;
  longitude: number | null;
  camoufox_config: string;
  user_js: string;
  cookies: CookieEntry[];
}

export interface CamoufoxStatus {
  installed: boolean;
  version: string | null;
  camoufox_tag: string | null;
  path: string | null;
}

// ── TOTP ──────────────────────────────────────────────────────────────────────

export interface TotpEntry {
  id: string;
  name: string;
  issuer: string | null;
  algorithm: string;
  digits: number;
  period: number;
  tags: string[];
  created_at: string;
  updated_at: string;
  last_used_at: string | null;
}

export interface TotpCode {
  id: string;
  code: string;
  seconds_left: number;
}

export interface TotpPreview {
  name: string;
  issuer: string | null;
  secret_masked: string;
  algorithm: string;
  digits: number;
  period: number;
}

export interface TotpAddRequest {
  name: string;
  issuer?: string | null;
  secret?: string | null;
  uri?: string | null;
  algorithm?: string;
  digits?: number;
  period?: number;
  tags: string[];
}

export interface TotpUpdateRequest {
  name?: string;
  issuer?: string | null;
  tags?: string[];
}

// ── Passwords ─────────────────────────────────────────────────────────────────

export interface PasswordEntry {
  id: string;
  title: string;
  username: string | null;
  url: string | null;
  has_note: boolean;
  totp_ids: string[];
  tags: string[];
  created_at: string;
  updated_at: string;
}

export interface PasswordCreateRequest {
  title: string;
  username?: string | null;
  url?: string | null;
  password: string;
  note?: string | null;
  totp_ids?: string[];
  tags?: string[];
}

export interface PasswordUpdateRequest {
  title?: string;
  username?: string | null;
  url?: string | null;
  password?: string | null;
  note?: string | null;
  clear_note?: boolean;
  totp_ids?: string[];
  tags?: string[];
}

export interface RevealedSecret {
  value: string;
}

// ── Notes ─────────────────────────────────────────────────────────────────────

export type NoteFormat = 'md' | 'txt' | 'py' | string;
export type NoteDocStatus = 'active' | 'orphan' | 'missing';
export type SaveStatus = 'saved' | 'saving' | 'unsaved' | 'failed' | 'external';

export interface NoteTagInfo {
  id: string;
  name: string;
  color: string;
}

export interface NoteTag {
  id: string;
  name: string;
  color: string;
  created_at: string;
  updated_at: string;
}

export interface NoteFolder {
  id: string;
  name: string;
  parent_id: string | null;
  color: string;
  created_at: string;
  updated_at: string;
}

export interface NoteListItem {
  id: string;
  title: string;
  format: NoteFormat;
  /** Context bindings, e.g. ["workspace:id", "profile:id"] */
  bindings: string[];
  tags: NoteTagInfo[];
  folder_ids: string[];
  pinned: boolean;
  archived: boolean;
  deleted: boolean;
  doc_status: NoteDocStatus;
  created_at: string;
  updated_at: string;
  has_draft: boolean;
  preview: string;
  snippet?: string;
}

export interface Note extends NoteListItem {
  file_path: string;
  /** Absolute dir of the note file; relative attachment links resolve against it */
  base_dir: string;
  content_hash: string | null;
  content: string | null;
}

export interface NoteAttachment {
  name: string;
  /** `attachments/{note_id}/{name}` — relative to the note file */
  rel_path: string;
  size: number;
  is_image: boolean;
  /** False for a chunked file that is still only in the vault. */
  present: boolean;
}

export interface OrphanAttachment {
  note_id: string;
  name: string;
  size: number;
}

/** Browser capture: domain -> folder / tags */
export interface CaptureRule {
  domain: string;
  folder_id: string | null;
  tags: string[];
  template_id?: string | null;
}

export interface NoteCreateInput {
  title: string;
  format?: NoteFormat;
  /** Context bindings, e.g. ["workspace:id", "profile:id"] */
  bindings?: string[];
  tag_names?: string[];
  content?: string;
  /** Note from the Templates folder whose rendered body becomes the content */
  template_id?: string;
}

export interface NoteUpdateInput {
  title?: string;
  content?: string;
  pinned?: boolean;
  /** Hash of the body this editor loaded. Sent with content so a stale buffer is rejected. */
  base_hash?: string | null;
}

/** List filter; also the persisted condition set of a smart view */
export interface NoteFilter {
  /** Filter notes that contain this binding, e.g. "workspace:id" or "profile:id" */
  binding?: string;
  tag_name?: string;
  /** Folder and all of its descendants */
  folder_id?: string;
  pinned?: boolean;
  /** undefined = any, true/false = only that state */
  archived?: boolean;
  /** true = trash only; otherwise live notes */
  deleted?: boolean;
  /** Tag `name` or any `name/...` sub-tag */
  tag_prefix?: string;
  /** Notes without workspace/profile bindings */
  global_only?: boolean;
  bindings_any?: string[];
  tags_any?: string[];
  tags_all?: string[];
  updated_within_days?: number;
  has_attachments?: boolean;
  has_open_tasks?: boolean;
}

export interface NoteLockStatus {
  enabled: boolean;
  locked: boolean;
  /** Inactivity minutes before auto-lock; 0 disables auto-lock */
  timeout_min: number;
  /** `none` until the first password, `ok` when the vault exists, `mismatch` when the lock cannot unwrap it */
  vault: 'none' | 'ok' | 'mismatch';
  kind: LockKind;
  hint: string | null;
  /** A recovery key wraps the vault; a lost PIN/password can be replaced with it. */
  has_recovery: boolean;
}

export type LockKind = 'pin' | 'password';

export interface LockSecret {
  password: string;
  kind: LockKind;
  hint: string | null;
}

/** Lock status plus the recovery key when one was just created; it is shown once. */
export interface LockSetResult extends NoteLockStatus {
  recovery_key: string | null;
}

export interface NoteSmartView {
  id: string;
  name: string;
  color: string;
  conditions: NoteFilter;
  sort_order: number;
}

export interface SmartViewInput {
  name: string;
  color?: string;
  conditions: NoteFilter;
}

// ── Notes navigation tree (counts + catalogs, one call) ──────────────────────

export interface NavChild {
  id: string;
  name: string;
  color: string;
  count: number;
  parent_id?: string | null;
  profiles?: NavChild[];
}

export interface NavTag extends NoteTagInfo {
  count: number;
}

export interface NoteNav {
  counts: { all: number; global: number; pinned: number; archived: number; trash: number };
  /** Workspaces with notes; profiles nested under their workspace. */
  workspaces: NavChild[];
  folders: NavChild[];
  tags: NavTag[];
  smart: NavChild[];
  sites: NavChild[];
  /** Full catalogs for pickers, no counts. */
  all_workspaces: NavChild[];
  all_profiles: NavChild[];
}

export interface NoteLinks {
  outgoing: NoteListItem[];
  backlinks: NoteListItem[];
  /** Link targets in the body that match no existing note */
  unresolved: string[];
}

/** Public description of the entity behind a `kind:id` binding */
export interface BindingSummary {
  binding: string;
  kind: string;
  name: string;
  subtitle: string;
}

export interface NoteSyncInfo {
  /** The vault knows this note. */
  tracked: boolean;
  /** Local edits not yet pushed. */
  pending: boolean;
}

export interface HostInfo {
  os: string;
  arch: string;
  version: string;
}

// ── Note History ──────────────────────────────────────────────────────────────

export type VersionType = 'save' | 'autosave' | 'restore' | 'merge' | 'import' | 'sync' | 'conflict';

export interface NoteHistoryEntry {
  id: string;
  note_id: string;
  parent_id: string | null;
  revision: number;
  version_type: VersionType;
  title: string;
  content?: string;
  content_hash: string;
  author: string | null;
  device: string | null;
  created_at: string;
}

export interface MergeBlock {
  kind: 'normal' | 'conflict';
  /** Normal block text (keeps its trailing newline) */
  text: string;
  ours: string;
  theirs: string;
}

export interface MergeResult {
  content: string;
  has_conflicts: boolean;
  blocks: MergeBlock[];
}

/** Sync conflict snapshot; `token` must be sent back with the resolution. */
export interface ConflictView {
  token: string;
  merge: MergeResult;
  /** This device's sync name. Empty when the user has not set one. */
  local_device: string;
  /** Other device's sync name. Empty when the name is unknown. */
  remote_device: string;
}

export interface DiffLine {
  kind: 'context' | 'added' | 'removed';
  content: string;
}

export interface DiffResult {
  lines: DiffLine[];
}

export interface HistoryFilter {
  version_type?: VersionType;
  date_from?: string;
  date_to?: string;
}

// ── SSH ───────────────────────────────────────────────────────────────────────

export type SshAuthType = 'password' | 'key' | 'key_password';
export type SshStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

export interface SshConnection {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type: SshAuthType;
  /** Secrets never leave the backend; these flags say whether one is stored. */
  has_password: boolean;
  has_private_key: boolean;
  has_key_passphrase: boolean;
  ssh_key_id: string | null;
  requires_2fa: boolean;
  totp_entry_id: string | null;
  proxy_id: string | null;
  workspace_ids: string[];
  profile_ids: string[];
  connect_timeout_sec: number;
  keepalive_sec: number;
  terminal_theme: string | null;
  default_cols: number;
  default_rows: number;
  /** SHA256 host-key fingerprint pinned on first successful connect (TOFU). */
  server_fingerprint: string | null;
  last_connected_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface SshConnectionCreateInput {
  name: string;
  host: string;
  port?: number;
  username: string;
  auth_type: SshAuthType;
  password?: string | null;
  private_key?: string | null;
  key_passphrase?: string | null;
  ssh_key_id?: string | null;
  requires_2fa?: boolean;
  totp_entry_id?: string | null;
  proxy_id?: string | null;
  workspace_ids?: string[];
  profile_ids?: string[];
  connect_timeout_sec?: number;
  keepalive_sec?: number;
  terminal_theme?: string | null;
  default_cols?: number;
  default_rows?: number;
}

export interface SshConnectionUpdateInput {
  name?: string;
  host?: string;
  port?: number;
  username?: string;
  auth_type?: SshAuthType;
  password?: string | null;
  private_key?: string | null;
  key_passphrase?: string | null;
  ssh_key_id?: string | null;
  requires_2fa?: boolean;
  totp_entry_id?: string | null;
  proxy_id?: string | null;
  workspace_ids?: string[];
  profile_ids?: string[];
  connect_timeout_sec?: number;
  keepalive_sec?: number;
  terminal_theme?: string | null;
  default_cols?: number;
  default_rows?: number;
}

export interface SshSessionInfo {
  session_id: string;
  connection_id: string;
  connection_name: string;
  host: string;
  port: number;
  status: SshStatus;
  error: string | null;
  connected_at: string | null;
}

// ── SSH keys store ─────────────────────────────────────────────────────────────

export interface SshKey {
  id: string;
  name: string;
  algorithm: string;
  bits: number | null;
  comment: string | null;
  public_key: string;
  /** Private material is fetched on demand via `api.sshKeys.exportPrivate`. */
  has_passphrase: boolean;
  fingerprint: string | null;
  source: 'generated' | 'imported';
  created_at: string;
  updated_at: string;
  usage_count: number;
}

export interface SshKeyImportInput {
  name: string;
  private_key: string;
  passphrase?: string | null;
}

export interface SshKeyGenerateInput {
  name: string;
  algorithm: 'ed25519' | 'rsa' | 'ecdsa';
  bits?: number | null;
  comment?: string | null;
  passphrase?: string | null;
}

export interface SshKeyUpdateInput {
  name?: string;
  comment?: string | null;
}

// ── File browser (SFTP + local) ───────────────────────────────────────────────

/** Unified directory entry — same shape for local FS and remote SFTP. */
export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  /** Epoch milliseconds. */
  mtime: number | null;
  mode: number;
  /** "rwxr-xr-x" */
  permissions: string;
  /** "0644" */
  octal: string;
  owner: string | null;
  group: string | null;
}

export interface SftpSessionInfo {
  connection_id: string;
  connection_name: string;
  home: string;
  connected_at: string;
}

export type TransferKind = 'download' | 'upload';

export interface TransferItemInput {
  src_path: string;
  dst_path: string;
  overwrite: boolean;
}

export interface TransferProgressEvent {
  transfer_id: string;
  kind: TransferKind;
  current_file: string;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
}

export interface TransferDoneEvent {
  transfer_id: string;
  kind: TransferKind;
  error: string | null;
  cancelled: boolean;
  files_done: number;
  files_skipped: number;
}

// ── Export / Import ───────────────────────────────────────────────────────────

export interface ExportOptions {
  include_proxy: boolean;
  include_proxy_password: boolean;
  include_files: boolean;
}

export interface ProfileExportData {
  name: string;
  browser_type: string;
  fingerprint_preset: string;
  user_agent: string | null;
  platform: string | null;
  timezone: string | null;
  locale: string;
  languages: string;
  screen_width: number;
  screen_height: number;
  webrtc_mode: string;
  geolocation_enabled: boolean;
  latitude: number | null;
  longitude: number | null;
  webgl_vendor: string | null;
  webgl_renderer: string | null;
  notes: string | null;
  kanban_status: string;
  tags: string[];
}

export interface ProxyExportData {
  name: string;
  proxy_type: string;
  host: string;
  port: number;
  username: string | null;
  password: string | null;
  country: string | null;
  city: string | null;
}

export interface ProfileExport {
  version: string;
  exported_at: string;
  profile: ProfileExportData;
  proxy: ProxyExportData | null;
}
