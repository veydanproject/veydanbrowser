<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { t } from '$lib/i18n';
  import { api } from '$lib/api';
  import { totpStore } from '$lib/store/totp.svelte';
  import { profilesStore } from '$lib/store/profiles.svelte';
  import { workspacesStore } from '$lib/store/workspaces.svelte';
  import type { TotpPreview } from '$lib/types';
  import Icon from '$lib/Icon.svelte';
  import Dialog from '$lib/components/ui/Dialog.svelte';
  import { formatError } from '$lib/utils';

  interface Props {
    initialTags?: string[];
    onclose: () => void;
    onadded?: () => void;
  }

  let { initialTags = [], onclose, onadded }: Props = $props();

  let tab = $state<'manual' | 'qr'>('manual');

  // Form fields
  let name = $state('');
  let issuer = $state('');
  let secret = $state('');
  let algorithm = $state('SHA1');
  let digits = $state(6);
  let period = $state(30);
  let labelInput = $state('');
  let userLabels = $state<string[]>([]);

  // System tags (profile:*, workspace:*) — read-only context, set from initialTags
  // User labels — free-form labels like "banking", "work"
  $effect.pre(() => {
    userLabels = initialTags.filter((t) => !t.startsWith('profile:') && !t.startsWith('workspace:'));
  });

  // Resolved names for display
  const systemBindings = $derived(
    initialTags
      .filter((t) => t.startsWith('profile:') || t.startsWith('workspace:'))
      .map((tag) => {
        if (tag.startsWith('profile:')) {
          const id = tag.slice('profile:'.length);
          const p = profilesStore.list.find((x) => x.id === id);
          return { tag, kind: 'profile' as const, label: p?.name ?? id };
        } else {
          const id = tag.slice('workspace:'.length);
          const ws = workspacesStore.list.find((x) => x.id === id);
          return { tag, kind: 'workspace' as const, label: ws?.name ?? id };
        }
      })
  );

  // QR state
  let qrPreview = $state<TotpPreview | null>(null);
  let qrUri = $state('');
  let qrError = $state('');
  let qrLoading = $state(false);
  let fileInput: HTMLInputElement | null = $state(null);

  let saving = $state(false);
  let error = $state('');
  let labelsExpanded = $state(false);

  function addLabel() {
    const v = labelInput.trim();
    if (v && !userLabels.includes(v)) userLabels = [...userLabels, v];
    labelInput = '';
  }

  function removeLabel(label: string) {
    userLabels = userLabels.filter((l) => l !== label);
  }

  async function handleQrFile(e: Event) {
    const file = (e.target as HTMLInputElement).files?.[0];
    if (!file) return;
    qrError = '';
    qrPreview = null;
    qrUri = '';
    qrLoading = true;

    try {
      const bitmap = await createImageBitmap(file);
      const canvas = document.createElement('canvas');
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const ctx = canvas.getContext('2d')!;
      ctx.drawImage(bitmap, 0, 0);
      const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);

      const jsQR = (await import('jsqr')).default;
      const result = jsQR(imageData.data, imageData.width, imageData.height);

      if (!result) {
        qrError = $t('totp_qr_not_found');
        return;
      }

      qrUri = result.data;
      qrPreview = await api.totp.previewUri(qrUri);

      name = qrPreview.name;
      issuer = qrPreview.issuer ?? '';
      algorithm = qrPreview.algorithm;
      digits = qrPreview.digits;
      period = qrPreview.period;
    } catch (err) {
      qrError = formatError(err);
    } finally {
      qrLoading = false;
      if (fileInput) fileInput.value = '';
    }
  }

  async function save() {
    error = '';
    if (!name.trim()) { error = $t('totp_error_name'); return; }
    if (tab === 'manual' && !secret.trim()) { error = $t('totp_error_secret'); return; }
    if (tab === 'qr' && !qrUri) { error = $t('totp_error_secret'); return; }

    // Combine system bindings + user labels into final tags array
    const finalTags = [
      ...systemBindings.map((b) => b.tag),
      ...userLabels,
    ];

    saving = true;
    try {
      if (tab === 'qr') {
        await api.totp.add({ name: name.trim(), issuer: issuer.trim() || null, uri: qrUri, tags: finalTags });
      } else {
        await api.totp.add({ name: name.trim(), issuer: issuer.trim() || null, secret: secret.trim(), algorithm, digits, period, tags: finalTags });
      }
      await totpStore.refresh();
      onadded?.();
      onclose();
    } catch (err) {
      error = formatError(err);
    } finally {
      saving = false;
    }
  }
</script>

<Dialog open title={$t('totp_add_title')} width="460px" {onclose}>

    <div class="tab-bar">
      <button class="tab" class:active={tab === 'manual'} onclick={() => { tab = 'manual'; qrPreview = null; qrUri = ''; qrError = ''; }}>
        <Icon name="edit" size={13} /> {$t('totp_tab_manual')}
      </button>
      <button class="tab" class:active={tab === 'qr'} onclick={() => { tab = 'qr'; }}>
        <Icon name="scan" size={13} /> {$t('totp_tab_qr')}
      </button>
    </div>

    <div class="totp-form">
      <!-- Context binding (read-only) -->
      {#if systemBindings.length > 0}
        <div class="context-section">
          <span class="context-label">
            <Icon name="link" size={11} />
            Привязка
          </span>
          <div class="context-chips">
            {#each systemBindings as binding (binding.tag)}
              <span class="context-chip context-chip--{binding.kind}">
                <Icon name={binding.kind === 'profile' ? 'monitor' : 'folder-open'} size={11} />
                {binding.kind === 'profile' ? 'Профиль' : 'Воркспейс'}: {binding.label}
              </span>
            {/each}
          </div>
        </div>
      {/if}

      {#if tab === 'qr'}
        <div class="qr-section">
          <p class="hint">{$t('totp_qr_hint')}</p>
          <button class="btn-ghost" onclick={() => fileInput?.click()} disabled={qrLoading}>
            <Icon name="image" size={14} />
            {$t('totp_qr_select')}
          </button>
          <input bind:this={fileInput} type="file" accept="image/*" style="display:none" onchange={handleQrFile} />
          {#if qrLoading}<span class="hint">{$t('loading')}</span>{/if}
          {#if qrError}<div class="error-msg">{qrError}</div>{/if}
          {#if qrPreview}
            <div class="qr-detected">
              <span style="color:var(--success);display:flex"><Icon name="check-circle" size={14} /></span>
              {$t('totp_qr_detected')}
              <span class="masked">{$t('totp_qr_secret_masked')} {qrPreview.secret_masked}</span>
            </div>
          {/if}
        </div>
      {/if}

      <div class="form-group">
        <label for="totp-name">{$t('totp_field_name')}</label>
        <input id="totp-name" type="text" bind:value={name} placeholder={$t('totp_field_name_placeholder')} />
      </div>

      <div class="form-group">
        <label for="totp-issuer">{$t('totp_field_issuer')}</label>
        <input id="totp-issuer" type="text" bind:value={issuer} placeholder={$t('totp_field_issuer_placeholder')} />
      </div>

      {#if tab === 'manual'}
        <div class="form-group">
          <div class="label-row">
            <label for="totp-secret">{$t('totp_field_secret')}</label>
            <span class="tooltip-wrap">
              <span class="tooltip-trigger">?</span>
              <span class="tooltip-box">
                Ключ аутентификации — строка в формате Base32 (буквы A–Z и цифры 2–7).
                Берётся из настроек 2FA на сайте — обычно показывается под QR-кодом.
              </span>
            </span>
          </div>
          <input id="totp-secret" type="text" bind:value={secret} placeholder={$t('totp_field_secret_placeholder')} autocomplete="off" />
        </div>
      {/if}

      <div class="form-row">
        <div class="form-group">
          <div class="label-row">
            <label for="totp-algo">{$t('totp_field_algorithm')}</label>
            <span class="tooltip-wrap">
              <span class="tooltip-trigger">?</span>
              <span class="tooltip-box">
                SHA1 подходит для 99% сервисов. При сканировании QR определяется автоматически.
                При ручном вводе меняйте только если сервис явно указал SHA256 или SHA512.
              </span>
            </span>
          </div>
          <select id="totp-algo" bind:value={algorithm}>
            <option value="SHA1">SHA1 (стандарт)</option>
            <option value="SHA256">SHA256</option>
            <option value="SHA512">SHA512</option>
          </select>
        </div>
        <div class="form-group">
          <div class="label-row">
            <label for="totp-digits">{$t('totp_field_digits')}</label>
            <span class="tooltip-wrap">
              <span class="tooltip-trigger">?</span>
              <span class="tooltip-box">
                Длина одноразового кода. Стандарт — 6 цифр.
                8 цифр используют некоторые сервисы (Steam и др.).
              </span>
            </span>
          </div>
          <select id="totp-digits" bind:value={digits}>
            <option value={6}>6</option>
            <option value={8}>8</option>
          </select>
        </div>
        <div class="form-group">
          <div class="label-row">
            <label for="totp-period">{$t('totp_field_period')}</label>
            <span class="tooltip-wrap">
              <span class="tooltip-trigger">?</span>
              <span class="tooltip-box">
                Как часто меняется код. Стандарт — 30 секунд.
                60 секунд встречается редко.
              </span>
            </span>
          </div>
          <select id="totp-period" bind:value={period}>
            <option value={30}>30</option>
            <option value={60}>60</option>
          </select>
        </div>
      </div>

      <!-- User labels (collapsible, optional) -->
      {#if userLabels.length > 0 || labelsExpanded}
        <div class="form-group">
          <div class="label-row">
            <label for="totp-label-input">Метки <span class="optional">(необязательно)</span></label>
            <span class="tooltip-wrap">
              <span class="tooltip-trigger">?</span>
              <span class="tooltip-box">
                Свободные метки для фильтрации: banking, work, personal…
                Привязка к профилю и воркспейсу задаётся автоматически.
              </span>
            </span>
          </div>
          <div class="label-input-row">
            <input
              id="totp-label-input"
              type="text"
              bind:value={labelInput}
              placeholder="banking"
              onkeydown={(e) => e.key === 'Enter' && (e.preventDefault(), addLabel())}
            />
            <button class="btn-ghost btn-sm" onclick={addLabel}>+</button>
          </div>
          {#if userLabels.length > 0}
            <div class="label-chips">
              {#each userLabels as label (label)}
                <span class="label-chip">
                  {label}
                  <button onclick={() => removeLabel(label)} aria-label="remove">×</button>
                </span>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <button class="add-label-link" onclick={() => (labelsExpanded = true)}>
          + Добавить метку
        </button>
      {/if}

      {#if error}
        <div class="error-msg">{error}</div>
      {/if}
    </div>

  {#snippet footer()}
      <button class="btn-ghost" onclick={onclose}>{$t('totp_btn_cancel')}</button>
      <button class="btn-primary" onclick={save} disabled={saving}>
        {saving ? $t('loading') : $t('totp_btn_save')}
      </button>
  {/snippet}

</Dialog>

<style>
  /* .tab-bar / .tab styling is global (base.css); only local deltas kept below */
  .tab-bar { padding-top: 0.3rem; }
  .tab { justify-content: center; }

  .totp-form {
    display: flex; flex-direction: column; gap: var(--sp-3);
  }

  /* Context binding section */
  .context-section {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: 0.6rem var(--sp-3);
    background: var(--accent-tint);
    border: 1px solid var(--accent-tint-border);
    border-radius: var(--radius-md);
  }

  .context-label {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: var(--fs-2xs);
    color: var(--accent-text);
    font-weight: var(--fw-bold);
    white-space: nowrap;
    padding-top: 0.15rem;
  }

  .context-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .context-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-size: var(--fs-xs);
    padding: 0.2rem 0.6rem;
    border-radius: var(--radius-sm);
    font-weight: var(--fw-medium);
  }

  .context-chip--profile {
    background: var(--accent-bg);
    color: var(--accent-text);
    border: 1px solid var(--accent-border);
  }

  .context-chip--workspace {
    background: var(--accent-tint);
    color: var(--accent-text-2);
    border: 1px solid var(--accent-tint-border);
  }

  /* Form */
  .form-group { display: flex; flex-direction: column; gap: 0.3rem; }

  .label-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .form-group label {
    font-size: var(--fs-2xs); text-transform: uppercase;
    letter-spacing: 0.04em; color: var(--text-dim); font-weight: var(--fw-bold);
  }

  .optional { text-transform: none; font-weight: 400; opacity: 0.7; }

  /* Tooltip */
  .tooltip-wrap {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .tooltip-trigger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--surface-3);
    border: 1px solid var(--border);
    font-size: var(--fs-2xs);
    color: var(--text-2);
    cursor: default;
    font-style: normal;
    line-height: 1;
    flex-shrink: 0;
    transition: all var(--dur-fast);
  }

  .tooltip-wrap:hover .tooltip-trigger {
    background: var(--accent-tint);
    border-color: var(--accent-border);
    color: var(--accent-text);
  }

  .tooltip-box {
    display: none;
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: var(--fs-xs);
    line-height: 1.5;
    padding: var(--sp-2) 0.65rem;
    border-radius: var(--radius-sm);
    width: 220px;
    box-shadow: var(--shadow-lg);
    z-index: var(--z-popover);
    white-space: normal;
    pointer-events: none;
  }

  .tooltip-box::after {
    content: '';
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    border: 5px solid transparent;
    border-top-color: var(--bg);
  }

  .tooltip-wrap:hover .tooltip-box { display: block; }

  /* "Add label" link */
  .add-label-link {
    background: none;
    border: none;
    padding: 0;
    font-size: var(--fs-sm);
    color: var(--text-2);
    cursor: pointer;
    text-align: left;
    transition: color 0.15s;
  }
  .add-label-link:hover { color: var(--accent-text-3); }

  .form-group input, .form-group select {
    background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text);
    padding: 0.45rem 0.6rem; font-size: var(--fs-base);
    width: 100%; box-sizing: border-box;
  }
  .form-group input:focus, .form-group select:focus {
    outline: none;
    border-color: var(--accent-border);
    box-shadow: 0 0 0 3px var(--accent-bg);
  }

  .form-row { display: grid; grid-template-columns: 1fr 1fr 1fr; gap: var(--sp-3); }

  /* Labels */
  .label-input-row { display: flex; gap: 0.4rem; }

  .label-chips {
    display: flex; flex-wrap: wrap; gap: 0.35rem; margin-top: 0.3rem;
  }

  .label-chip {
    display: inline-flex; align-items: center; gap: var(--sp-1);
    background: var(--surface-3);
    border: 1px solid var(--border);
    color: var(--text-2);
    border-radius: var(--radius-sm); font-size: var(--fs-2xs); padding: 0.15rem var(--sp-2);
  }

  .label-chip button {
    background: none; border: none; cursor: pointer;
    color: var(--text-2); padding: 0; font-size: var(--fs-base); line-height: 1;
  }
  .label-chip button:hover { color: var(--danger-text); }

  /* QR */
  .qr-section {
    display: flex; flex-direction: column; gap: var(--sp-2);
    padding: var(--sp-3); background: var(--surface-3);
    border-radius: var(--radius-md); border: 1px solid var(--border);
  }

  .qr-detected {
    display: flex; align-items: center; gap: 0.4rem;
    font-size: var(--fs-sm); color: var(--success-text); flex-wrap: wrap;
  }

  .masked { color: var(--text-2); font-family: var(--font-mono); font-size: var(--fs-sm); }
  .hint { font-size: var(--fs-sm); color: var(--text-2); margin: 0; }

  .error-msg {
    background: var(--danger-bg);
    border: 1px solid var(--danger-border);
    border-radius: var(--radius);
    color: var(--danger-text);
    font-size: var(--fs-sm); padding: var(--sp-2) var(--sp-3);
  }

  .btn-primary {
    background: var(--accent-grad); color: #fff; border: none;
    border-radius: var(--radius); padding: 0.5rem var(--sp-4);
    font-size: var(--fs-base); font-weight: var(--fw-semibold);
    box-shadow: var(--shadow-accent);
    cursor: pointer; transition: filter var(--dur-fast);
  }
  .btn-primary:hover:not(:disabled) { filter: brightness(1.08); }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-ghost {
    background: var(--surface-3); border: 1px solid var(--border);
    border-radius: var(--radius); padding: 0.5rem var(--sp-4);
    font-size: var(--fs-base); cursor: pointer; color: var(--text-body);
    transition: all var(--dur-fast); display: inline-flex; align-items: center; gap: 0.35rem;
  }
  .btn-ghost:hover:not(:disabled) { background: var(--surface-hover); border-color: var(--border-2); color: var(--text); }
  .btn-ghost:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-sm { padding: 0.35rem 0.65rem; font-size: var(--fs-sm); }
</style>
