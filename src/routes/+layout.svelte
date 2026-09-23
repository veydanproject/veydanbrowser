<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { isMobile } from '$lib/platform';

  let { children }: { children: Snippet } = $props();

  // Only this platform's shell is fetched, so its CSS cannot leak into the other UI.
  const shell = isMobile
    ? import('$lib/components/mobile/MobileShell.svelte')
    : import('$lib/components/desktop/DesktopShell.svelte');
</script>

{#await shell then { default: Shell }}
  <Shell>{@render children()}</Shell>
{/await}
