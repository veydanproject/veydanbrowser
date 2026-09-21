<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { isMobile } from '$lib/platform';
  import DesktopShell from '$lib/components/desktop/DesktopShell.svelte';
  import MobileShell from '$lib/components/mobile/MobileShell.svelte';

  let { children }: { children: Snippet } = $props();
</script>

{#if isMobile}
  <MobileShell>{@render children()}</MobileShell>
{:else}
  <DesktopShell>{@render children()}</DesktopShell>
{/if}
