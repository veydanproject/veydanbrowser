<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!--
  Generic right-click menu. Positioned at a viewport coordinate and clamped to
  stay on screen; closes on outside-click, Escape, or after an item fires.
  Modelled on CustomSelect's fixed-positioned popover.
-->
<script lang="ts">
  import { onMount, onDestroy, type Snippet } from 'svelte';
  import { portal } from '$lib/portal';
  import Icon from '$lib/Icon.svelte';

  export interface MenuItem {
    type?: 'item';
    label: string;
    icon?: string;
    danger?: boolean;
    disabled?: boolean;
    shortcut?: string;
    onselect: () => void;
  }
  export type MenuEntry = MenuItem | { type: 'separator' };

  interface Props {
    open: boolean;
    x: number;
    y: number;
    items: MenuEntry[];
    onclose: () => void;
    /** Optional header rendered above the items (e.g. the target name). */
    header?: Snippet;
  }

  let { open = $bindable(), x, y, items, onclose, header }: Props = $props();

  let menuEl = $state<HTMLDivElement | null>(null);
  let style = $state('');

  function reposition() {
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const w = menuEl?.offsetWidth ?? 220;
    const h = menuEl?.offsetHeight ?? 240;
    let left = x;
    let top = y;
    if (left + w > vw - 4) left = vw - w - 4;
    if (top + h > vh - 4) top = Math.max(4, vh - h - 4);
    style = `left:${left}px;top:${top}px`;
  }

  $effect(() => {
    if (open) {
      // measure after render, then clamp
      style = `left:${x}px;top:${y}px;visibility:hidden`;
      requestAnimationFrame(reposition);
    }
  });

  function close() {
    open = false;
    onclose();
  }

  function fire(item: MenuItem) {
    if (item.disabled) return;
    close();
    item.onselect();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      e.stopPropagation();
      close();
    }
  }
  function onOutside(e: MouseEvent) {
    if (open && menuEl && !menuEl.contains(e.target as Node)) close();
  }

  onMount(() => {
    document.addEventListener('mousedown', onOutside, true);
    document.addEventListener('contextmenu', onOutside, true);
    document.addEventListener('keydown', onKeydown, true);
  });
  onDestroy(() => {
    document.removeEventListener('mousedown', onOutside, true);
    document.removeEventListener('contextmenu', onOutside, true);
    document.removeEventListener('keydown', onKeydown, true);
  });
</script>

{#if open}
  <div bind:this={menuEl} class="context-menu" style={style} use:portal role="menu">
    {#if header}
      <div class="menu-header">{@render header()}</div>
    {/if}
    {#each items as item (item)}
      {#if item.type === 'separator'}
        <div class="menu-sep"></div>
      {:else}
        <button
          class="menu-item"
          class:danger={item.danger}
          disabled={item.disabled}
          role="menuitem"
          onclick={() => fire(item)}
        >
          {#if item.icon}<Icon name={item.icon} size={14} />{:else}<span class="icon-gap"></span>{/if}
          <span class="menu-label">{item.label}</span>
          {#if item.shortcut}<span class="menu-shortcut">{item.shortcut}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: var(--z-popover);
    min-width: 200px;
    max-width: 320px;
    padding: 4px;
    background: var(--surface-drawer);
    border: 1px solid var(--border-2);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
  }

  .menu-header {
    padding: var(--sp-1) var(--sp-2) var(--sp-2);
    margin-bottom: 4px;
    border-bottom: 1px solid var(--border);
    color: var(--text-2);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-item {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    padding: var(--sp-2) var(--sp-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--text);
    font-size: var(--fs-sm);
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }
  .menu-item:hover:not(:disabled) { background: var(--surface-hover); }
  .menu-item:disabled { color: var(--text-3); cursor: default; }
  .menu-item.danger { color: var(--danger-text); }
  .menu-item.danger:hover:not(:disabled) { background: var(--danger-bg); }

  .menu-item :global(svg) { flex-shrink: 0; color: var(--text-2); }
  .menu-item.danger :global(svg) { color: var(--danger-text); }
  .icon-gap { width: 14px; flex-shrink: 0; }

  .menu-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .menu-shortcut {
    flex-shrink: 0;
    color: var(--text-3);
    font-size: var(--fs-xs);
    font-family: var(--font-mono);
  }

  .menu-sep {
    height: 1px;
    margin: 4px 2px;
    background: var(--border);
  }
</style>
