<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import {
    scan,
    cancel,
    checkPermissions,
    requestPermissions,
    openAppSettings,
    Format,
  } from '@tauri-apps/plugin-barcode-scanner';
  import Icon from '$lib/Icon.svelte';
  import { formatError } from '$lib/mobile/api';
  import { t } from '$lib/mobile/i18n';

  let error = $state('');
  let denied = $state(false);
  let active = false;

  async function start() {
    let perm = await checkPermissions();
    if (perm === 'prompt') {
      perm = await requestPermissions();
      // Let the permission activity finish before CameraX binds to ours.
      await new Promise((r) => setTimeout(r, 400));
    }
    if (perm !== 'granted') {
      denied = true;
      return;
    }
    // windowed: camera renders behind the (now transparent) webview.
    document.documentElement.classList.add('scanning');
    active = true;
    try {
      const result = await scan({ windowed: true, formats: [Format.QRCode] });
      active = false;
      if (!result.content.startsWith('otpauth://')) {
        error = $t('totp_scan_not_otpauth');
        return;
      }
      goto(`/totp/add?uri=${encodeURIComponent(result.content)}`, { replaceState: true });
    } catch (e) {
      active = false;
      error = formatError(e);
    } finally {
      document.documentElement.classList.remove('scanning');
    }
  }

  onMount(() => {
    start().catch((e) => (error = formatError(e)));
  });
  onDestroy(() => {
    document.documentElement.classList.remove('scanning');
    if (active) cancel().catch(() => {});
  });
</script>

<div class="scan">
  <div class="top">
    <a class="icon-btn" href="/totp/add" aria-label={$t('common_back')}>
      <Icon name="arrow-left" size={20} />
    </a>
    <span class="title">{$t('totp_scan_title')}</span>
  </div>

  {#if denied}
    <div class="msg">
      <Icon name="alert-triangle" size={32} />
      <p>{$t('totp_scan_denied')}</p>
      <button class="btn btn-ghost" onclick={() => openAppSettings()}>{$t('totp_scan_open_settings')}</button>
    </div>
  {:else if error}
    <div class="msg">
      <Icon name="alert-triangle" size={32} />
      <p>{error}</p>
      <button class="btn btn-primary" onclick={() => { error = ''; start(); }}>{$t('totp_scan_retry')}</button>
    </div>
  {:else}
    <div class="frame"></div>
    <p class="hint">{$t('totp_scan_hint')}</p>
  {/if}
</div>

<style>
  .scan {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    height: 100%;
    padding: var(--sp-4);
    animation: vfade var(--dur-base) ease-out;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
  }
  .title { font-size: var(--fs-lg); font-weight: 600; color: #fff; }

  .frame {
    width: min(70vw, 300px);
    aspect-ratio: 1;
    margin-top: 18vh;
    border: 3px solid var(--accent);
    border-radius: 24px;
  }
  .hint {
    margin-top: var(--sp-5);
    color: #fff;
    font-size: var(--fs-sm);
    text-align: center;
  }

  .msg {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    margin-top: 20vh;
    color: var(--text-2);
    text-align: center;
  }
  .msg p { margin: 0; }
</style>
