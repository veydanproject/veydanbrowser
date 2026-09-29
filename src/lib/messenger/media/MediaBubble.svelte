<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  The attachment of one message: upload or download state, preview for
  what a webview renders passively, a file card for everything else.
  Shared by DMs and, later, groups.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';
  import { isTauriHost, mediaErrorCode, mediaOf, messengerApi, messengerError, type MessengerMessage } from '../api';
  import { bytes, percent } from '../shared/format';
  import { transferStore } from './transferStore.svelte';

  interface Props { message: MessengerMessage }
  let { message: m }: Props = $props();

  const media = $derived(mediaOf(m));
  const out = $derived(m.direction === 'out');
  const live = $derived(transferStore.get(m.id));
  let src = $state<string | null>(null);
  let local = $state<string | null>(null);
  let busy = $state(false);
  let error = $state('');
  let previewFailed = $state(false);

  /** One word for the whole component. */
  const phase = $derived.by(() => {
    if (out && (m.status === 'uploading' || live?.status === 'running' && live.direction === 'up')) return 'uploading';
    if (out && m.status === 'paused') return 'upload_paused';
    if (out && m.status === 'failed' && m.id.startsWith('local:')) return 'upload_failed';
    if (local || media?.local_path) return 'here';
    if (live?.direction === 'down' && (live.status === 'running' || live.status === 'queued')) return 'downloading';
    if (live?.direction === 'down' && live.status === 'paused') return 'download_paused';
    if (live?.direction === 'down' && live.status === 'failed') return 'download_failed';
    return 'remote';
  });

  const progress = $derived(live ? percent(live.done_bytes, live.total_bytes) : 0);
  const transferId = $derived(live?.transfer_id ?? media?.transfer_id ?? null);
  const previewable = $derived(!!media && media.kind !== 'file' && media.size <= 24 * 1024 * 1024 && !previewFailed);

  function explain(e: unknown): string {
    const code = mediaErrorCode(e);
    return code ? $t(`msg_media_${code.replace('.', '_')}` as 'msg_media_err_network') : messengerError(e);
  }

  const failure = $derived.by(() => {
    const raw = error || live?.failure_reason || (phase === 'upload_failed' ? m.failure_reason : null);
    if (!raw) return '';
    const code = mediaErrorCode(raw);
    return code ? $t(`msg_media_${code.replace('.', '_')}` as 'msg_media_err_network') : raw;
  });

  async function loadPreview() {
    if (!previewable || src) return;
    try { src = await messengerApi.media.dataUrl(m.id); }
    catch { previewFailed = true; }
  }

  async function download(manual: boolean) {
    if (busy) return;
    busy = true; error = '';
    try {
      const p = await messengerApi.media.download(m.id, manual);
      if (p) { local = p; await loadPreview(); }
    } catch (e) { if (manual) error = explain(e); }
    finally { busy = false; }
  }

  async function act(fn: () => Promise<unknown>) {
    error = '';
    try { await fn(); } catch (e) { error = explain(e); }
  }

  async function open() {
    await act(() => messengerApi.media.open(m.id));
  }

  async function saveAs() {
    if (!isTauriHost || !media) return;
    await act(async () => {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const dest = await save({ defaultPath: media.name });
      if (dest) await messengerApi.media.saveAs(m.id, dest);
    });
  }

  // A download that finished while we were watching.
  $effect(() => {
    if (live?.direction === 'down' && live.status === 'done' && live.local_path && !local) {
      local = live.local_path;
      loadPreview();
    }
  });

  onMount(() => {
    transferStore.hydrate(m.id).catch(() => {});
    if (media?.local_path) { local = media.local_path; loadPreview(); }
    else if (!out && !m.id.startsWith('local:')) download(false);
  });
</script>

{#if media}
  <div class="media {media.kind}" class:out>
    {#if phase === 'here' && src && media.kind === 'image'}
      <button class="thumb" onclick={open} title={$t('msg_media_open')}>
        <img {src} alt={media.name} onerror={() => { previewFailed = true; src = null; }} />
      </button>
    {:else if phase === 'here' && src && media.kind === 'video'}
      <!-- svelte-ignore a11y_media_has_caption -->
      <video class="player" {src} controls preload="metadata"></video>
    {:else if phase === 'here' && src && media.kind === 'audio'}
      <audio class="audio" {src} controls preload="metadata"></audio>
    {/if}

    <div class="file-row">
      <span class="ico" class:spin={phase === 'uploading' || phase === 'downloading'}>
        {#if phase === 'uploading' || phase === 'downloading'}<Icon name="loader" size={18} />
        {:else if phase === 'remote' || phase === 'download_paused' || phase === 'download_failed'}<Icon name="download" size={18} />
        {:else if phase === 'upload_failed'}<Icon name="alert-triangle" size={18} />
        {:else if media.kind === 'image'}<Icon name="image" size={18} />
        {:else if media.kind === 'video'}<Icon name="video" size={18} />
        {:else if media.kind === 'audio'}<Icon name="mic" size={18} />
        {:else}<Icon name="file" size={18} />{/if}
      </span>
      <span class="info">
        <span class="name" title={media.name}>{media.name}</span>
        <span class="sub">
          {#if phase === 'uploading'}{$t('msg_media_uploading', { pct: String(progress), size: bytes(media.size) })}
          {:else if phase === 'downloading'}{$t('msg_media_downloading', { pct: String(progress), size: bytes(media.size) })}
          {:else if phase === 'upload_paused' || phase === 'download_paused'}{$t('msg_media_paused', { pct: String(progress) })}
          {:else}{bytes(media.size)}{/if}
        </span>
      </span>
      <span class="actions">
        {#if phase === 'uploading' || phase === 'downloading'}
          {#if transferId}
            <button class="act" onclick={() => act(() => messengerApi.media.pause(transferId))} title={$t('msg_media_pause')}><Icon name="square" size={13} /></button>
            <button class="act" onclick={() => act(() => messengerApi.media.cancel(transferId))} title={$t('msg_media_cancel')}><Icon name="x" size={14} /></button>
          {/if}
        {:else if phase === 'upload_paused' || phase === 'upload_failed'}
          {#if transferId}
            <button class="act" onclick={() => act(() => messengerApi.media.resume(transferId))} title={$t('msg_media_resume')}><Icon name="play" size={13} /></button>
            <button class="act" onclick={() => act(() => messengerApi.media.cancel(transferId))} title={$t('msg_media_cancel')}><Icon name="x" size={14} /></button>
          {/if}
        {:else if phase === 'remote' || phase === 'download_paused' || phase === 'download_failed'}
          <button class="act primary" disabled={busy} onclick={() => download(true)} title={$t('msg_media_download')}><Icon name="download" size={14} /></button>
        {:else}
          <button class="act" onclick={open} title={$t('msg_media_open')}><Icon name="external-link" size={14} /></button>
          <button class="act" onclick={saveAs} title={$t('msg_media_save_as')}><Icon name="save" size={14} /></button>
        {/if}
      </span>
    </div>

    {#if phase === 'uploading' || phase === 'downloading' || phase === 'upload_paused' || phase === 'download_paused'}
      <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progress}>
        <span style="width:{progress}%"></span>
      </div>
    {/if}
    {#if failure}<div class="fail">{failure}</div>{/if}
  </div>
{/if}

<style>
  .media { display: flex; flex-direction: column; gap: 6px; min-width: 240px; max-width: 360px; }
  .thumb { border: none; padding: 0; background: none; cursor: zoom-in; border-radius: 10px; overflow: hidden; display: block; }
  .thumb img { display: block; max-width: 100%; max-height: 320px; object-fit: contain; border-radius: 10px; background: var(--surface-3); }
  .player { max-width: 100%; max-height: 320px; border-radius: 10px; background: #000; }
  .audio { width: 100%; height: 36px; }
  .file-row { display: flex; align-items: center; gap: var(--sp-2); }
  .ico {
    width: 38px; height: 38px; flex-shrink: 0; border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    background: var(--surface-3); color: var(--accent-text-2);
  }
  .out .ico { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .spin :global(svg) { animation: spin 1.1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .info { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
  .name { font-size: var(--fs-sm); font-weight: var(--fw-semibold); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sub { font-size: var(--fs-2xs); color: var(--text-3); }
  .actions { display: inline-flex; gap: 2px; flex-shrink: 0; }
  .act { border: none; background: none; color: var(--text-2); cursor: pointer; display: inline-flex; padding: 6px; border-radius: var(--radius-sm); }
  .act:hover:not(:disabled) { color: var(--text); background: var(--surface-3); }
  .act.primary { color: var(--accent-text-2); }
  .act:disabled { opacity: 0.4; cursor: default; }
  .bar { height: 3px; border-radius: 2px; background: var(--surface-3); overflow: hidden; }
  .bar span { display: block; height: 100%; background: var(--accent); transition: width 0.25s var(--ease); }
  .fail { font-size: var(--fs-2xs); color: var(--danger-text); }
</style>
