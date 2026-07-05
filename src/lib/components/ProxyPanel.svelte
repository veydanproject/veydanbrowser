<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import type { CreateProxyRequest, Proxy } from '$lib/types';
  import Drawer from '$lib/components/ui/Drawer.svelte';
  import { formatError } from '$lib/utils';

  interface Props {
    proxy?: Proxy | null;
    workspaceId?: string | null;
    onclose: () => void;
    onsaved: (proxy: Proxy) => void;
  }

  let { proxy = null, workspaceId = null, onclose, onsaved }: Props = $props();

  const isEdit = $derived(proxy !== null);

  let saving = $state(false);
  let error = $state('');

  // SSH TOFU: shown when a new (unknown) server fingerprint is received
  let pendingFingerprint = $state<{ fingerprint: string; ip: string } | null>(null);
  let fingerprintError = $state('');

  let form = $state<CreateProxyRequest>(untrack(() => ({
    name: proxy?.name ?? '',
    proxy_type: proxy?.proxy_type ?? 'socks5',
    host: proxy?.host ?? '',
    port: proxy?.port ?? 1080,
    username: proxy?.username ?? null,
    password: proxy?.password ?? null,
    country: proxy?.country ?? null,
    city: proxy?.city ?? null,
    private_key: proxy?.private_key ?? null,
    tags: proxy ? proxy.tags : (workspaceId ? [`workspace:${workspaceId}`] : []),
  })));

  async function submit() {
    if (!form.name.trim() || !form.host.trim()) {
      error = $t('proxy_error_fields');
      return;
    }
    saving = true;
    error = '';
    try {
      const result = isEdit
        ? await api.proxies.update(proxy!.id, form)
        : await api.proxies.create(form);
      onsaved(result);
    } catch (e) {
      error = formatError(e);
    } finally {
      saving = false;
    }
  }
</script>

<Drawer open title={isEdit ? $t('proxies_form_edit') : $t('proxies_form_new')} {onclose}>
      <form onsubmit={(e) => { e.preventDefault(); submit(); }}>
        {#if error}
          <div class="error-msg" style="margin-bottom:0.75rem">{error}</div>
        {/if}

        <div class="section">
          <div class="section-label">{$t('proxy_field_type')}</div>
          <div class="type-row">
            {#each ['http', 'https', 'socks5', 'ssh'] as t_}
              <button
                type="button"
                class="type-btn"
                class:active={form.proxy_type === t_}
                onclick={() => {
                  form.proxy_type = t_;
                  if (t_ === 'ssh' && (form.port === 1080 || form.port === 8080)) form.port = 22;
                  if (t_ !== 'ssh' && form.port === 22) form.port = 1080;
                }}
              >{t_.toUpperCase()}</button>
            {/each}
          </div>
        </div>

        <div class="divider"></div>

        <div class="section">
          <div class="section-label">{$t('proxy_field_name')} *</div>
          <div class="form-group">
            <!-- svelte-ignore a11y_autofocus -->
            <input
              id="pp-name"
              type="text"
              bind:value={form.name}
              placeholder={$t('proxy_field_name_placeholder')}
              autofocus
            />
          </div>

          <div class="form-row">
            <div class="form-group host-group">
              <label for="pp-host">{$t('proxy_field_host')} *</label>
              <input id="pp-host" type="text" bind:value={form.host} placeholder={$t('proxy_field_host_placeholder')} />
            </div>
            <div class="form-group port-group">
              <label for="pp-port">{$t('proxy_field_port')}</label>
              <input id="pp-port" type="number" bind:value={form.port} min="1" max="65535" />
            </div>
          </div>
        </div>

        <div class="divider"></div>

        <div class="section">
          <div class="section-label">{$t('proxy_field_username')} / {$t('proxy_field_password')}</div>
          <div class="form-group">
            <input id="pp-user" type="text" bind:value={form.username} placeholder={$t('proxy_field_username')} />
          </div>
          <div class="form-group">
            <input id="pp-pass" type="password" bind:value={form.password} placeholder={$t('proxy_field_password')} />
          </div>
        </div>

        {#if form.proxy_type === 'ssh'}
        <div class="divider"></div>
        <div class="section">
          <div class="section-label">SSH Private Key (опционально)</div>
          <div class="form-group">
            <textarea
              id="pp-pkey"
              rows="5"
              bind:value={form.private_key}
              placeholder="-----BEGIN OPENSSH PRIVATE KEY-----&#10;...&#10;-----END OPENSSH PRIVATE KEY-----"
              style="font-family: monospace; font-size: 0.72rem; resize: vertical;"
            ></textarea>
          </div>
          <div class="field-hint">Если заполнено — используется вместо пароля</div>
        </div>
        {/if}

        <div class="divider"></div>

        <div class="section">
          <div class="section-label">Geo (опционально)</div>
          <div class="form-row">
            <div class="form-group">
              <label for="pp-country">Country</label>
              <input id="pp-country" type="text" bind:value={form.country} placeholder="US" />
            </div>
            <div class="form-group">
              <label for="pp-city">City</label>
              <input id="pp-city" type="text" bind:value={form.city} placeholder="New York" />
            </div>
          </div>
        </div>

        <div class="form-actions">
          <button type="button" class="btn btn-ghost" onclick={onclose}>{$t('proxy_btn_cancel')}</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? '…' : isEdit ? $t('proxy_btn_save') : $t('proxy_btn_add')}
          </button>
        </div>
      </form>
</Drawer>

<style>
  .section { display: flex; flex-direction: column; gap: var(--sp-2); margin-bottom: var(--sp-1); }

  .section-label {
    font-size: var(--fs-2xs); font-weight: 700; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.08em;
  }

  .type-row { display: flex; gap: 0.35rem; }

  .type-btn {
    flex: 1; padding: 0.4rem; font-size: var(--fs-sm); font-weight: 600;
    background: var(--surface-2); border: 1px solid var(--border);
    color: var(--text-2); border-radius: var(--radius-sm); cursor: pointer;
    transition: all 0.15s;
  }
  .type-btn:hover { border-color: var(--border-2); color: var(--text); }
  .type-btn.active { background: var(--accent-bg); border-color: var(--accent); color: var(--accent); }

  .divider { height: 1px; background: var(--border); margin: var(--sp-3) 0; }

  .field-hint {
    font-size: var(--fs-2xs); color: var(--text-3); margin-top: -0.25rem;
  }

  textarea {
    width: 100%; background: var(--surface); border: 1px solid var(--border);
    border-radius: var(--radius-sm); color: var(--text);
    padding: 0.45rem 0.6rem; box-sizing: border-box;
  }
  textarea:focus { outline: none; border-color: var(--accent); }

  .form-row { display: grid; grid-template-columns: 1fr auto; gap: var(--sp-2); }
  .port-group { width: 90px; }
  .host-group { flex: 1; }

  .form-actions {
    display: flex; gap: var(--sp-2); justify-content: flex-end;
    padding-top: var(--sp-3);
  }
</style>
