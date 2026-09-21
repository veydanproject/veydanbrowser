<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { isMobile } from '$lib/platform';
  import DesktopSettings from '$lib/components/desktop/pages/SettingsPage.svelte';
  import MobileSettings from '$lib/components/mobile/pages/SettingsPage.svelte';
</script>

{#if isMobile}
  <MobileSettings />
{:else}
  <DesktopSettings />
{/if}
