<!--
  SPDX-FileCopyrightText: 2026 Veydan Project
  SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1
-->
<!--
  The move to Veydan Space 5, in two forms: a banner under the title bar (a
  computer) or on the home screen (a phone) that opens the steps in a dialog,
  and a card with the steps in Settings.
-->
<script lang="ts">
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { isMobile } from '$lib/platform';
  import Icon from '$lib/Icon.svelte';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import { move5, SPACE5_URL } from '$lib/store/move5.svelte';

  let {
    mode = 'banner',
    onsync,
  }: {
    mode?: 'banner' | 'card';
    /** How to show the sync settings; by default they are opened by route. */
    onsync?: () => void;
  } = $props();

  let stepsOpen = $state(false);

  function openSync() {
    stepsOpen = false;
    if (onsync) return onsync();
    void goto(isMobile ? '/settings/sync' : '/settings#sync');
  }

  function download() {
    api.system.openUrl(SPACE5_URL).catch(() => {});
  }
</script>

{#snippet steps()}
  <p class="move-intro">{$t('move5_intro')}</p>
  <ol class="move-steps">
    <li>{$t('move5_step_sync')}</li>
    <li>{$t('move5_step_wait')}</li>
    <li>{$t('move5_step_key')}</li>
    <li>{$t('move5_step_install')}</li>
    <li>{$t('move5_step_join')}</li>
  </ol>
  <p class="move-order">
    <Icon name="alert-triangle" size={14} />
    <span>{$t('move5_order')}</span>
  </p>
  <div class="move-actions">
    <button type="button" class="move-btn" onclick={openSync}>
      <Icon name="refresh-cw" size={14} />{$t('move5_open_sync')}
    </button>
    <button type="button" class="move-btn primary" onclick={download}>
      <Icon name="download" size={14} />{$t('move5_download')}
    </button>
  </div>
{/snippet}

{#if mode === 'card'}
  <section class="move-card" class:mobile={isMobile} aria-labelledby="move5-title">
    <h2 id="move5-title" class="move-title">{$t('move5_title')}</h2>
    {@render steps()}
  </section>
{:else if !move5.bannerHidden}
  <div class="move-banner" class:mobile={isMobile} role="status">
    <Icon name="info" size={14} />
    <span class="move-banner-text">{$t('move5_banner')}</span>
    <button type="button" class="move-btn primary" onclick={() => (stepsOpen = true)}>
      {$t('move5_how')}
    </button>
    <button
      type="button"
      class="move-close"
      onclick={() => (move5.bannerHidden = true)}
      title={$t('move5_hide')}
      aria-label={$t('move5_hide')}
    >
      <Icon name="x" size={13} />
    </button>
  </div>

  <Dialog bind:open={stepsOpen} title={$t('move5_title')} width="min(560px, calc(100vw - 32px))">
    <div class="move-dialog">{@render steps()}</div>
  </Dialog>
{/if}

<style>
  .move-banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 16px;
    background: var(--accent-bg);
    border-bottom: 1px solid var(--accent-border);
    color: var(--accent-text);
    font-size: var(--fs-sm, 12.5px);
    flex-shrink: 0;
  }
  .move-banner.mobile {
    flex-wrap: wrap;
    margin: 8px 12px 0;
    padding: 10px 12px;
    border: 1px solid var(--accent-border);
    border-radius: 12px;
    font-size: 13px;
  }
  .move-banner-text {
    flex: 1 1 auto;
    min-width: 0;
  }
  .move-banner.mobile .move-banner-text {
    flex-basis: calc(100% - 60px);
  }

  .move-close {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--accent-text);
    cursor: pointer;
    padding: 2px;
    display: flex;
    align-items: center;
    opacity: 0.7;
  }
  .move-close:hover {
    opacity: 1;
  }
  .move-banner.mobile .move-close {
    order: -1;
    margin-left: auto;
  }

  .move-card {
    border: 1px solid var(--accent-border);
    background: var(--accent-bg);
    border-radius: var(--radius, 10px);
    padding: 16px 18px;
    margin-bottom: 16px;
    color: var(--text);
  }
  .move-card.mobile {
    margin: 0 0 4px;
    border-radius: var(--m-radius, 12px);
    padding: 14px;
  }
  .move-title {
    margin: 0 0 8px;
    font-size: 15px;
    font-weight: 650;
    color: var(--accent-text);
  }

  .move-dialog {
    color: var(--text);
  }
  .move-intro {
    margin: 0 0 10px;
    line-height: 1.5;
  }
  .move-steps {
    margin: 0 0 10px;
    padding-left: 20px;
    line-height: 1.5;
  }
  .move-steps li + li {
    margin-top: 4px;
  }
  .move-order {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    margin: 0 0 12px;
    line-height: 1.45;
    color: var(--warning, var(--text));
  }
  .move-order :global(svg) {
    flex-shrink: 0;
    margin-top: 3px;
  }

  .move-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .move-btn {
    appearance: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 1px solid var(--accent-tint-border);
    background: transparent;
    color: var(--accent-text);
    border-radius: var(--radius-sm, 6px);
    padding: 4px 12px;
    font-size: inherit;
    font-family: inherit;
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease;
  }
  .move-banner .move-btn {
    padding: 2px 10px;
  }
  .move-btn:hover {
    background: var(--accent-tint);
    border-color: var(--accent-border);
  }
  .move-btn.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .move-btn.primary:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
  }
</style>
