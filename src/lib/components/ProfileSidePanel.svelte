<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { tick } from 'svelte';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import type { Profile, Proxy, WorkspaceColumn } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Drawer from '$lib/components/ui/Drawer.svelte';
  import Modal from '$lib/Modal.svelte';
  import ExportProfileModal from '$lib/components/ExportProfileModal.svelte';
  import TotpPanel from '$lib/components/TotpPanel.svelte';
  import NotesPanel from '$lib/components/notes/NotesPanel.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { formatError } from '$lib/utils';
  import SshProfileTab from '$lib/components/ssh/SshProfileTab.svelte';

  interface Props {
    profile: Profile;
    proxy: Proxy | null;
    workspaceId: string;
    columns: WorkspaceColumn[];
    isRunning: boolean;
    onclose: () => void;
    onchange: () => void;
    onsync: (p: Profile) => void;
    onedit: (p: Profile) => void;
    onrawdata: (p: Profile) => void;
  }

  let { profile, proxy, workspaceId, columns, isRunning, onclose, onchange, onsync, onedit, onrawdata }: Props = $props();

  let activeTab = $state<'info' | 'totp' | 'notes' | 'ssh'>('info');
  let notesOpen = $state(false);
  let noteToOpen = $state<string | null>(null);

  $effect(() => {
    if (!notesOpen) noteToOpen = null;
  });

  const profileNotes = $derived(
    notesStore.list
      .filter((n) => n.bindings.includes(`profile:${profile.id}`) && !n.archived)
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
  );

  function relTime(iso: string): string {
    const diff = Date.now() - new Date(iso).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return $t('notes_time_just_now');
    if (m < 60) return $t('notes_time_m_ago', { m: String(m) });
    const h = Math.floor(m / 60);
    if (h < 24) return $t('notes_time_h_ago', { h: String(h) });
    const d = Math.floor(h / 24);
    if (d < 7) return $t('notes_time_d_ago', { d: String(d) });
    return new Date(iso).toLocaleDateString();
  }
  let actionLoading = $state(false);
  let deleteModal = $state(false);
  let exportModal = $state(false);
  let error = $state('');
  let cookieFileInput: HTMLInputElement | null = $state(null);
  let cookieImportResult = $state<{ count: number; domains: string[] } | null>(null);
  let exportingCookies = $state(false);

  const totpCount = $derived(totpStore.countForProfile(profile.id));

  // Column (single tag) assignment
  let tagsLoading = $state(false);
  const tagColorMap = $derived(new Map(columns.map((col) => [col.tag_name, col.color])));

  // Current column tag = first tag that matches a workspace column
  const currentColumnTag = $derived(
    (profile.tags ?? []).find((t) => columns.some((c) => c.tag_name === t)) ?? ''
  );

  async function setColumn(tagName: string) {
    tagsLoading = true;
    try {
      // Keep non-column tags, replace column tag with the new one
      const nonColumnTags = (profile.tags ?? []).filter(
        (t) => !columns.some((c) => c.tag_name === t)
      );
      const updated = tagName ? [...nonColumnTags, tagName] : nonColumnTags;
      await api.profiles.setTags(profile.id, updated);
      onsync({ ...profile, tags: updated });
    } catch (e) { error = formatError(e); }
    finally { tagsLoading = false; }
  }

  function formatDate(d: string | null) {
    if (!d) return $t('panel_never');
    return new Date(d).toLocaleString();
  }

  function getOsLabel(preset: string) {
    const map: Record<string, string> = {
      win10: 'Windows 10', win11: 'Windows 11', macos: 'macOS', linux: 'Linux',
    };
    return map[preset] ?? preset;
  }

  async function launch() {
    actionLoading = true; error = '';
    try {
      await api.profiles.launch(profile.id);
      onsync({ ...profile, status: 'running' });
    } catch (e) { error = formatError(e); }
    finally { actionLoading = false; }
  }

  async function stop() {
    actionLoading = true; error = '';
    try {
      await api.profiles.stop(profile.id);
      onsync({ ...profile, status: 'stopped' });
    } catch (e) { error = formatError(e); }
    finally { actionLoading = false; }
  }

  async function clone() {
    actionLoading = true; error = '';
    try {
      await api.profiles.clone(profile.id);
      onchange();
    } catch (e) { error = formatError(e); }
    finally { actionLoading = false; }
  }

  async function exportCookies() {
    exportingCookies = true;
    error = '';
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const safeName = profile.name.replace(/[^a-z0-9_-]/gi, '_');
      const path = await save({
        defaultPath: `${safeName}_cookies.json`,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!path) return;
      await api.profiles.exportCookiesToFile(profile.id, path);
    } catch (e) {
      error = formatError(e);
    } finally {
      exportingCookies = false;
    }
  }

  async function confirmDelete() {
    try {
      await api.profiles.delete(profile.id);
      deleteModal = false;
      await tick();
      onchange();
    } catch (e) { error = formatError(e); }
  }

  async function onCookieFileSelected(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    error = '';
    cookieImportResult = null;
    try {
      const text = await file.text();
      const result = await api.profiles.importCookies(profile.id, text);
      cookieImportResult = result;
    } catch (e) {
      error = formatError(e);
    } finally {
      if (cookieFileInput) cookieFileInput.value = '';
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
<Drawer open title={profile.name} {onclose}>
  {#snippet actions()}
    <span class="status-badge" class:running={isRunning}>
      {isRunning ? $t('status_running') : $t('status_stopped')}
    </span>
  {/snippet}
  {#snippet subheader()}
    <div class="tab-bar">
      <button class="tab" class:active={activeTab === 'info'} onclick={() => (activeTab = 'info')}>
        <Icon name="info" size={12} /> Info
      </button>
      <button class="tab" class:active={activeTab === 'totp'} onclick={() => (activeTab = 'totp')}>
        <Icon name="shield" size={12} /> TOTP
        {#if totpCount > 0}
          <span class="tab-count">{totpCount}</span>
        {/if}
      </button>
      <button class="tab" class:active={activeTab === 'notes'} onclick={() => { activeTab = 'notes'; notesStore.ensureLoaded(); }}>
        <Icon name="file-text" size={12} /> {$t('panel_tab_notes')}
      </button>
      <button class="tab" class:active={activeTab === 'ssh'} onclick={() => (activeTab = 'ssh')}>
        <Icon name="terminal" size={12} /> {$t('panel_tab_ssh')}
      </button>
    </div>
  {/snippet}
  <div class="psp-body">
      {#if activeTab === 'totp'}
        <TotpPanel profileId={profile.id} />
      {:else if activeTab === 'notes'}
        <div class="notes-inline">
          <div class="notes-inline-header">
            <span class="notes-count">{profileNotes.length === 1 ? $t('panel_notes_count_one', { n: String(profileNotes.length) }) : $t('panel_notes_count_many', { n: String(profileNotes.length) })}</span>
            <button class="btn-open-notes" onclick={() => (notesOpen = true)}>
              <Icon name="external-link" size={12} /> {$t('panel_notes_open')}
            </button>
          </div>
          {#if notesStore.loading}
            <div class="notes-empty">{$t('loading')}</div>
          {:else if profileNotes.length === 0}
            <div class="notes-empty">{$t('panel_notes_empty')}</div>
          {:else}
            <ul class="notes-list-inline">
              {#each profileNotes as note (note.id)}
                <li>
                  <button class="note-card-inline" onclick={() => { noteToOpen = note.id; notesOpen = true; }}>
                    <div class="note-card-top">
                      <span class="note-title-inline">{note.title || $t('notes_untitled')}</span>
                      <span class="note-badge">{note.format.toUpperCase()}</span>
                    </div>
                    {#if note.preview}
                      <span class="note-preview-inline">{note.preview}</span>
                    {/if}
                    <div class="note-card-bottom">
                      <span class="note-time-inline">{relTime(note.updated_at)}</span>
                      <span class="note-flags">
                        {#if note.pinned}<span class="note-flag">📌</span>{/if}
                        {#if note.has_draft}<span class="note-flag draft-flag">{$t('panel_notes_draft')}</span>{/if}
                      </span>
                    </div>
                    {#if note.tags.length > 0}
                      <div class="note-tags-inline">
                        {#each note.tags.slice(0, 3) as tag}
                          <span class="note-tag-chip" style="background:{tag.color}22;color:{tag.color}">{tag.name}</span>
                        {/each}
                        {#if note.tags.length > 3}<span class="note-tag-more">+{note.tags.length - 3}</span>{/if}
                      </div>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else if activeTab === 'ssh'}
        <SshProfileTab profileId={profile.id} {workspaceId} />
      {:else}

      {#if error}
        <div class="error-msg" style="margin-bottom:0.5rem">{error}</div>
      {/if}

      <div class="info-section">
        <div class="info-row">
          <span class="info-label">{$t('panel_os')}</span>
          <span class="info-value">
            <Icon name="monitor" size={12} />
            {getOsLabel(profile.fingerprint_preset)} / {profile.browser_type}
          </span>
        </div>

        <div class="info-row">
          <span class="info-label">{$t('panel_proxy')}</span>
          <span class="info-value" class:no-proxy={!proxy}>
            {#if proxy}
              <Icon name="globe" size={12} />
              {proxy.name}
              {#if proxy.country}
                <span class="country-badge">{proxy.country}</span>
              {/if}
            {:else}
              <Icon name="wifi-off" size={12} />
              {$t('panel_no_proxy')}
            {/if}
          </span>
        </div>

        <div class="info-row">
          <span class="info-label">Locale</span>
          <span class="info-value"><Icon name="globe" size={12} />{profile.locale}</span>
        </div>

        <div class="info-row">
          <span class="info-label">Screen</span>
          <span class="info-value">{profile.screen_width}×{profile.screen_height}</span>
        </div>

        <div class="info-row">
          <span class="info-label">{$t('panel_last_activity')}</span>
          <span class="info-value muted">{formatDate(profile.last_launch_at)}</span>
        </div>

        {#if profile.notes}
          <div class="notes-row">
            <Icon name="file-text" size={12} />
            <span>{profile.notes}</span>
          </div>
        {/if}
      </div>

      <!-- Column (tag) assignment -->
      <div class="tags-section">
        <div class="tags-label">Column</div>
        <div class="column-select-row">
          {#each columns as col}
            {@const active = currentColumnTag === col.tag_name}
            <button
              class="col-chip"
              class:active
              style="--col-color: {col.color}"
              disabled={tagsLoading}
              onclick={() => setColumn(active ? '' : col.tag_name)}
              title={active ? $t('panel_col_unassign') : `${$t('panel_col_assign')}: ${col.name}`}
            >
              {col.name}
            </button>
          {/each}
          {#if columns.length === 0}
            <span class="no-tags">{$t('panel_no_columns')}</span>
          {/if}
        </div>
      </div>

      <div class="panel-actions">
        {#if !isRunning}
          <button class="btn btn-success" disabled={actionLoading} onclick={launch}>
            {actionLoading ? '…' : $t('panel_btn_launch')}
          </button>
        {:else}
          <button class="btn btn-danger" disabled={actionLoading} onclick={stop}>
            {actionLoading ? '…' : $t('panel_btn_stop')}
          </button>
        {/if}

        <button class="btn btn-ghost" onclick={() => onedit(profile)}>
          <Icon name="pencil" size={13} />{$t('panel_btn_edit')}
        </button>
        <button class="btn btn-ghost" onclick={() => onrawdata(profile)}>
          <Icon name="code" size={13} />Raw Data
        </button>
        <button class="btn btn-ghost" disabled={actionLoading} onclick={clone}>
          <Icon name="copy" size={13} />{$t('panel_btn_clone')}
        </button>

        <button class="btn btn-ghost" onclick={() => (exportModal = true)}>
          <Icon name="upload" size={13} />{$t('panel_btn_export')}
        </button>

        <input
          bind:this={cookieFileInput}
          type="file"
          accept=".json,.txt"
          style="display:none"
          onchange={onCookieFileSelected}
        />
        <button
          class="btn btn-ghost"
          disabled={isRunning}
          title={isRunning ? $t('panel_cookie_import_blocked') : $t('panel_cookie_import_hint')}
          onclick={() => cookieFileInput?.click()}
        >
          <Icon name="cookie" size={13} />Import Cookies
        </button>

        <button
          class="btn btn-ghost"
          disabled={exportingCookies}
          title={$t('panel_btn_export_cookies_hint')}
          onclick={exportCookies}
        >
          <Icon name="download" size={13} />{exportingCookies ? '…' : $t('panel_btn_export_cookies')}
        </button>

        <button
          class="btn btn-ghost btn-delete"
          onclick={() => (deleteModal = true)}
        >
          <Icon name="trash-2" size={13} />{$t('panel_btn_delete')}
        </button>
      </div>

      {/if}
  </div>
</Drawer>

<Modal
  open={deleteModal}
  title={$t('profiles_btn_delete')}
  message={$t('profiles_confirm_delete', { name: profile.name })}
  confirmLabel={$t('profiles_btn_delete')}
  cancelLabel={$t('profile_btn_cancel')}
  variant="danger"
  onconfirm={confirmDelete}
  oncancel={() => (deleteModal = false)}
/>

<ExportProfileModal
  {profile}
  {proxy}
  open={exportModal}
  onclose={() => (exportModal = false)}
/>

{#if cookieImportResult}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div class="cookie-result-backdrop" onclick={(e) => { if (e.target === e.currentTarget) cookieImportResult = null; }} role="presentation" tabindex="-1">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_interactive_supports_focus -->
    <div class="cookie-result-modal" role="dialog" tabindex="-1">
      <div class="crm-header">
        <Icon name="check-circle" size={16} />
        <span>Cookies imported</span>
      </div>
      <div class="crm-count">{cookieImportResult.count} cookies</div>
      {#if cookieImportResult.domains.length > 0}
        <div class="crm-domains-label">Domains ({cookieImportResult.domains.length}{cookieImportResult.domains.length === 20 ? '+' : ''}):</div>
        <div class="crm-domains">
          {#each cookieImportResult.domains as d}
            <span class="crm-domain">{d}</span>
          {/each}
        </div>
      {/if}
      <button class="btn btn-ghost crm-ok" onclick={() => (cookieImportResult = null)}>OK</button>
    </div>
  </div>
{/if}

<NotesPanel
  bind:open={notesOpen}
  context="profile"
  contextId={profile.id}
  workspaceId={workspaceId}
  openNoteId={noteToOpen}
/>

<style>
  .notes-inline {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
  }
  .notes-inline-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--sp-1);
  }
  .notes-count {
    font-size: var(--fs-sm);
    color: var(--text-2);
  }
  .notes-empty {
    font-size: var(--fs-base);
    color: var(--text-2);
    padding: var(--sp-2) 0;
  }
  .notes-list-inline {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .note-card-inline {
    width: 100%;
    text-align: left;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: var(--sp-2) 0.65rem;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    transition: background 0.15s;
  }
  .note-card-inline:hover { background: var(--bg-3); }
  .note-card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
  }
  .note-title-inline {
    font-size: var(--fs-sm);
    font-weight: 500;
    color: var(--text-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }
  .note-badge {
    font-size: var(--fs-2xs);
    font-weight: 600;
    color: var(--text-2);
    background: var(--bg-3);
    border-radius: 3px;
    padding: 0.1rem 0.3rem;
    flex-shrink: 0;
  }
  .note-preview-inline {
    font-size: var(--fs-xs);
    color: var(--text-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    line-height: 1.4;
  }
  .note-card-bottom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
  }
  .note-time-inline {
    font-size: var(--fs-2xs);
    color: var(--text-3, var(--text-2));
  }
  .note-flags {
    display: flex;
    gap: var(--sp-1);
    align-items: center;
  }
  .note-flag {
    font-size: var(--fs-2xs);
  }
  .draft-flag {
    background: var(--warn, #f59e0b);
    color: #fff;
    border-radius: 3px;
    padding: 0.05rem 0.25rem;
    font-size: var(--fs-2xs);
    font-weight: 600;
  }
  .note-tags-inline {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem;
    margin-top: 0.1rem;
  }
  .note-tag-chip {
    font-size: var(--fs-2xs);
    border-radius: 3px;
    padding: 0.05rem 0.3rem;
    font-weight: 500;
  }
  .note-tag-more {
    font-size: var(--fs-2xs);
    color: var(--text-2);
  }
  .btn-open-notes {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    background: transparent;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--radius-sm);
    padding: 0.2rem 0.6rem;
    font-size: var(--fs-sm);
    cursor: pointer;
    transition: background 0.15s;
  }

  .btn-open-notes:hover { background: color-mix(in srgb, var(--accent) 15%, transparent); }

  .status-badge {
    display: inline-flex; align-items: center;
    font-size: var(--fs-2xs); font-weight: 600; padding: 0.18rem var(--sp-2);
    border-radius: 999px; text-transform: uppercase; letter-spacing: 0.04em;
    background: var(--surface-2); color: var(--text-2);
    border: 1px solid var(--border);
    width: fit-content;
  }
  .status-badge.running { background: var(--success-bg); color: var(--success-text); border-color: color-mix(in srgb, var(--success) 30%, transparent); }

  .psp-body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }

  .info-section { display: flex; flex-direction: column; gap: 0.625rem; }

  .info-row {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    font-size: var(--fs-sm);
  }

  .info-label { color: var(--text-3); min-width: 80px; flex-shrink: 0; padding-top: 1px; }

  .info-value {
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex-wrap: wrap;
  }
  .info-value.no-proxy { color: var(--text-3); }
  .info-value.muted { color: var(--text-2); }

  .country-badge {
    font-size: var(--fs-2xs);
    background: var(--surface-2);
    border: 1px solid var(--border);
    padding: 0 0.35rem;
    border-radius: 999px;
    color: var(--text-2);
  }

  .notes-row {
    display: flex;
    align-items: flex-start;
    gap: 0.4rem;
    font-size: var(--fs-base);
    color: var(--text-2);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) 0.65rem;
  }

  .panel-actions { display: flex; flex-direction: column; gap: 0.4rem; }
  .panel-actions .btn { width: 100%; justify-content: center; }
  .btn-delete { color: var(--danger-text) !important; }
  .btn-delete:hover:not(:disabled) { background: var(--danger-bg) !important; border-color: color-mix(in srgb, var(--danger) 35%, var(--border)) !important; }

  .cookie-result-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.45);
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .cookie-result-modal {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    padding: var(--sp-5) var(--sp-6);
    min-width: 260px;
    max-width: 340px;
    display: flex;
    flex-direction: column;
    gap: 0.65rem;
  }

  .crm-header {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-weight: 700;
    font-size: var(--fs-md);
    color: var(--success-text);
  }

  .crm-count {
    font-size: var(--fs-xl);
    font-weight: 800;
    color: var(--text);
    line-height: 1;
  }

  .crm-domains-label {
    font-size: var(--fs-xs);
    font-weight: 600;
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .crm-domains {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    max-height: 120px;
    overflow-y: auto;
  }

  .crm-domain {
    font-size: var(--fs-xs);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.1rem 0.4rem;
    color: var(--text-2);
    font-family: monospace;
  }

  .crm-ok {
    align-self: flex-end;
    margin-top: var(--sp-1);
  }

  /* Tags */
  .tags-section { display: flex; flex-direction: column; gap: 0.4rem; }
  .tags-label { font-size: var(--fs-xs); font-weight: 600; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.06em; }
  .no-tags { font-size: var(--fs-sm); color: var(--text-muted); }
  .column-select-row { display: flex; flex-wrap: wrap; gap: 0.3rem; }
  .col-chip {
    display: inline-flex; align-items: center;
    background: color-mix(in srgb, var(--col-color) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--col-color) 35%, transparent);
    color: var(--col-color);
    border-radius: 4px; padding: 0.15rem var(--sp-2);
    font-size: var(--fs-sm); cursor: pointer; transition: all 0.15s;
    opacity: 0.55;
  }
  .col-chip:hover { opacity: 0.85; }
  .col-chip.active {
    opacity: 1;
    background: color-mix(in srgb, var(--col-color) 22%, transparent);
    border-color: color-mix(in srgb, var(--col-color) 60%, transparent);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--col-color) 30%, transparent);
  }
  .col-chip:disabled { cursor: not-allowed; }
</style>
