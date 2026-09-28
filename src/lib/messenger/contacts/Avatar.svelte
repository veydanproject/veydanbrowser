<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  interface Props {
    url: string | null;
    label: string;
    size?: number;
  }
  let { url, label, size = 36 }: Props = $props();
  let failed = $state(false);
  const initials = $derived(label.trim().slice(0, 2).toUpperCase() || '?');
  // Only https images; anything else falls back to initials (no plain-http leaks).
  const src = $derived(url && url.startsWith('https://') && !failed ? url : null);
</script>

{#if src}
  <img class="avatar" {src} alt="" width={size} height={size} loading="lazy" referrerpolicy="no-referrer" onerror={() => (failed = true)} />
{:else}
  <span class="avatar initials" style="width:{size}px;height:{size}px;font-size:{Math.round(size * 0.38)}px">{initials}</span>
{/if}

<style>
  .avatar { border-radius: 50%; flex-shrink: 0; object-fit: cover; background: var(--surface-2); }
  .initials { display: inline-flex; align-items: center; justify-content: center; font-weight: var(--fw-bold); color: var(--accent-text-2); background: var(--accent-tint); }
</style>
