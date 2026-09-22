<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { t } from '$lib/mobile/i18n';
  import type { NavItem } from '$lib/mobile/nav';

  interface Props {
    /** Items around the centre hub button: first half left, second half right. */
    items: NavItem[];
    activeId: string;
    hubActive?: boolean;
    onhub: () => void;
  }

  let { items, activeId, hubActive = false, onhub }: Props = $props();

  const half = $derived(Math.ceil(items.length / 2));
  const left = $derived(items.slice(0, half));
  const right = $derived(items.slice(half));
</script>

{#snippet item(it: NavItem)}
  {#if it.href}
    <a href={it.href} class="item" class:active={activeId === it.id}>
      <Icon name={it.icon} size={22} />
      <span class="label">{$t(it.title)}</span>
    </a>
  {:else}
    <button type="button" class="item" class:active={activeId === it.id} onclick={it.onclick}>
      <Icon name={it.icon} size={22} />
      <span class="label">{$t(it.title)}</span>
    </button>
  {/if}
{/snippet}

<nav class="nav" style:--cols={items.length + 1}>
  {#each left as it (it.id)}{@render item(it)}{/each}
  <button type="button" class="item item--hub" class:active={hubActive} onclick={onhub} aria-label="Veydan">
    <span class="hub"><img src="/logo.png" alt="" /></span>
    <span class="label">Veydan</span>
  </button>
  {#each right as it (it.id)}{@render item(it)}{/each}
</nav>

<style>
  .nav {
    flex-shrink: 0;
    display: grid;
    grid-template-columns: repeat(var(--cols), 1fr);
    height: calc(var(--nav-h) + var(--sab));
    padding-bottom: var(--sab);
    background: var(--m-nav);
    --hub-ring: var(--m-nav);
    border-top: 1px solid var(--border);
  }
  .item {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    padding: 0;
    border: 0;
    border-radius: 0;
    background: transparent;
    color: var(--text-2);
    text-decoration: none;
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    transition: color var(--dur-fast);
  }
  .item.active { color: var(--accent); }
  .item:active { color: var(--text); }
  .item--hub { justify-content: flex-end; padding-bottom: 7px; }
  .hub {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 50px;
    height: 50px;
    border-radius: 50%;
    background: var(--m-hub);
    box-shadow: 0 0 0 4px var(--hub-ring), var(--m-hub-shadow);
    transform: translateY(-10px);
    transition: transform var(--dur-fast), filter var(--dur-fast), background var(--dur-fast), box-shadow var(--dur-fast);
  }
  .hub img { width: 32px; height: 32px; object-fit: contain; }
  .item--hub.active .hub { filter: brightness(1.15); }
  .item--hub:active .hub { transform: translateY(-10px) scale(0.94); }
  .item--hub .label { margin-top: -8px; }
</style>
