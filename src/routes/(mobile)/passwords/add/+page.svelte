<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { goto } from '$app/navigation';
  import { get } from 'svelte/store';
  import Icon from '$lib/Icon.svelte';
  import EntityLabelField from '$lib/components/mobile/EntityLabelField.svelte';
  import PasswordTotpField from '$lib/components/mobile/PasswordTotpField.svelte';
  import { api, formatError } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';
  import { generatePassword, pwSettings } from '$lib/password-gen';
  import { passwordErrorKey } from '$lib/password-error';
  import { passwordStore } from '$lib/store/passwords.svelte';
  import type { TotpEntry } from '$lib/types';

  let title = $state('');
  let username = $state('');
  let url = $state('');
  let password = $state('');
  let note = $state('');
  let totpIds = $state<string[]>([]);
  let addTotp = $state(false);
  let totpName = $state('');
  let totpIssuer = $state('');
  let totpSecret = $state('');
  let tags = $state<string[]>([]);
  let show = $state(false);
  let totp = $state<TotpEntry[]>([]);
  let error = $state('');
  let saving = $state(false);

  $effect(() => {
    api.totp.list().then((list) => (totp = list)).catch(() => {});
  });

  function generate() {
    try {
      password = generatePassword(get(pwSettings));
      show = true;
    } catch (e) {
      error = formatError(e);
    }
  }

  async function save() {
    error = '';
    if (!title.trim()) return (error = $t('pw_err_title'));
    if (!password) return (error = $t('pw_err_password'));
    saving = true;
    try {
      const linked = [...totpIds];
      // Created before the password; removed again if the password save fails.
      let newTotpId: string | null = null;
      if (addTotp && totpSecret.trim()) {
        const code = await api.totp.add({
          name: (totpName || title).trim(),
          issuer: totpIssuer.trim() || null,
          secret: totpSecret.trim(),
          tags,
        });
        newTotpId = code.id;
        linked.push(code.id);
      }
      const created = await api.passwords.create({
        title: title.trim(),
        username: username || null,
        url: url || null,
        password,
        note: note || null,
        totp_ids: linked,
        tags,
      }).catch(async (e) => {
        if (newTotpId) await api.totp.delete(newTotpId).catch(() => {});
        throw e;
      });
      await passwordStore.refresh();
      await passwordStore.dropStaleNoteBindings(created.id, tags);
      goto(`/passwords/${created.id}`, { replaceState: true });
    } catch (e) {
      const key = passwordErrorKey(e);
      error = key ? $t(key) : formatError(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/passwords" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    <h1 class="m-title">{$t('pw_btn_add')}</h1>
  </div>
  <div class="m-body">
    {#if error}<div class="m-error">{error}</div>{/if}
    <div class="m-field"><label for="title">{$t('pw_field_title')}</label><input id="title" bind:value={title} autocomplete="off" /></div>
    <div class="m-field"><label for="user">{$t('pw_field_username')}</label><input id="user" bind:value={username} autocomplete="off" /></div>
    <div class="m-field"><label for="url">{$t('pw_field_url')}</label><input id="url" bind:value={url} autocomplete="off" /></div>
    <div class="m-field">
      <label for="pw">{$t('pw_field_password')}</label>
      <input id="pw" type={show ? 'text' : 'password'} bind:value={password} autocomplete="new-password" />
      <div class="row">
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => (show = !show)}>{$t(show ? 'pw_btn_hide' : 'pw_btn_reveal')}</button>
        <button type="button" class="btn btn-ghost btn-sm" onclick={generate}>{$t('pw_btn_generate')}</button>
      </div>
    </div>
    <div class="m-field"><label for="note">{$t('pw_field_note')}</label><textarea id="note" rows="3" bind:value={note}></textarea></div>
    <PasswordTotpField
      entries={totp}
      bind:totpIds
      bind:creating={addTotp}
      bind:totpName
      bind:totpIssuer
      bind:totpSecret
      suggestName={title}
    />
    <EntityLabelField linkNotes {tags} onchange={(next) => (tags = next)} />
    <button class="btn btn-primary save" onclick={save} disabled={saving}>{$t('pw_btn_save')}</button>
  </div>
</div>

<style>
  .row { display: flex; gap: var(--sp-2); margin-top: var(--sp-2); }
  .save { margin-top: var(--sp-3); }
</style>
