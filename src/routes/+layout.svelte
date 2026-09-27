<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { isMobile } from '$lib/platform';
  import { api, type MigrationPhase } from '$lib/api';

  let { children }: { children: Snippet } = $props();

  // Only this platform's shell is fetched, so its CSS cannot leak into the other UI.
  const shell = isMobile
    ? import('$lib/components/mobile/MobileShell.svelte')
    : import('$lib/components/desktop/DesktopShell.svelte');

  // Legacy Veydan Browser data migration gate. Remove in 4.0.
  // The shell (and its backend calls) is mounted only once the migration is
  // finished or was never needed; on mobile there is nothing to migrate.
  let migration = $state<MigrationPhase | null>(null);
  let migrationPassed = $state(isMobile);
  let migrationScreen = $state<Promise<typeof import('$lib/components/desktop/MigrationScreen.svelte')> | null>(null);

  if (!isMobile) {
    api.migration.status().then((p) => {
      // Desktop-only import: keeps its CSS out of the mobile bundle.
      if (p.phase !== 'none' && p.phase !== 'done') {
        migrationScreen = import('$lib/components/desktop/MigrationScreen.svelte');
      }
      migration = p;
      if (p.phase === 'none' || p.phase === 'done') migrationPassed = true;
    });
  }
</script>

{#if migrationPassed}
  {#await shell then { default: Shell }}
    <Shell>{@render children()}</Shell>
  {/await}
{:else if migration && migrationScreen}
  {#await migrationScreen then { default: MigrationScreen }}
    <MigrationScreen initial={migration} onfinished={() => (migrationPassed = true)} />
  {/await}
{/if}
