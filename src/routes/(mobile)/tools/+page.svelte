<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { pwSettings, generatePassword } from '$lib/password-gen';
  import { t } from '$lib/mobile/i18n';

  let password = $state('');
  let error = $state('');
  let toast = $state('');
  let toastTimer: ReturnType<typeof setTimeout>;

  function generate() {
    error = '';
    try {
      password = generatePassword($pwSettings);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      error = msg === 'pwgen_error_no_charset' ? $t('pwgen_error_no_charset') : msg;
    }
  }

  async function copy() {
    if (!password) return;
    await navigator.clipboard.writeText(password);
    clearTimeout(toastTimer);
    toast = $t('pwgen_copied');
    toastTimer = setTimeout(() => (toast = ''), 1600);
  }

  const flags = [
    { key: 'uppercase', label: 'pwgen_upper' },
    { key: 'lowercase', label: 'pwgen_lower' },
    { key: 'numbers', label: 'pwgen_digits' },
    { key: 'symbols', label: 'pwgen_symbols' },
    { key: 'excludeSimilar', label: 'pwgen_exclude_similar' },
  ] as const;

  function toggle(key: (typeof flags)[number]['key']) {
    pwSettings.update((s) => ({ ...s, [key]: !s[key] }));
  }
</script>

<div class="m-page">
  <div class="m-header">
    <a class="m-ibtn" href="/" aria-label={$t('common_back')}><Icon name="chevron-left" size={24} /></a>
    <h1 class="m-title">{$t('tools_generator')}</h1>
  </div>

  <div class="m-body">
  <div class="output m-card" class:empty={!password}>
    <span class="pw">{password || $t('pwgen_placeholder')}</span>
    <button class="m-ibtn" disabled={!password} onclick={copy} aria-label={$t('pwgen_btn_copy')}>
      <Icon name="copy" size={20} />
    </button>
  </div>

  {#if error}
    <div class="m-error">{error}</div>
  {/if}

  <div class="m-list">
    <div class="m-row length">
      <span class="m-row-label">{$t('pwgen_length_label', { n: String($pwSettings.length) })}</span>
      <input type="range" min="8" max="64" bind:value={$pwSettings.length} />
    </div>
    {#each flags as f (f.key)}
      <button class="m-row" onclick={() => toggle(f.key)}>
        <span class="m-row-label">{$t(f.label)}</span>
        <span class="toggle" class:on={$pwSettings[f.key]}></span>
      </button>
    {/each}
  </div>

  <button class="m-btn-grad generate" onclick={generate}>
    <Icon name="refresh-cw" size={18} />
    {$t('pwgen_btn_generate')}
  </button>

  {#if toast}
    <div class="toast">{toast}</div>
  {/if}
  </div>
</div>

<style>
  .output {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 60px;
    padding: var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-4);
    margin-bottom: var(--sp-4);
  }
  .output.empty .pw { color: var(--text-3); }
  .pw {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 15px;
    word-break: break-all;
    user-select: text;
    -webkit-user-select: text;
  }
  .m-ibtn:disabled { opacity: 0.35; }
  .generate { margin-top: var(--sp-5); }
  .length {
    flex-direction: column;
    align-items: stretch;
    gap: var(--sp-2);
    padding-top: var(--sp-3);
    padding-bottom: var(--sp-3);
  }
  .length .m-row-label { font-weight: 600; }
  .length input[type='range'] {
    width: 100%;
    min-height: 0;
    padding: 0;
    border: 0;
    background: transparent;
    accent-color: var(--accent);
  }
  .toggle { width: 46px; height: 26px; }
  .toggle::after { width: 20px; height: 20px; }
  .toggle.on::after { left: 23px; }
</style>
