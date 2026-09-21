<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { isMobile } from '$lib/platform';
  import DesktopHome from '$lib/components/desktop/pages/HomePage.svelte';
  import MobileHome from '$lib/components/mobile/pages/HomePage.svelte';
</script>

{#if isMobile}
  <MobileHome />
{:else}
  <DesktopHome />
{/if}
