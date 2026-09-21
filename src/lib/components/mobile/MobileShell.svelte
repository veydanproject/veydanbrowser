<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import '@fontsource-variable/manrope/index.css';
  import '@fontsource-variable/jetbrains-mono/index.css';
  import '$lib/styles/tokens.css';
  import '$lib/styles/base.css';
  import '$lib/styles/mobile.css';
  import { onMount, type Snippet } from 'svelte';
  import { page } from '$app/state';
  import { theme } from '$lib/theme';
  import { api } from '$lib/mobile/api';
  import { navActive, navKind, notesNav, rootNav } from '$lib/mobile/nav';
  import BottomNav from '$lib/components/mobile/BottomNav.svelte';
  import HubSheet from '$lib/components/mobile/HubSheet.svelte';
  import NotesMoreSheet from '$lib/components/mobile/NotesMoreSheet.svelte';

  let { children }: { children: Snippet } = $props();

  let hubOpen = $state(false);
  let moreOpen = $state(false);

  const kind = $derived(navKind(page.url.pathname));
  const active = $derived(navActive(page.url.pathname));
  const items = $derived(kind === 'notes' ? notesNav(() => (moreOpen = true)) : rootNav());

  // Mobile defaults to the light theme; the shared store defaults to dark and
  // has already persisted it, so a one-time flag marks the first launch.
  if (typeof localStorage !== 'undefined' && !localStorage.getItem('m_theme_init')) {
    localStorage.setItem('m_theme_init', '1');
    theme.set('light');
  }

  // Subscribing applies data-theme to <body>.
  $effect(() => {
    void $theme;
  });

  // Android freezes the process in background; catch up when the app comes back.
  onMount(() => {
    const onVisible = () => {
      if (document.visibilityState === 'visible') api.sync.trigger().catch(() => {});
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  });
</script>

<div class="shell">
  <div class="screen">
    {@render children()}
  </div>
  {#if kind}
    <BottomNav {items} activeId={active} hubActive={hubOpen} onhub={() => (hubOpen = true)} />
  {/if}
</div>

<HubSheet open={hubOpen} onclose={() => (hubOpen = false)} />
<NotesMoreSheet open={moreOpen} onclose={() => (moreOpen = false)} />

<style>
  .shell {
    height: 100dvh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }
  .screen {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    padding-top: env(safe-area-inset-top);
    background: var(--bg);
  }
  .screen > :global(.m-page) {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .screen > :global(.editor) {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
</style>
