// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import type {
  BulkImportResult,
  BulkProxyItem,
  CamoufoxStatus,
  CreateProfileRequest,
  CreateProxyRequest,
  CreateWorkspaceColumnRequest,
  CreateWorkspaceRequest,
  DiffResult,
  ExportOptions,
  FileEntry,
  HistoryFilter,
  MergeResult,
  Note,
  NoteCreateInput,
  NoteFilter,
  NoteFolder,
  NoteHistoryEntry,
  NoteListItem,
  NoteTag,
  NoteUpdateInput,
  PresetInfo,
  Profile,
  ProfileExport,
  ProfileRawData,
  Proxy,
  ProxyCheckResult,
  SftpSessionInfo,
  SshConnection,
  SshConnectionCreateInput,
  SshConnectionUpdateInput,
  SshKey,
  SshKeyGenerateInput,
  SshKeyImportInput,
  SshKeyUpdateInput,
  SshSessionInfo,
  TotpAddRequest,
  TotpCode,
  TransferItemInput,
  TransferKind,
  TotpEntry,
  TotpPreview,
  TotpUpdateRequest,
  UpdateProfileRequest,
  UpdateWorkspaceColumnRequest,
  UpdateWorkspaceRequest,
  Workspace,
  WorkspaceColumn,
  WorkspaceStats,
} from './types';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const devMocks: Record<string, unknown> = {
  fingerprint_presets: [
    { id: 'win10', label: 'Windows 10 / Chrome' },
    { id: 'win11', label: 'Windows 11 / Chrome' },
    { id: 'macos', label: 'macOS / Safari' },
    { id: 'linux', label: 'Linux / Firefox' },
  ],
  profiles_list: [],
  proxies_list: [],
  workspace_list: [],
  profiles_list_by_workspace: [],
  workspace_column_list: [],
  fs_home: '/home/dev',
  fs_list: [
    { name: 'projects', path: '/home/dev/projects', is_dir: true, is_symlink: false, size: 4096, mtime: Date.now(), mode: 0o40755, permissions: 'rwxr-xr-x', octal: '0755', owner: '1000', group: '1000' },
    { name: 'readme.txt', path: '/home/dev/readme.txt', is_dir: false, is_symlink: false, size: 1234, mtime: Date.now(), mode: 0o100644, permissions: 'rw-r--r--', octal: '0644', owner: '1000', group: '1000' },
  ],
  sftp_session_list: [],
};

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }
  console.warn(`[dev-browser] invoke('${cmd}')`, args ?? '');
  return (cmd in devMocks ? devMocks[cmd] : []) as T;
}

export const api = {
  profiles: {
    list: () => call<Profile[]>('profiles_list'),
    get: (id: string) => call<Profile | null>('profile_get', { id }),
    create: (req: CreateProfileRequest) => call<Profile>('profile_create', { req }),
    update: (id: string, req: UpdateProfileRequest) =>
      call<Profile>('profile_update', { id, req }),
    delete: (id: string) => call<void>('profile_delete', { id }),
    clone: (id: string) => call<Profile>('profile_clone', { id }),
    launch: (id: string) => call<number>('profile_launch', { id }),
    stop: (id: string) => call<void>('profile_stop', { id }),
    isRunning: (id: string) => call<boolean>('profile_is_running', { id }),
    runningIds: () => call<string[]>('profiles_running_ids'),
    listByWorkspace: (workspaceId: string) =>
      call<Profile[]>('profiles_list_by_workspace', { workspaceId }),
    setTags: (id: string, tags: string[]) =>
      call<void>('profile_set_tags', { id, tags }),
    moveToKanbanColumn: (profileId: string, targetTag: string, kanbanOrder: number) =>
      call<void>('profile_move_to_kanban_column', { profileId, targetTag, kanbanOrder }),
    rawData: (id: string) => call<ProfileRawData>('profile_raw_data', { id }),
    exportJson: (id: string, options: ExportOptions) =>
      call<string>('profile_export_json', { id, options }),
    exportZip: (id: string, options: ExportOptions, outputPath: string) =>
      call<void>('profile_export_zip', { id, options, outputPath }),
    importJson: (json: string, workspaceId?: string) =>
      call<Profile>('profile_import_json', { json, workspaceId }),
    importZip: (filePath: string, workspaceId?: string) =>
      call<Profile>('profile_import_zip', { filePath, workspaceId }),
    importZipData: (dataB64: string, workspaceId?: string) =>
      call<Profile>('profile_import_zip_data', { dataB64, workspaceId }),
    importCookies: (id: string, cookiesJson: string) =>
      call<{ count: number; domains: string[] }>('profile_import_cookies', { id, cookiesJson }),
    exportCookies: (id: string) =>
      call<string>('profile_export_cookies', { id }),
    exportCookiesToFile: (id: string, outputPath: string) =>
      call<void>('profile_export_cookies_to_file', { id, outputPath }),
    exportJsonToFile: (id: string, options: ExportOptions, outputPath: string) =>
      call<void>('profile_export_json_to_file', { id, options, outputPath }),
  },

  proxies: {
    list: () => call<Proxy[]>('proxies_list'),
    get: (id: string) => call<Proxy | null>('proxy_get', { id }),
    create: (req: CreateProxyRequest) => call<Proxy>('proxy_create', { req }),
    bulkCreate: (items: BulkProxyItem[]) =>
      call<BulkImportResult>('proxies_bulk_create', { items }),
    update: (id: string, req: CreateProxyRequest) =>
      call<Proxy>('proxy_update', { id, req }),
    delete: (id: string) => call<void>('proxy_delete', { id }),
    check: (id: string) => call<ProxyCheckResult>('proxy_check', { id }),
    trustFingerprint: (id: string, fingerprint: string, ip: string, country: string | null, city: string | null) =>
      call<void>('proxy_trust_fingerprint', { id, fingerprint, ip, country, city }),
  },

  workspaces: {
    list: () => call<Workspace[]>('workspace_list'),
    get: (id: string) => call<Workspace | null>('workspace_get', { id }),
    create: (req: CreateWorkspaceRequest) => call<Workspace>('workspace_create', { req }),
    update: (id: string, req: UpdateWorkspaceRequest) =>
      call<Workspace>('workspace_update', { id, req }),
    delete: (id: string, mode: 'move_to_default' | 'delete_all') =>
      call<void>('workspace_delete', { id, mode }),
    stats: (id: string) => call<WorkspaceStats>('workspace_stats', { id }),
    columns: {
      list: (workspaceId: string) =>
        call<WorkspaceColumn[]>('workspace_column_list', { workspaceId }),
      create: (workspaceId: string, req: CreateWorkspaceColumnRequest) =>
        call<WorkspaceColumn>('workspace_column_create', { workspaceId, req }),
      update: (id: string, req: UpdateWorkspaceColumnRequest) =>
        call<WorkspaceColumn>('workspace_column_update', { id, req }),
      delete: (id: string) => call<void>('workspace_column_delete', { id }),
    },
  },

  fingerprintPresets: () => call<PresetInfo[]>('fingerprint_presets'),

  camoufox: {
    status: () => call<CamoufoxStatus>('camoufox_status'),
    download: () => call<void>('camoufox_download'),
    downloadState: () =>
      call<{
        state: string;
        downloaded?: number;
        total?: number;
        percent?: number;
        version?: string;
        error?: string;
      }>('camoufox_download_state'),
    cancel: () => call<void>('camoufox_download_cancel'),
    latestVersion: () => call<string>('camoufox_latest_version'),
  },

  pwgen: {
    list: () => call<{ id: string; password: string; created_at: string }[]>('pwgen_history_list'),
    add: (password: string) =>
      call<{ id: string; password: string; created_at: string }>('pwgen_history_add', { password }),
    clear: () => call<void>('pwgen_history_clear'),
    trim: (limit: number) => call<void>('pwgen_history_trim', { limit }),
  },

  totp: {
    list: () => call<TotpEntry[]>('totp_list'),
    add: (req: TotpAddRequest) => call<TotpEntry>('totp_add', { req }),
    update: (id: string, req: TotpUpdateRequest) => call<TotpEntry>('totp_update', { id, req }),
    delete: (id: string) => call<void>('totp_delete', { id }),
    generateCode: (id: string) => call<TotpCode>('totp_generate_code', { id }),
    generateCodes: (ids: string[]) => call<TotpCode[]>('totp_generate_codes', { ids }),
    previewUri: (uri: string) => call<TotpPreview>('totp_preview_uri', { uri }),
  },

  notes: {
    list: (filter?: NoteFilter) => call<NoteListItem[]>('note_list', { filter: filter ?? {} }),
    get: (id: string) => call<Note>('note_get', { id }),
    create: (input: NoteCreateInput) => call<Note>('note_create', { input }),
    update: (id: string, input: NoteUpdateInput) => call<Note>('note_update', { id, input }),
    delete: (id: string, hard?: boolean) => call<void>('note_delete', { id, hard }),
    archive: (id: string) => call<void>('note_archive', { id }),
    restore: (id: string) => call<void>('note_restore', { id }),
    setTags: (id: string, tagNames: string[]) => call<void>('note_set_tags', { id, tagNames }),
    search: (query: string, filter?: NoteFilter) =>
      call<NoteListItem[]>('note_search', { query, filter: filter ?? {} }),
    sync: () => call<void>('note_sync'),
    reindex: () => call<void>('note_reindex'),
    openFolder: () => call<void>('note_open_folder'),
    openExternal: (id: string) => call<void>('note_open_external', { id }),
    draftSave: (id: string, content: string) => call<void>('note_draft_save', { id, content }),
    draftGet: (id: string) => call<string | null>('note_draft_get', { id }),
    draftDiscard: (id: string) => call<void>('note_draft_discard', { id }),
    tagList: () => call<NoteTag[]>('note_tag_list'),
    tagCreate: (name: string, color?: string) => call<NoteTag>('note_tag_create', { name, color }),
    tagDelete: (id: string) => call<void>('note_tag_delete', { id }),
    tagUpdate: (id: string, name?: string, color?: string) => call<NoteTag>('note_tag_update', { id, name, color }),
    folderList: () => call<NoteFolder[]>('note_folder_list'),
    folderCreate: (name: string, parent_id?: string, color?: string) => call<NoteFolder>('note_folder_create', { name, parentId: parent_id, color }),
    folderUpdate: (id: string, name?: string, color?: string) => call<NoteFolder>('note_folder_update', { id, name, color }),
    folderDelete: (id: string) => call<void>('note_folder_delete', { id }),
    noteAddFolder: (noteId: string, folderId: string) => call<void>('note_add_folder', { noteId, folderId }),
    noteRemoveFolder: (noteId: string, folderId: string) => call<void>('note_remove_folder', { noteId, folderId }),
    noteAddBinding: (noteId: string, binding: string) => call<void>('note_add_binding', { noteId, binding }),
    noteRemoveBinding: (noteId: string, binding: string) => call<void>('note_remove_binding', { noteId, binding }),
    getDir: () => call<{ current: string; is_custom: boolean }>('notes_get_dir'),
    setDir: (path: string | null) => call<{ current: string; is_custom: boolean }>('notes_set_dir', { path }),
    historyList: (noteId: string, filter?: HistoryFilter) =>
      call<NoteHistoryEntry[]>('note_history_list', { noteId, filter }),
    historyGet: (historyId: string) =>
      call<NoteHistoryEntry>('note_history_get', { historyId }),
    historyDiff: (noteId: string, fromId: string, toId: string) =>
      call<DiffResult>('note_history_diff', { noteId, fromId, toId }),
    historyRestore: (noteId: string, historyId: string) =>
      call<Note>('note_history_restore', { noteId, historyId }),
    historyMerge: (noteId: string, historyId: string) =>
      call<MergeResult>('note_history_merge', { noteId, historyId }),
  },

  ssh: {
    connectionList: (workspaceId?: string, profileId?: string, search?: string) =>
      call<SshConnection[]>('ssh_connection_list', { workspaceId, profileId, search }),
    connectionGet: (id: string) => call<SshConnection>('ssh_connection_get', { id }),
    connectionCreate: (input: SshConnectionCreateInput) =>
      call<SshConnection>('ssh_connection_create', { input }),
    connectionUpdate: (id: string, input: SshConnectionUpdateInput) =>
      call<SshConnection>('ssh_connection_update', { id, input }),
    connectionDelete: (id: string) => call<void>('ssh_connection_delete', { id }),
    connect: (connectionId: string) =>
      call<string>('ssh_connect', { connectionId }),
    disconnect: (sessionId: string) => call<void>('ssh_disconnect', { sessionId }),
    sendData: (sessionId: string, data: number[]) =>
      call<void>('ssh_send_data', { sessionId, data }),
    resize: (sessionId: string, cols: number, rows: number) =>
      call<void>('ssh_resize', { sessionId, cols, rows }),
    respondPrompt: (sessionId: string, response: string) =>
      call<void>('ssh_respond_prompt', { sessionId, response }),
    sessionList: () => call<SshSessionInfo[]>('ssh_session_list'),
    sessionRemove: (sessionId: string) => call<void>('ssh_session_remove', { sessionId }),
  },
  sshKeys: {
    list: () => call<SshKey[]>('ssh_key_list'),
    get: (id: string) => call<SshKey>('ssh_key_get', { id }),
    import: (input: SshKeyImportInput) => call<SshKey>('ssh_key_import', { input }),
    generate: (input: SshKeyGenerateInput) => call<SshKey>('ssh_key_generate', { input }),
    update: (id: string, input: SshKeyUpdateInput) =>
      call<SshKey>('ssh_key_update', { id, input }),
    delete: (id: string) => call<void>('ssh_key_delete', { id }),
  },
  sftp: {
    connect: (connectionId: string) =>
      call<SftpSessionInfo>('sftp_connect', { connectionId }),
    disconnect: (connectionId: string) =>
      call<void>('sftp_disconnect', { connectionId }),
    sessionList: () => call<SftpSessionInfo[]>('sftp_session_list'),
    home: (connectionId: string) => call<string>('sftp_home', { connectionId }),
    list: (connectionId: string, path: string) =>
      call<FileEntry[]>('sftp_list', { connectionId, path }),
    stat: (connectionId: string, path: string) =>
      call<FileEntry | null>('sftp_stat', { connectionId, path }),
    respondPrompt: (connectionId: string, response: string) =>
      call<void>('sftp_respond_prompt', { connectionId, response }),
    transferStart: (kind: TransferKind, connectionId: string, items: TransferItemInput[]) =>
      call<string>('sftp_transfer_start', { kind, connectionId, items }),
    transferCancel: (transferId: string) =>
      call<void>('sftp_transfer_cancel', { transferId }),
    mkdir: (connectionId: string, path: string) =>
      call<void>('sftp_mkdir', { connectionId, path }),
    createFile: (connectionId: string, path: string) =>
      call<void>('sftp_create_file', { connectionId, path }),
    rename: (connectionId: string, from: string, to: string) =>
      call<void>('sftp_rename', { connectionId, from, to }),
    delete: (connectionId: string, path: string) =>
      call<void>('sftp_delete', { connectionId, path }),
    chmod: (connectionId: string, path: string, mode: number) =>
      call<void>('sftp_chmod', { connectionId, path, mode }),
  },

  fs: {
    home: () => call<string>('fs_home'),
    list: (path: string) => call<FileEntry[]>('fs_list', { path }),
    stat: (path: string) => call<FileEntry | null>('fs_stat', { path }),
    mkdir: (path: string) => call<void>('fs_mkdir', { path }),
    createFile: (path: string) => call<void>('fs_create_file', { path }),
    rename: (from: string, to: string) => call<void>('fs_rename', { from, to }),
    delete: (path: string) => call<void>('fs_delete', { path }),
    chmod: (path: string, mode: number) => call<void>('fs_chmod', { path, mode }),
  },

  system: {
    openUrl: (url: string) => call<void>('open_url', { url }),
  },
};
