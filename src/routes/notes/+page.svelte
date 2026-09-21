<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { isMobile } from '$lib/platform';
  import DesktopNotes from '$lib/components/desktop/pages/NotesPage.svelte';
  import MobileNotes from '$lib/components/mobile/pages/NotesPage.svelte';
</script>

{#if isMobile}
  <MobileNotes />
{:else}
  <DesktopNotes />
{/if}
