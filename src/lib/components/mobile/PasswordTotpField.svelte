<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Linked TOTP codes of a password: pick existing ones or draft a new one. -->
<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import BottomSheet from '$lib/components/mobile/BottomSheet.svelte';
  import { t } from '$lib/mobile/i18n';
  import { NAV_COLORS } from '$lib/mobile/nav-colors';
  import type { TotpEntry } from '$lib/types';

  interface Props {
    entries: TotpEntry[];
    totpIds?: string[];
    creating?: boolean;
    totpName?: string;
    totpIssuer?: string;
    totpSecret?: string;
    suggestName?: string;
  }

  let {
    entries,
    totpIds = $bindable([]),
    creating = $bindable(false),
    totpName = $bindable(''),
    totpIssuer = $bindable(''),
    totpSecret = $bindable(''),
    suggestName = '',
  }: Props = $props();

  let open = $state(false);
  let query = $state('');

  const linked = $derived(
    totpIds.map((id) => entries.find((item) => item.id === id)).filter((item): item is TotpEntry => !!item),
  );
  const q = $derived(query.trim().toLowerCase());
  const hits = $derived(
    entries.filter((item) => !totpIds.includes(item.id) && `${item.issuer ?? ''} ${item.name}`.toLowerCase().includes(q)),
  );

  function labelOf(item: TotpEntry): string {
    return item.issuer ? `${item.issuer} · ${item.name}` : item.name;
  }

  function colorOf(s: string): string {
    let h = 0;
    for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
    return NAV_COLORS[h % NAV_COLORS.length];
  }

  function add(id: string) {
    if (!totpIds.includes(id)) totpIds = [...totpIds, id];
    open = false;
  }

  function remove(id: string) {
    totpIds = totpIds.filter((item) => item !== id);
  }

  function startCreate() {
    creating = true;
    if (!totpName) totpName = suggestName;
    open = false;
  }

  function cancelCreate() {
    creating = false;
    totpSecret = '';
  }
</script>

<div class="m-field">
  <span class="caption">{$t('pw_field_totp')}</span>
  {#each linked as item (item.id)}
    {@const label = item.issuer || item.name}
    <div class="picked">
      <span class="m-avatar" style:--c={colorOf(label)}>{label.charAt(0).toUpperCase()}</span>
      <span class="meta">
        <span class="name">{label}</span>
        {#if item.issuer}<span class="sub">{item.name}</span>{/if}
      </span>
      <button type="button" class="icon-btn" onclick={() => remove(item.id)} aria-label={$t('pw_field_totp_none')}>
        <Icon name="x" size={16} />
      </button>
    </div>
  {/each}
  {#if creating}
    <div class="draft">
      <input bind:value={totpName} placeholder={$t('totp_field_name')} autocomplete="off" />
      <input bind:value={totpIssuer} placeholder={$t('totp_field_issuer')} autocomplete="off" />
      <input bind:value={totpSecret} placeholder={$t('totp_field_secret')} autocomplete="off" />
      <button type="button" class="btn btn-ghost btn-sm" onclick={cancelCreate}>{$t('pw_btn_cancel')}</button>
    </div>
  {:else}
    <button type="button" class="btn btn-ghost btn-sm" onclick={() => (open = true)}>
      <Icon name="plus" size={14} /> {$t('pw_add_totp')}
    </button>
  {/if}
</div>

<BottomSheet {open} title={$t('pw_add_totp')} onclose={() => (open = false)}>
  <button type="button" class="m-row create" onclick={startCreate}>
    <Icon name="plus" size={18} />
    <span class="m-row-label">{$t('totp_btn_add')}</span>
  </button>
  <input class="find" bind:value={query} placeholder={$t('pw_search')} autocomplete="off" />
  <div class="m-list">
    {#each hits as item (item.id)}
      <button type="button" class="m-row" onclick={() => add(item.id)}>
        <span class="m-row-label">{labelOf(item)}</span>
      </button>
    {/each}
  </div>
</BottomSheet>

<style>
  .caption { font-size: var(--fs-sm); color: var(--text-2); font-weight: 600; }
  .picked {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: 64px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--m-radius, 12px);
  }
  .meta { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .name { font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: 13px; color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .draft { display: flex; flex-direction: column; gap: var(--sp-2); }
  .create { margin-bottom: var(--sp-2); }
  .find { width: 100%; margin-bottom: var(--sp-2); }
</style>
