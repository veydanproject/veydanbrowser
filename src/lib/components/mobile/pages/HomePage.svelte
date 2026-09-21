<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/mobile/i18n';
  import { APPS, loadDefaultApp } from '$lib/mobile/apps';

  let ready = $state(false);

  function greeting(): 'home_morning' | 'home_day' | 'home_evening' | 'home_night' {
    const h = new Date().getHours();
    if (h < 5) return 'home_night';
    if (h < 12) return 'home_morning';
    if (h < 18) return 'home_day';
    return 'home_evening';
  }

  onMount(() => {
    const id = loadDefaultApp();
    const app = APPS.find((a) => a.id === id && a.id !== 'settings');
    if (app) {
      goto(app.route, { replaceState: true });
      return;
    }
    ready = true;
  });
</script>

{#if ready}
  <div class="m-page home">
    <header class="brand">
      <img src="/logo.png" alt="" class="logo" />
      <h1>Veydan</h1>
      <p>{$t(greeting())}</p>
    </header>

    <a class="search" href="/search">
      <Icon name="search" size={18} />
      <span>{$t('home_search')}</span>
    </a>

    <div class="m-grid">
      {#each APPS.filter((a) => a.id !== 'settings') as app, i (app.id)}
        <a href={app.route} class="m-tile" class:active={i === 0}>
          <Icon name={app.icon} size={24} />
          <span class="label">{$t(app.title)}</span>
        </a>
      {/each}
    </div>
  </div>
{/if}

<style>
  .home { padding-top: var(--sp-8); }
  .brand {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    margin-bottom: var(--sp-6);
  }
  .logo { width: 64px; height: 64px; object-fit: contain; margin-bottom: var(--sp-2); }
  h1 { margin: 0; font-size: 22px; font-weight: 800; letter-spacing: -0.02em; }
  p { margin: 0; color: var(--text-2); font-size: 15px; }
  .search {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 44px;
    padding: 0 14px;
    border-radius: 12px;
    background: var(--m-field);
    color: var(--text-3);
    font-size: 15px;
    text-decoration: none;
    margin-bottom: var(--sp-5);
  }
</style>
