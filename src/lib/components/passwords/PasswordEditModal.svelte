<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import { passwordErrorKey } from '$lib/password-error';
  import { generatePassword, pwSettings } from '$lib/password-gen';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import { totpStore } from '$lib/store/totp.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { notesLock } from '$lib/store/notes-lock.svelte';
  import { isSystemTag, mergeTags, systemTags, userLabels } from '$lib/entity-tags';
  import { parseBinding, isEntityKind } from '$lib/bindings';
  import { entitySummary } from '$lib/notes-context';
  import ChipMark from '$lib/components/notes/ChipMark.svelte';
  import type { PasswordEntry } from '$lib/types';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    entry?: PasswordEntry | null;
    initialTags?: string[];
    onclose: () => void;
  }

  let { entry = null, initialTags = [], onclose }: Props = $props();

  let title = $state('');
  let username = $state('');
  let url = $state('');
  let password = $state('');
  let note = $state('');
  let noteTouched = $state(false);
  let totpIds = $state<string[]>([]);
  let totpQuery = $state('');
  let totpSuggest = $state(false);
  let addTotp = $state(false);
  let totpName = $state('');
  let totpIssuer = $state('');
  let totpSecret = $state('');
  let tags = $state<string[]>([]);
  let query = $state('');
  let suggest = $state(false);
  let showPassword = $state(false);
  let error = $state('');
  let saving = $state(false);

  const editing = $derived(entry !== null);
  const q = $derived(query.trim().toLowerCase());
  const chosen = $derived(new Set(tags));
  const tagHits = $derived(
    notesStore.allTags.filter((tag) => !userLabels(tags).includes(tag.name) && (!q || tag.name.toLowerCase().includes(q))).slice(0, 8),
  );
  const profileHits = $derived(
    profilesStore.list.filter((profile) => !chosen.has(`profile:${profile.id}`) && (!q || profile.name.toLowerCase().includes(q))).slice(0, 6),
  );
  const workspaceHits = $derived(
    workspacesStore.list.filter((ws) => !chosen.has(`workspace:${ws.id}`) && (!q || ws.name.toLowerCase().includes(q))).slice(0, 6),
  );
  const noteHits = $derived(
    q
      ? notesStore.list.filter((n) => !n.deleted && !chosen.has(`note:${n.id}`) && n.title.toLowerCase().includes(q)).slice(0, 6)
      : [],
  );

  onMount(() => {
    void totpStore.ensureLoaded();
    void profilesStore.ensureLoaded();
    void workspacesStore.ensureLoaded();
    void notesStore.ensureLoaded();
    if (entry) {
      title = entry.title;
      username = entry.username ?? '';
      url = entry.url ?? '';
      totpIds = [...entry.totp_ids];
      tags = [...entry.tags];
      if (entry.has_note) {
        api.passwords.reveal(entry.id, 'note').then((result) => { note = result.value; }).catch(() => {});
      }
    } else {
      tags = [...initialTags];
    }
  });

  function bindingKind(tag: string): string {
    return parseBinding(tag)?.kind ?? 'tag';
  }

  function bindingLabel(tag: string): string {
    const parsed = parseBinding(tag);
    if (!parsed) return tag;
    if (parsed.kind === 'note') return notesStore.list.find((n) => n.id === parsed.value)?.title ?? parsed.value;
    if (isEntityKind(parsed.kind)) return entitySummary(parsed.kind, parsed.value)?.name ?? parsed.value;
    return parsed.value;
  }

  function addLabel(name: string) {
    const value = name.trim();
    if (!value || isSystemTag(value)) return;
    tags = mergeTags([], systemTags(tags), [...userLabels(tags), value]);
    query = '';
  }

  function addBinding(value: string) {
    if (!value || !isSystemTag(value) || tags.includes(value)) return;
    tags = mergeTags([], [...systemTags(tags), value], userLabels(tags));
    query = '';
  }

  async function commitQuery() {
    const value = query.trim();
    if (!value) return;
    try {
      const existing = notesStore.allTags.find((tag) => tag.name.toLowerCase() === value.toLowerCase());
      if (existing) {
        addLabel(existing.name);
        return;
      }
      const profile = profilesStore.list.filter((item) => item.name.toLowerCase() === value.toLowerCase());
      if (profile.length === 1) {
        addBinding(`profile:${profile[0].id}`);
        return;
      }
      const workspace = workspacesStore.list.filter((item) => item.name.toLowerCase() === value.toLowerCase());
      if (workspace.length === 1) {
        addBinding(`workspace:${workspace[0].id}`);
        return;
      }
      const titled = notesStore.list.filter((n) => !n.deleted && n.title.toLowerCase() === value.toLowerCase());
      if (titled.length === 1) {
        addBinding(`note:${titled[0].id}`);
        return;
      }
      const partial = notesStore.list.filter((n) => !n.deleted && n.title.toLowerCase().includes(value.toLowerCase()));
      if (partial.length === 1) {
        addBinding(`note:${partial[0].id}`);
        return;
      }
      const created = await notesStore.createTag(value);
      addLabel(created.name);
    } catch (e) {
      error = formatError(e);
    }
  }

  function chipColor(tag: string): string | undefined {
    const parsed = parseBinding(tag);
    if (!parsed) return notesStore.allTags.find((item) => item.name === tag)?.color;
    if (parsed.kind === 'workspace') return workspacesStore.list.find((ws) => ws.id === parsed.value)?.color;
    if (parsed.kind === 'profile') return 'var(--accent)';
    return undefined;
  }

  function chipBg(tag: string): string | undefined {
    const color = chipColor(tag);
    if (!color || color.startsWith('var(')) return undefined;
    return `${color}22`;
  }

  function removeTag(tag: string) {
    if (initialTags.includes(tag) && !editing) return;
    tags = tags.filter((item) => item !== tag);
  }

  function generate() {
    try {
      password = generatePassword(get(pwSettings));
      showPassword = true;
    } catch (e) {
      error = formatError(e);
    }
  }

  async function save() {
    if (saving) return;
    suggest = false;
    error = '';
    if (!title.trim()) {
      error = $t('pw_err_title');
      return;
    }
    if (!editing && !password) {
      error = $t('pw_err_password');
      return;
    }
    saving = true;
    try {
      if (query.trim()) await commitQuery();
      const linked = [...totpIds];
      // Created before the password; removed again if the password save fails.
      let newTotpId: string | null = null;
      if (addTotp && totpSecret.trim()) {
        const created = await api.totp.add({
          name: (totpName || title).trim(),
          issuer: totpIssuer.trim() || null,
          secret: totpSecret.trim(),
          tags,
        });
        newTotpId = created.id;
        linked.push(created.id);
      }
      let savedId = entry?.id;
      try {
        if (entry) {
          await api.passwords.update(entry.id, {
            title: title.trim(),
            username,
            url,
            password: password || null,
            note: noteTouched || note ? note : null,
            clear_note: noteTouched && !note.trim(),
            totp_ids: linked,
            tags,
          });
        } else {
          const created = await api.passwords.create({
            title: title.trim(),
            username: username || null,
            url: url || null,
            password,
            note: note || null,
            totp_ids: linked,
            tags,
          });
          savedId = created.id;
        }
      } catch (e) {
        if (newTotpId) await api.totp.delete(newTotpId).catch(() => {});
        throw e;
      } finally {
        if (newTotpId) await totpStore.refresh();
      }
      await passwordStore.refresh();
      if (savedId) await passwordStore.dropStaleNoteBindings(savedId, tags);
      await notesLock.refresh();
      onclose();
    } catch (e) {
      const key = passwordErrorKey(e);
      error = key ? $t(key) : formatError(e);
    } finally {
      saving = false;
    }
  }
</script>

<Dialog open title={editing ? $t('pw_btn_edit') : $t('pw_btn_add')} width="460px" {onclose}>
  <div class="form">
    {#if error}<div class="err">{error}</div>{/if}
    <label>{$t('pw_field_title')}
      <input bind:value={title} autocomplete="off" />
    </label>
    <label>{$t('pw_field_username')}
      <input bind:value={username} autocomplete="off" />
    </label>
    <label>{$t('pw_field_url')}
      <input bind:value={url} autocomplete="off" />
    </label>
    <label>{$t('pw_field_password')}
      <span class="row">
        <input type={showPassword ? 'text' : 'password'} bind:value={password} autocomplete="new-password" placeholder={editing ? $t('pw_unchanged') : ''} />
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => (showPassword = !showPassword)}>
          {$t(showPassword ? 'pw_btn_hide' : 'pw_btn_reveal')}
        </button>
        <button type="button" class="btn btn-ghost btn-sm" onclick={generate}>
          <Icon name="key" size={12} /> {$t('pw_btn_generate')}
        </button>
      </span>
    </label>
    <label>{$t('pw_field_note')}
      <textarea bind:value={note} rows="3" oninput={() => (noteTouched = true)}></textarea>
    </label>
    <div class="tags">
      <span>{$t('pw_field_totp')}</span>
      {#if totpIds.length}
        <div class="chips">
          {#each totpIds as id (id)}
            {@const item = totpStore.list.find((t) => t.id === id)}
            <button type="button" class="chip" onclick={() => (totpIds = totpIds.filter((x) => x !== id))}>
              <ChipMark kind="totp" />
              {item ? (item.issuer ? `${item.issuer} · ${item.name}` : item.name) : id}
            </button>
          {/each}
        </div>
      {/if}
      {#if !addTotp}
        {@const tq = totpQuery.trim().toLowerCase()}
        {@const totpHits = totpStore.list
          .filter((item) => !totpIds.includes(item.id) && `${item.issuer ?? ''} ${item.name}`.toLowerCase().includes(tq))
          .slice(0, 8)}
        <span class="row">
          <input
            bind:value={totpQuery}
            placeholder={$t('pw_search')}
            autocomplete="off"
            onfocus={() => (totpSuggest = true)}
            onblur={() => (totpSuggest = false)}
          />
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => { addTotp = true; totpName = title; }}>
            <Icon name="plus" size={12} /> {$t('pw_add_totp')}
          </button>
        </span>
        {#if totpSuggest && totpHits.length}
          <div class="pick" role="presentation">
            {#each totpHits as item (item.id)}
              <button type="button" class="pick-item" onmousedown={(e) => { e.preventDefault(); totpIds = [...totpIds, item.id]; totpQuery = ''; }}>
                <ChipMark kind="totp" />
                {item.issuer ? `${item.issuer} · ${item.name}` : item.name}
              </button>
            {/each}
          </div>
        {/if}
      {:else}
        <input bind:value={totpName} placeholder={$t('totp_field_name')} autocomplete="off" />
        <input bind:value={totpIssuer} placeholder={$t('totp_field_issuer')} autocomplete="off" />
        <input bind:value={totpSecret} placeholder={$t('totp_field_secret')} autocomplete="off" />
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => (addTotp = false)}>{$t('pw_btn_cancel')}</button>
      {/if}
    </div>
    <div class="tags">
      <span>{$t('pw_field_tags')}</span>
      {#if tags.length}
        <div class="chips">
          {#each tags as tag (tag)}
        <button type="button" class="chip" style:color={chipColor(tag)} style:border-color={chipColor(tag)} style:background={chipBg(tag)} onclick={() => removeTag(tag)}>
              <ChipMark kind={bindingKind(tag)} />
              {bindingLabel(tag)}
            </button>
          {/each}
        </div>
      {/if}
      <input
        bind:value={query}
        placeholder={$t('notes_tags_placeholder')}
        onfocus={() => (suggest = true)}
        onblur={() => (suggest = false)}
        onkeydown={(e) => { if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); void commitQuery(); } }}
      />
      {#if suggest && (tagHits.length || profileHits.length || workspaceHits.length || noteHits.length)}
        <div class="pick" role="presentation">
          {#each tagHits as tag (tag.id)}
            <button type="button" class="pick-item" onmousedown={(e) => { e.preventDefault(); addLabel(tag.name); }}>
              <span class="dot" style:background={tag.color}></span>
              {tag.name}
            </button>
          {/each}
          {#each profileHits as profile (profile.id)}
            <button type="button" class="pick-item" onmousedown={(e) => { e.preventDefault(); addBinding(`profile:${profile.id}`); }}>
              <ChipMark kind="profile" />
              {profile.name}
            </button>
          {/each}
          {#each workspaceHits as ws (ws.id)}
            <button type="button" class="pick-item" onmousedown={(e) => { e.preventDefault(); addBinding(`workspace:${ws.id}`); }}>
              <span class="dot" style:background={ws.color}></span>
              {ws.name}
            </button>
          {/each}
          {#each noteHits as n (n.id)}
            <button type="button" class="pick-item" onmousedown={(e) => { e.preventDefault(); addBinding(`note:${n.id}`); }}>
              <ChipMark kind="note" />
              {n.title}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
  {#snippet footer()}
    <button class="btn btn-ghost" type="button" onclick={onclose}>{$t('pw_btn_cancel')}</button>
    <button class="btn btn-primary" type="button" onmousedown={(e) => { e.preventDefault(); void save(); }} disabled={saving}>{$t('pw_btn_save')}</button>
  {/snippet}
</Dialog>

<style>
  .form { display: flex; flex-direction: column; gap: var(--sp-3); }
  label, .tags { display: flex; flex-direction: column; gap: 4px; font-size: 0.78rem; color: var(--text-2); }
  .tags { position: relative; }
  input, select, textarea {
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .row { display: flex; gap: var(--sp-2); }
  .row input, .row select { flex: 1; }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid var(--border);
    border-radius: 99px;
    background: var(--surface-2);
    color: var(--text);
    padding: 2px 8px;
    font: inherit;
    font-size: 0.75rem;
  }
  .pick {
    position: absolute;
    z-index: 2;
    left: 0;
    right: 0;
    bottom: 100%;
    display: flex;
    flex-direction: column;
    max-height: 220px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    box-shadow: var(--shadow-lg);
  }
  .pick-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
  }
  .pick-item:hover { background: var(--surface-2); }
  .dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
  .tags > .btn { align-self: flex-start; }
  .err { color: var(--danger-text); font-size: 0.82rem; }
</style>
