<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { syncStore } from '$lib/store/sync.svelte';
  import { notesStore } from '$lib/store/notes.svelte';
  import { t } from '$lib/i18n';
  import Icon from '$lib/Icon.svelte';

  onMount(() => {
    let off: (() => void) | null = null;
    void syncStore.listen().then((fn) => (off = fn));
    return () => off?.();
  });

  const running = $derived(syncStore.busy || syncStore.status?.running === true);

  const title = $derived.by(() => {
    const s = syncStore.status;
    if (running) return $t('notes_sync_running');
    if (!s?.joined) return `${$t('notes_btn_sync')} · ${$t('notes_sync_not_joined')}`;
    const last = s.last_run ? new Date(s.last_run).toLocaleString() : $t('notes_sync_never');
    const err = syncStore.error || s.last_error;
    let base = `${$t('notes_btn_sync')} · ${$t('notes_sync_last')}: ${last}`;
    if (s.last_applied !== null) base += ` · ${$t('notes_sync_applied', { n: String(s.last_applied) })}`;
    return err ? `${base}\n${$t('notes_sync_error')}: ${err}` : base;
  });

  const hasError = $derived(!running && !!(syncStore.error || syncStore.status?.last_error));

  async function run() {
    await syncStore.runNow();
    await notesStore.refresh();
  }
</script>

<button class="icon-btn" class:danger={hasError} {title} disabled={running} onclick={run}>
  <span class:spin={running}><Icon name="refresh-cw" size={14} /></span>
</button>

<style>
  span { display: inline-flex; }
</style>
