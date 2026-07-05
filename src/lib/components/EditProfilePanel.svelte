<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { api } from '$lib/api';
  import { t } from '$lib/i18n';
  import type { PresetInfo, Profile, Proxy, UpdateProfileRequest } from '$lib/types';
  import { LOCALES } from '$lib/locales';
  import { GPU_PRESETS, presetToGpuOs, defaultGpuForPreset } from '$lib/gpu-presets';
  import { formatError } from '$lib/utils';
  import { TIMEZONES, getExpectedRegion, getTimezoneRegion } from '$lib/timezones';
  import CustomSelect from '$lib/components/CustomSelect.svelte';
  import Drawer from '$lib/components/ui/Drawer.svelte';

  interface Props {
    profile: Profile;
    /** All proxies (workspace-tagged shown first) */
    proxies: Proxy[];
    onclose: () => void;
    onsaved: (updated: Profile) => void;
  }

  let { profile, proxies, onclose, onsaved }: Props = $props();

  const wsTag = $derived(`workspace:${profile.workspace_id ?? 'default'}`);
  const proxyOptions = $derived.by(() => {
    const ws = proxies.filter(p => p.tags.includes(wsTag));
    const other = proxies.filter(p => !p.tags.includes(wsTag));
    const toOpt = (p: Proxy) => ({ label: `${p.name} (${p.proxy_type}://${p.host}:${p.port})`, value: p.id });
    return [
      { label: $t('profile_proxy_none'), value: null as string | null },
      ...ws.map(toOpt),
      ...(other.length > 0 && ws.length > 0 ? [{ label: '— ' + $t('proxies_other') + ' —', value: null, disabled: true }] : []),
      ...other.map(toOpt),
    ];
  });

  let presets = $state<PresetInfo[]>([]);
  let saving = $state(false);
  let error = $state('');

  let form = $state<UpdateProfileRequest>(untrack(() => ({
    name: profile.name,
    browser_type: profile.browser_type,
    proxy_id: profile.proxy_id,
    fingerprint_preset: profile.fingerprint_preset,
    user_agent: null,
    platform: profile.platform,
    timezone: profile.timezone,
    locale: profile.locale,
    languages: profile.languages,
    screen_width: profile.screen_width,
    screen_height: profile.screen_height,
    webrtc_mode: profile.webrtc_mode,
    geolocation_enabled: profile.geolocation_enabled,
    latitude: profile.latitude,
    longitude: profile.longitude,
    webgl_vendor: profile.webgl_vendor,
    webgl_renderer: profile.webgl_renderer,
    notes: profile.notes,
    default_search_engine: profile.default_search_engine ?? 'ddg',
    history_enabled: profile.history_enabled ?? true,
  })));

  let gpuOptions = $derived(
    GPU_PRESETS.filter(g => g.os === presetToGpuOs(form.fingerprint_preset ?? 'win10'))
  );

  let warnings = $derived(() => {
    const result: string[] = [];
    const gpuOs = presetToGpuOs(form.fingerprint_preset ?? 'win10');

    if (form.webgl_vendor && form.webgl_renderer) {
      const gpu = GPU_PRESETS.find(g => g.vendor === form.webgl_vendor && g.renderer === form.webgl_renderer);
      if (gpu && gpu.os !== gpuOs) {
        result.push($t('profile_warning_gpu_os'));
      }
    } else if (gpuOs !== 'linux') {
      result.push($t('profile_warning_no_gpu_windows'));
    }

    if (form.proxy_id && form.timezone) {
      const proxy = proxies.find(p => p.id === form.proxy_id);
      const proxyRegion = getExpectedRegion(proxy?.country);
      const tzRegion = getTimezoneRegion(form.timezone);
      if (proxyRegion && tzRegion && proxyRegion !== tzRegion) {
        result.push($t('profile_warning_tz_proxy'));
      }
    }

    return result;
  });

  onMount(async () => {
    presets = await api.fingerprintPresets();
  });

  async function submit() {
    if (!form.name?.trim()) { error = $t('profile_error_name'); return; }
    saving = true; error = '';
    try {
      const updated = await api.profiles.update(profile.id, form);
      onsaved(updated);
    } catch (e) {
      error = formatError(e);
    } finally {
      saving = false;
    }
  }
</script>

<Drawer open title={`${$t('profile_edit_title')} — ${profile.name}`} {onclose}>
      {#if error}
        <div class="error-msg" style="margin-bottom:0.75rem">{error}</div>
      {/if}

      <form onsubmit={(e) => { e.preventDefault(); submit(); }}>
        <!-- General -->
        <div class="section">
          <div class="section-label">{$t('profile_section_general')}</div>

          <div class="form-group">
            <label for="ep-name">{$t('profile_field_name')} *</label>
            <input id="ep-name" type="text" bind:value={form.name} />
          </div>

          <div class="form-group">
            <label for="ep-browser">{$t('profile_field_browser')}</label>
            <CustomSelect
              id="ep-browser"
              bind:value={form.browser_type}
              options={[
                { label: 'Camoufox', value: 'camoufox' },
                { label: 'Firefox',  value: 'firefox' },
              ]}
            />
          </div>

          <div class="form-group">
            <label for="ep-proxy">{$t('profile_field_proxy')}</label>
            <CustomSelect
              id="ep-proxy"
              bind:value={form.proxy_id}
              options={proxyOptions}
            />
          </div>
        </div>

        <div class="divider"></div>

        <!-- Fingerprint -->
        <div class="section">
          <div class="section-label">{$t('profile_section_fingerprint')}</div>

          <div class="form-group">
            <label for="ep-preset">{$t('profile_field_preset')}</label>
            <CustomSelect
              id="ep-preset"
              bind:value={form.fingerprint_preset}
              options={presets.map(p => ({ label: p.label, value: p.id }))}
              onchange={(preset) => {
                const def = defaultGpuForPreset(preset ?? 'win10');
                if (def) {
                  form.webgl_vendor = def.vendor;
                  form.webgl_renderer = def.renderer;
                } else {
                  form.webgl_vendor = null;
                  form.webgl_renderer = null;
                }
              }}
            />
          </div>

          <div class="form-group">
            <label for="ep-gpu">{$t('profile_field_gpu')}</label>
            <CustomSelect
              id="ep-gpu"
              value={form.webgl_vendor && form.webgl_renderer ? `${form.webgl_vendor}|${form.webgl_renderer}` : ''}
              options={[
                { label: $t('profile_gpu_none'), value: '' },
                ...gpuOptions.map(g => ({ label: g.label, value: `${g.vendor}|${g.renderer}` })),
              ]}
              onchange={(v) => {
                const gpu = GPU_PRESETS.find(g => `${g.vendor}|${g.renderer}` === v);
                form.webgl_vendor = gpu?.vendor ?? null;
                form.webgl_renderer = gpu?.renderer ?? null;
              }}
            />
          </div>

          <div class="form-group">
            <label for="ep-locale">{$t('profile_field_locale')}</label>
            <CustomSelect
              id="ep-locale"
              bind:value={form.locale}
              options={LOCALES.map(l => ({ label: `${l.label} — ${l.locale}`, value: l.locale }))}
              onchange={(v) => {
                const opt = LOCALES.find(l => l.locale === v);
                if (opt) form.languages = opt.languages;
              }}
            />
          </div>

          <div class="form-group">
            <label for="ep-tz">{$t('profile_field_timezone')}</label>
            <CustomSelect
              id="ep-tz"
              bind:value={form.timezone}
              placeholder={$t('profile_timezone_none')}
              options={[
                { label: $t('profile_timezone_none'), value: null },
                ...TIMEZONES.map(tz => ({ label: tz.label, value: tz.value })),
              ]}
            />
          </div>

          <div class="form-group">
            <label for="ep-webrtc">{$t('profile_field_webrtc')}</label>
            <CustomSelect
              id="ep-webrtc"
              bind:value={form.webrtc_mode}
              options={[
                { label: $t('profile_webrtc_disable'), value: 'disable' },
                { label: $t('profile_webrtc_proxy'),   value: 'proxy_only' },
                { label: $t('profile_webrtc_real_ip'), value: 'real_ip' },
              ]}
            />
          </div>

          <div class="form-group">
            <label for="ep-search">{$t('profile_field_search_engine')}</label>
            <CustomSelect
              id="ep-search"
              bind:value={form.default_search_engine}
              options={[
                { label: 'DuckDuckGo',   value: 'ddg' },
                { label: 'Google',       value: 'google' },
                { label: 'Brave Search', value: 'brave' },
                { label: 'Startpage',    value: 'startpage' },
              ]}
            />
          </div>

          <div class="form-row">
            <div class="form-group">
              <label for="ep-sw">{$t('profile_field_screen_w')}</label>
              <input id="ep-sw" type="number" bind:value={form.screen_width} />
            </div>
            <div class="form-group">
              <label for="ep-sh">{$t('profile_field_screen_h')}</label>
              <input id="ep-sh" type="number" bind:value={form.screen_height} />
            </div>
          </div>

          <div class="toggle-row">
            <div class="toggle-info">
              <span class="toggle-label">{$t('profile_field_history')}</span>
              <span class="toggle-hint">{$t('profile_field_history_hint')}</span>
            </div>
            <button
              type="button"
              class="toggle-btn"
              class:active={form.history_enabled}
              onclick={() => (form.history_enabled = !form.history_enabled)}
              aria-pressed={form.history_enabled}
              aria-label={$t('profile_field_history')}
            >
              <span class="toggle-thumb"></span>
            </button>
          </div>
        </div>

        {#if warnings().length > 0}
          <div class="warnings">
            <div class="warning-title">⚠ {$t('profile_warning_title')}</div>
            {#each warnings() as w}
              <div class="warning-item">• {w}</div>
            {/each}
          </div>
        {/if}

        <div class="divider"></div>

        <div class="form-group">
          <label for="ep-notes">{$t('profile_field_notes')}</label>
          <textarea id="ep-notes" bind:value={form.notes} rows="3"></textarea>
        </div>

        <div class="form-actions">
          <button type="button" class="btn btn-ghost" onclick={onclose}>{$t('profile_btn_cancel')}</button>
          <button type="submit" class="btn btn-primary" disabled={saving}>
            {saving ? $t('profile_btn_saving') : $t('profile_btn_save')}
          </button>
        </div>
      </form>
</Drawer>

<style>
  .section { display: flex; flex-direction: column; gap: 0.6rem; margin-bottom: var(--sp-3); }

  .section-label {
    font-size: var(--fs-2xs); font-weight: 700; color: var(--text-muted);
    text-transform: uppercase; letter-spacing: 0.08em;
  }

  .divider { height: 1px; background: var(--border); margin: var(--sp-3) 0; }

  .form-row { display: grid; grid-template-columns: 1fr 1fr; gap: 0.6rem; }

  .warnings {
    background: color-mix(in srgb, var(--warning, #f59e0b) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--warning, #f59e0b) 40%, transparent);
    border-radius: 6px;
    padding: 0.6rem var(--sp-3);
    margin-bottom: var(--sp-3);
    font-size: var(--fs-sm);
  }
  .warning-title { font-weight: 700; margin-bottom: var(--sp-1); }
  .warning-item { color: var(--text-muted); }

  .form-actions {
    display: flex; gap: var(--sp-2); justify-content: flex-end; padding-top: var(--sp-2);
  }

  .toggle-row {
    display: flex; align-items: center; justify-content: space-between;
    padding: 0.45rem 0; gap: var(--sp-3);
  }
  .toggle-info { display: flex; flex-direction: column; gap: 0.15rem; min-width: 0; }
  .toggle-label { font-size: var(--fs-base); font-weight: 500; color: var(--text); }
  .toggle-hint { font-size: var(--fs-xs); color: var(--text-2); }

  .toggle-btn {
    position: relative; flex-shrink: 0;
    width: 36px; height: 20px;
    background: var(--border-2); border: none; border-radius: 999px;
    cursor: pointer; padding: 0; transition: background 0.2s;
  }
  .toggle-btn.active { background: var(--accent); }
  .toggle-thumb {
    position: absolute; top: 2px; left: 2px;
    width: 16px; height: 16px; border-radius: 50%;
    background: #fff; transition: transform 0.2s;
    box-shadow: 0 1px 3px rgba(0,0,0,0.3);
  }
  .toggle-btn.active .toggle-thumb { transform: translateX(16px); }
</style>
