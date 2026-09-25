<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import Icon from '$lib/Icon.svelte';
  import EntityLabelField from '$lib/components/mobile/EntityLabelField.svelte';
  import TotpNoteLinks from '$lib/components/TotpNoteLinks.svelte';
  import { api, formatError } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { notesLinkedToTotp, syncTotpNotes } from '$lib/totp-notes';

  const id = $derived(page.params.id ?? '');

  let name = $state('');
  let issuer = $state('');
  let tags = $state<string[]>([]);
  let notes = $state<{ id: string; title: string; bindings: string[] }[]>([]);
  let noteIds = $state<string[]>([]);
  let noteSeed = $state<string[]>([]);
  let ready = $state(false);
  let missing = $state(false);
  let error = $state('');
  let saving = $state(false);

  onMount(async () => {
    try {
      const entries = await api.totp.list();
      const entry = entries.find((item) => item.id === id);
      if (!entry) {
        missing = true;
        return;
      }
      name = entry.name;
      issuer = entry.issuer ?? '';
      tags = [...entry.tags];
      ready = true;
      try {
        const items = await api.notes.list();
        notes = items.map((n) => ({ id: n.id, title: n.title, bindings: n.bindings }));
        noteIds = notesLinkedToTotp(id, notes);
        noteSeed = [...noteIds];
      } catch {
        /* note picker stays empty; the account can still be saved */
      }
    } catch (err) {
      error = formatError(err);
    }
  });

  async function save() {
    error = '';
    if (!name.trim()) {
      error = $t('totp_error_name');
      return;
    }
    saving = true;
    try {
      await api.totp.update(id, {
        name: name.trim(),
        issuer: issuer.trim() || null,
        tags,
      });
      await syncTotpNotes(id, noteIds, noteSeed);
      goto('/totp', { replaceState: true });
    } catch (err) {
      error = formatError(err);
    } finally {
      saving = false;
    }
  }
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/totp" aria-label={$t('common_back')}>
      <Icon name="chevron-left" size={24} />
    </a>
    <h1 class="m-title">{$t('totp_edit_title')}</h1>
  </div>

  <div class="m-body">
    {#if error}
      <div class="m-error">{error}</div>
    {/if}

    {#if missing}
      <div class="m-empty">
        <p>{$t('totp_not_found')}</p>
      </div>
    {:else if ready}
      <div class="m-field">
        <label for="name">{$t('totp_field_name')}</label>
        <input id="name" bind:value={name} placeholder={$t('totp_field_name_placeholder')} autocomplete="off" />
      </div>
      <div class="m-field">
        <label for="issuer">{$t('totp_field_issuer')}</label>
        <input id="issuer" bind:value={issuer} placeholder={$t('totp_field_issuer_placeholder')} autocomplete="off" />
      </div>

      <EntityLabelField {tags} onchange={(next) => (tags = next)} />
      <TotpNoteLinks {notes} selected={noteIds} onchange={(ids) => (noteIds = ids)} />

      <button class="btn btn-primary save" onclick={save} disabled={saving}>
        <Icon name="check" size={16} />
        {$t('totp_btn_save')}
      </button>
    {:else if !error}
      <p class="wait">{$t('loading')}</p>
    {/if}
  </div>
</div>

<style>
  .save {
    width: 100%;
    margin-top: var(--sp-4);
    font-size: 15px;
  }
  .wait { color: var(--text-2); font-size: var(--fs-sm); }
</style>
