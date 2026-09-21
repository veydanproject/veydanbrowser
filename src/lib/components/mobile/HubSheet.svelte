<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/mobile/i18n';
  import { APPS, appForPath } from '$lib/mobile/apps';
  import BottomSheet from './BottomSheet.svelte';

  let { open, onclose }: { open: boolean; onclose: () => void } = $props();

  let current = $derived(appForPath(page.url.pathname)?.id);

  function pick(route: string) {
    onclose();
    void goto(route);
  }
</script>

<BottomSheet {open} {onclose}>
  <div class="head">
    <img src="/logo.png" alt="" />
    <h2>Veydan</h2>
    <p>{$t('hub_sub')}</p>
  </div>
  <div class="m-grid">
    {#each APPS as app (app.id)}
      <button type="button" class="m-tile" class:active={current === app.id} onclick={() => pick(app.route)}>
        <Icon name={app.icon} size={24} />
        <span class="label">{$t(app.title)}</span>
      </button>
    {/each}
  </div>
</BottomSheet>

<style>
  .head {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding-bottom: var(--sp-2);
  }
  .head img { width: 40px; height: 40px; object-fit: contain; margin-bottom: var(--sp-1); }
  h2 { margin: 0; font-size: 20px; font-weight: 800; letter-spacing: -0.02em; }
  p { margin: 0; color: var(--text-2); font-size: 13px; }
  .m-grid { padding-bottom: var(--sp-2); }
</style>
